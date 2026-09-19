//! Plan 14: a guest reading a binary file gets the bytes, not mojibake.
//!
//! `broker/read` answers JSON, and a JSON string cannot carry arbitrary bytes.
//! The old reply had only `text`, produced by a lossy UTF-8 decode, so every
//! byte outside UTF-8 came back as U+FFFD and a guest reading a PNG got a
//! corrupted PNG with no way to know. `bytes_b64` is the fix, and it is
//! populated **only** when the lossy decode actually lost something, so the hot
//! path — reading source files — costs exactly what it did before.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{BrokerFacade, BrokerResult, ReadChunk, ReadRequest};
use orrery_host_rpc::protocol::{self, ReadReply};
use orrery_jsonrpc::Handler;
use orrery_host_rpc::BrokerBridge;

/// A broker that hands back whatever bytes it was built with.
struct Bytes(Vec<u8>);

#[async_trait]
impl BrokerFacade for Bytes {
    async fn read(&self, _req: ReadRequest) -> BrokerResult<ReadChunk> {
        Ok(ReadChunk {
            bytes: self.0.clone(),
            eof: true,
            total: Some(self.0.len() as u64),
        })
    }
}

fn bridge(bytes: Vec<u8>) -> BrokerBridge {
    BrokerBridge::new("ext".parse().unwrap(), Arc::new(Bytes(bytes)))
}

async fn read_reply(bytes: Vec<u8>) -> ReadReply {
    let value = bridge(bytes)
        .request(
            protocol::BROKER_READ,
            serde_json::json!({ "path": "f", "limit": 64 }),
        )
        .await
        .expect("the read succeeds");
    serde_json::from_value(value).expect("the reply is a ReadReply")
}

/// The whole point: the exact bytes survive the JSON round trip.
#[tokio::test]
async fn a_binary_read_survives_the_round_trip() {
    // A PNG signature: byte 0x89 is not valid UTF-8 on its own.
    let original = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff, 0xfe];
    let reply = read_reply(original.clone()).await;

    let encoded = reply
        .bytes_b64
        .as_deref()
        .expect("a read that lost bytes to the decode carries them base64");
    assert_eq!(
        ReadReply::decode_b64(encoded).expect("valid base64"),
        original,
        "what the guest decodes is byte-for-byte what the broker read"
    );
}

/// The hot path is unchanged: text reads carry no base64 at all.
#[tokio::test]
async fn a_text_read_carries_no_base64() {
    let reply = read_reply(b"fn main() {}\n".to_vec()).await;
    assert_eq!(reply.text, "fn main() {}\n");
    assert!(
        reply.bytes_b64.is_none(),
        "paying a third of the bandwidth on every source file is what the field avoids"
    );
}

/// An old guest, which has never heard of `bytes_b64`, still parses a new
/// reply — and an old host's reply still parses here.
#[tokio::test]
async fn the_field_is_backwards_compatible() {
    let old: ReadReply =
        serde_json::from_value(serde_json::json!({ "text": "hi", "eof": true })).expect("parses");
    assert!(old.bytes_b64.is_none());

    let reply = read_reply(vec![0xff]).await;
    let wire = serde_json::to_value(&reply).expect("serialises");
    assert!(
        wire.get("bytes_b64").is_some(),
        "the field is on the wire when there is something to put in it"
    );
}
