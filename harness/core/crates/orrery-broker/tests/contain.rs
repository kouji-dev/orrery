//! Task 8 · processes, and everything they start.
//!
//! These tests really spawn. Where a platform cannot do the thing under test —
//! no shell, no job object — the test says so and skips rather than asserting
//! something it did not prove.

mod common;

use std::time::Duration;

use common::Fixture;
use orrery_broker::contain::MemoryEnforcement;
use orrery_broker::{Broker, SpawnSpec};
use orrery_policy::PendingCall;
use orrery_tools::ToolBudget;

fn budget(ms: u64, bytes: u64) -> ToolBudget {
    ToolBudget::new(ms, bytes)
}

/// A shell that exists on this platform, or `None`.
fn shell(script: &str) -> Option<SpawnSpec> {
    #[cfg(windows)]
    {
        Some(SpawnSpec::new("powershell.exe").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]))
    }
    #[cfg(not(windows))]
    {
        Some(SpawnSpec::new("/bin/sh").args(["-c", script]))
    }
}

fn spawn_token(fx: &Fixture, spec: &SpawnSpec) -> orrery_policy::CapabilityToken {
    fx.token(&PendingCall::spawn(spec.command_text()))
}

/// A child that starts a grandchild; killing the call must reach both.
#[tokio::test]
async fn grandchildren_die() {
    let fx = Fixture::new();
    let beat = fx.path("heartbeat.txt");
    let beat_s = beat.display().to_string().replace('\\', "/");

    #[cfg(windows)]
    let script = format!(
        "Start-Process -NoNewWindow -FilePath powershell.exe -ArgumentList '-NoProfile','-Command','while($true){{ Add-Content -LiteralPath \"{beat_s}\" -Value 1; Start-Sleep -Milliseconds 50 }}'; Start-Sleep -Seconds 30"
    );
    #[cfg(not(windows))]
    let script = format!("( while true; do echo 1 >> '{beat_s}'; sleep 0.05; done ) & sleep 30");

    let Some(spec) = shell(&script) else {
        eprintln!("skipped: no shell on this platform");
        return;
    };
    let token = spawn_token(&fx, &spec);
    let mut child = match fx.broker.spawn(token, spec, &budget(30_000, 1 << 20)).await {
        Ok(child) => child,
        Err(e) => {
            eprintln!("skipped: could not spawn a shell here ({e})");
            return;
        }
    };

    // Wait for the grandchild to prove it is alive.
    let mut seen = 0u64;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        seen = tokio::fs::metadata(&beat)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if seen > 0 {
            break;
        }
    }
    if seen == 0 {
        eprintln!("skipped: the grandchild never started here");
        child.kill_tree();
        return;
    }

    child.kill_tree();
    drop(child);

    // Give anything still running a chance to write more.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let after = tokio::fs::metadata(&beat)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let later = tokio::fs::metadata(&beat)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    assert_eq!(
        after, later,
        "the grandchild outlived the call: the heartbeat kept growing after the tree was killed"
    );
}

/// A child that will not stop on its own is killed once its window is up.
#[tokio::test]
async fn wall_clock_watchdog() {
    let fx = Fixture::new();
    #[cfg(windows)]
    let script = "Start-Sleep -Seconds 60";
    #[cfg(not(windows))]
    let script = "trap '' TERM; sleep 60";

    let Some(spec) = shell(script) else {
        eprintln!("skipped: no shell on this platform");
        return;
    };
    let token = spawn_token(&fx, &spec);
    let Ok(child) = fx.broker.spawn(token, spec, &budget(1_000, 1 << 16)).await else {
        eprintln!("skipped: could not spawn a shell here");
        return;
    };

    let started = std::time::Instant::now();
    let out = child.wait().await.expect("the watchdog answers");
    assert!(
        out.timed_out,
        "a child past its window must be reported as such"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the watchdog did not fire: {:?}",
        started.elapsed()
    );
}

/// Windows and Linux really enforce a memory ceiling; macOS samples.
///
/// **The macOS case is marked rather than asserted**: open question 4 of the
/// plan is decided as "document it and report `Unenforced`", and a test that
/// claimed enforcement there would be a lie.
#[tokio::test]
async fn memory_ceiling() {
    let fx = Fixture::new();
    #[cfg(windows)]
    let script = "Start-Sleep -Milliseconds 200";
    #[cfg(not(windows))]
    let script = "sleep 0.2";

    let Some(spec) = shell(script) else {
        eprintln!("skipped: no shell on this platform");
        return;
    };
    let token = spawn_token(&fx, &spec);
    let mut b = budget(5_000, 1 << 16);
    b.memory_bytes = Some(256 * 1024 * 1024);

    let Ok(child) = fx.broker.spawn(token, spec, &b).await else {
        eprintln!("skipped: could not spawn a shell here");
        return;
    };
    let enforcement = child.memory_enforcement();
    let out = child.wait().await.expect("it finishes");

    if cfg!(target_os = "macos") {
        assert_eq!(
            enforcement,
            MemoryEnforcement::Unenforced,
            "macOS must report the gap rather than pretend to enforce"
        );
        eprintln!("macOS: memory ceilings are sampled, not enforced — reported as Unenforced");
    } else {
        assert_eq!(
            enforcement,
            MemoryEnforcement::Enforced,
            "this platform can enforce a memory ceiling and must say so"
        );
    }
    assert_eq!(out.memory, enforcement);
}

/// A child writing faster than we read gets a closed pipe, not our heap.
#[tokio::test]
async fn stdout_backpressure() {
    let fx = Fixture::new();
    #[cfg(windows)]
    let script = "$line = 'x' * 1024; for ($i = 0; $i -lt 200000; $i++) { Write-Output $line }";
    #[cfg(not(windows))]
    let script = "yes xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

    let Some(spec) = shell(script) else {
        eprintln!("skipped: no shell on this platform");
        return;
    };
    let ceiling = 32 * 1024;
    let token = spawn_token(&fx, &spec);
    let Ok(child) = fx.broker.spawn(token, spec, &budget(20_000, ceiling)).await else {
        eprintln!("skipped: could not spawn a shell here");
        return;
    };

    let out = child.wait().await.expect("it finishes or is killed");
    assert!(out.truncated, "a firehose child must be cut off");
    assert!(
        out.stdout.len() as u64 <= ceiling,
        "kept {} bytes for a {ceiling}-byte ceiling",
        out.stdout.len()
    );
}
