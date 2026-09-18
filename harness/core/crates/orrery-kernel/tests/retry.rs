//! Plan 05, Task 6: the kernel retries, charged and audited. Providers never
//! sleep.

mod common;

use std::sync::Arc;

use common::{Passes, Rig, TestHost, fixture, registry, write_stream};
use orrery_audit::AuditEvent;
use orrery_kernel::{Kernel, KernelConfig, RetryPolicy, TurnInput, TurnOutcome};
use orrery_proto::UserInput;
use tokio_util::sync::CancellationToken;

/// A stream that fails with a 429 and says how long to wait.
const RATE_LIMITED: &[&str] = &[
    r#"{"t":"started","id":"msg_429"}"#,
    r#"{"error":{"code":"rate_limited","retry_after_ms":10}}"#,
];

/// A stream that fails terminally.
const UNAUTHORISED: &[&str] = &[r#"{"error":{"code":"auth","message":"the key was rejected"}}"#];

fn kernel(
    rig: &Rig,
    provider: Arc<dyn orrery_provider::Provider>,
    retry: RetryPolicy,
    audit: orrery_audit::Audit,
) -> Kernel {
    Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            retry,
            ..KernelConfig::default()
        },
    )
    .with_audit(audit)
}

async fn run(rig: &Rig, kernel: &Kernel) -> TurnOutcome {
    kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn")
}

fn attempts(audit: &Arc<orrery_audit::MemorySink>) -> Vec<String> {
    audit
        .records()
        .into_iter()
        .filter_map(|r| match r.event {
            AuditEvent::Content { action, content } if action == "provider.attempt" => {
                Some(content.scope)
            }
            _ => None,
        })
        .collect()
}

/// Two rate limits, then a success: three attempts, all audited, and the
/// backoff counted against the turn's wall clock rather than paused outside it.
#[tokio::test]
async fn retryable_is_retried_and_charged() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    let audit = orrery_audit::memory();

    let provider = Passes::of(vec![
        write_stream(dir.path(), "a.jsonl", RATE_LIMITED),
        write_stream(dir.path(), "b.jsonl", RATE_LIMITED),
        fixture("text-turn.jsonl"),
    ]);
    let kernel = kernel(
        &rig,
        provider.clone(),
        RetryPolicy {
            max_attempts: 3,
            base_ms: 5,
            max_ms: 20,
            jitter: false,
        },
        audit.clone(),
    );

    let started = std::time::Instant::now();
    let outcome = run(&rig, &kernel).await;
    let elapsed = started.elapsed();

    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "the third attempt succeeded: {outcome:?}"
    );
    assert_eq!(provider.served(), 3, "three attempts, one pass");
    assert_eq!(
        attempts(&audit),
        vec!["rate_limited", "rate_limited"],
        "every failed attempt is in the audit, with its code"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(20),
        "the provider asked for 10ms twice and the kernel waited: {elapsed:?}"
    );
}

/// A terminal failure is not retried: one attempt, and the turn reports it.
#[tokio::test]
async fn terminal_is_not_retried() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    let audit = orrery_audit::memory();

    let provider = Passes::repeating(write_stream(dir.path(), "auth.jsonl", UNAUTHORISED));
    let kernel = kernel(
        &rig,
        provider.clone(),
        RetryPolicy::default(),
        audit.clone(),
    );

    match run(&rig, &kernel).await {
        TurnOutcome::Failed { code, message, .. } => {
            assert_eq!(code, "auth");
            assert!(message.contains("1 attempt"), "{message}");
        }
        other => panic!("an auth failure is terminal: {other:?}"),
    }
    assert_eq!(provider.served(), 1, "retrying would send the same bytes");
}

/// Past the cap, the turn ends with a typed outcome rather than an error.
#[tokio::test]
async fn exhaustion_is_an_outcome() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    let audit = orrery_audit::memory();

    let provider = Passes::repeating(write_stream(dir.path(), "429.jsonl", RATE_LIMITED));
    let kernel = kernel(
        &rig,
        provider.clone(),
        RetryPolicy {
            max_attempts: 2,
            base_ms: 1,
            max_ms: 2,
            jitter: false,
        },
        audit.clone(),
    );

    // The assertion is the type: `run_turn` returned `Ok`, so exhaustion came
    // back as a value. A `?` here would have propagated an error instead.
    let outcome = run(&rig, &kernel).await;
    match outcome {
        TurnOutcome::Failed { code, message, .. } => {
            assert_eq!(code, "rate_limited");
            assert!(message.contains("2 attempt"), "{message}");
        }
        other => panic!("exhaustion is an outcome: {other:?}"),
    }
    assert_eq!(provider.served(), 2);
    assert!(
        audit.records().iter().any(|r| matches!(
            &r.event,
            AuditEvent::Content { action, .. } if action == "provider.failed"
        )),
        "and the giving-up is in the audit too"
    );
}
