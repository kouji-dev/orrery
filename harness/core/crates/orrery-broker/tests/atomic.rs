//! Task 7 · writes that are all-or-nothing.

mod common;

use common::Fixture;
use orrery_broker::Broker;

#[tokio::test]
async fn write_reverts_on_cancel() {
    let fx = Fixture::new();
    let path = fx.path("notes.txt");
    tokio::fs::write(&path, b"the original").await.unwrap();

    let token = fx.write_token(&path);
    let mut handle = fx.broker.write(token, &path, true).await.unwrap();
    handle.write_all(b"half a replacement").await.unwrap();
    let temp = handle.temp_path().to_path_buf();
    assert!(temp.exists(), "an atomic write goes to a temp file first");

    // Cancelled mid-write.
    handle.cancel().await;

    assert_eq!(
        tokio::fs::read(&path).await.unwrap(),
        b"the original",
        "the original must be untouched"
    );
    assert!(
        !temp.exists(),
        "a temp file was left behind: {}",
        temp.display()
    );
}

#[tokio::test]
async fn a_dropped_handle_is_a_cancelled_write() {
    let fx = Fixture::new();
    let path = fx.path("notes.txt");
    tokio::fs::write(&path, b"the original").await.unwrap();

    let temp = {
        let token = fx.write_token(&path);
        let mut handle = fx.broker.write(token, &path, true).await.unwrap();
        handle.write_all(b"never committed").await.unwrap();
        handle.temp_path().to_path_buf()
        // dropped here, without a commit
    };

    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"the original");
    assert!(!temp.exists(), "a dropped handle left its temp file behind");
}

#[tokio::test]
async fn commit_replaces_the_original() {
    let fx = Fixture::new();
    let path = fx.path("notes.txt");
    tokio::fs::write(&path, b"the original").await.unwrap();

    let token = fx.write_token(&path);
    let mut handle = fx.broker.write(token, &path, true).await.unwrap();
    handle.write_all(b"the replacement").await.unwrap();
    let temp = handle.temp_path().to_path_buf();
    handle.commit().await.unwrap();

    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"the replacement");
    assert!(!temp.exists());
}

#[tokio::test]
async fn a_non_atomic_write_goes_straight_at_the_file() {
    let fx = Fixture::new();
    let path = fx.path("log.txt");

    let token = fx.write_token(&path);
    let mut handle = fx.broker.write(token, &path, false).await.unwrap();
    handle.write_all(b"appended").await.unwrap();
    handle.commit().await.unwrap();

    assert_eq!(tokio::fs::read(&path).await.unwrap(), b"appended");
}
