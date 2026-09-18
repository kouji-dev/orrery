//! Task 1: every `ProviderError` variant is classified.

use orrery_provider::ProviderError;

/// One row per variant, so a variant added without a classification shows up
/// as a missing row here and in the exhaustive match below.
fn every_variant() -> Vec<(ProviderError, bool, &'static str)> {
    vec![
        (
            ProviderError::RateLimited {
                retry_after_ms: Some(1_500),
            },
            true,
            "rate_limited",
        ),
        (
            ProviderError::RateLimited {
                retry_after_ms: None,
            },
            true,
            "rate_limited",
        ),
        (ProviderError::ServerError { status: 500 }, true, "server"),
        (
            ProviderError::Timeout { elapsed_ms: 30_000 },
            true,
            "timeout",
        ),
        (
            ProviderError::Connection("dns".to_owned()),
            true,
            "connection",
        ),
        (ProviderError::Auth("no key".to_owned()), false, "auth"),
        (
            ProviderError::BadRequest("nope".to_owned()),
            false,
            "bad_request",
        ),
        (ProviderError::Refusal("no".to_owned()), false, "refusal"),
        (
            ProviderError::ContextTooLong {
                tokens: 300_000,
                max: 200_000,
            },
            false,
            "context_too_long",
        ),
        (
            ProviderError::NoSuchModel("qwen-9".to_owned()),
            false,
            "no_such_model",
        ),
    ]
}

#[test]
fn classification_is_total() {
    for (err, retryable, code) in every_variant() {
        assert_eq!(err.is_retryable(), retryable, "is_retryable for {err:?}");
        assert_eq!(err.code(), code, "code for {err:?}");
    }
}

#[test]
fn exhaustive_match_forces_a_decision_on_new_variants() {
    fn classify(e: &ProviderError) -> bool {
        match e {
            ProviderError::RateLimited { .. }
            | ProviderError::ServerError { .. }
            | ProviderError::Timeout { .. }
            | ProviderError::Connection(_) => true,
            ProviderError::Auth(_)
            | ProviderError::BadRequest(_)
            | ProviderError::Refusal(_)
            | ProviderError::ContextTooLong { .. }
            | ProviderError::NoSuchModel(_) => false,
            // `ProviderError` is `#[non_exhaustive]`, so an out-of-crate match
            // needs an arm here; an unclassified variant reaching it is a bug.
            _ => unreachable!("unclassified provider error: {e:?}"),
        }
    }
    for (err, retryable, _) in every_variant() {
        assert_eq!(classify(&err), retryable);
    }
}

#[test]
fn display_mentions_retry_after_only_when_known() {
    assert_eq!(
        ProviderError::RateLimited {
            retry_after_ms: Some(1_500)
        }
        .to_string(),
        "rate limited, retry after 1500ms"
    );
    assert_eq!(
        ProviderError::RateLimited {
            retry_after_ms: None
        }
        .to_string(),
        "rate limited"
    );
}
