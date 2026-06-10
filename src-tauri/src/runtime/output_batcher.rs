//! Coalesces raw PTY bytes into bounded, UTF-8-safe flushes before IPC.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Length of the longest prefix of `bytes` that ends on a UTF-8 character
/// boundary. A trailing incomplete sequence is excluded so a 4KB read boundary
/// can never split a char into two lossy (U+FFFD) halves.
fn utf8_complete_prefix_len(bytes: &[u8]) -> usize {
    let len = bytes.len();
    // start of the last (possibly incomplete) char: a non-continuation byte
    // within the final 4 bytes
    let start = (1..=4.min(len))
        .map(|back| len - back)
        .find(|&i| bytes[i] & 0xC0 != 0x80)
        .unwrap_or(0);
    let first = bytes[start];
    let need = if first < 0x80 {
        1
    } else if first & 0xE0 == 0xC0 {
        2
    } else if first & 0xF0 == 0xE0 {
        3
    } else if first & 0xF8 == 0xF0 {
        4
    } else {
        1 // invalid lead byte — let from_utf8_lossy deal with it
    };
    if len - start < need {
        start
    } else {
        len
    }
}

fn flush(
    pending: &mut Vec<u8>,
    seq: &mut u64,
    emit: &mut impl FnMut(String, u64),
    final_flush: bool,
) {
    if pending.is_empty() {
        return;
    }
    let cut = if final_flush {
        pending.len() // stream over — emit the dangling partial char lossily
    } else {
        utf8_complete_prefix_len(pending)
    };
    if cut == 0 {
        return; // only a partial char buffered — wait for its continuation bytes
    }
    let chunk: Vec<u8> = pending.drain(..cut).collect();
    *seq += chunk.len() as u64;
    emit(String::from_utf8_lossy(&chunk).to_string(), *seq);
}

/// Coalesce incoming byte chunks and emit them as strings: flush when the
/// pending buffer reaches `max_bytes` or `window` after the first pending byte,
/// whichever comes first. `seq` passed to `emit` is the cumulative count of
/// UTF-8 bytes emitted — the dedup foundation for snapshot recovery.
/// Returns when the sender side disconnects, after a final lossy flush.
#[allow(dead_code)]
// Why: wired up in the next commit
pub fn batch_loop(
    rx: Receiver<Vec<u8>>,
    window: Duration,
    max_bytes: usize,
    mut emit: impl FnMut(String, u64),
) {
    let mut pending: Vec<u8> = Vec::new();
    let mut seq: u64 = 0;
    // invariant: pending non-empty ⇒ deadline Some (a held partial char keeps
    // rescheduling — a harmless ~window-rate wake while its continuation is late)
    let mut deadline: Option<Instant> = None;
    loop {
        let msg = match deadline {
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    None
                } else {
                    match rx.recv_timeout(d - now) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
        };
        match msg {
            Some(bytes) => {
                if pending.is_empty() {
                    deadline = Some(Instant::now() + window);
                }
                pending.extend_from_slice(&bytes);
                if pending.len() >= max_bytes {
                    flush(&mut pending, &mut seq, &mut emit, false);
                    deadline = if pending.is_empty() {
                        None
                    } else {
                        Some(Instant::now() + window)
                    };
                }
            }
            None => {
                // window elapsed
                flush(&mut pending, &mut seq, &mut emit, false);
                deadline = if pending.is_empty() {
                    None
                } else {
                    Some(Instant::now() + window)
                };
            }
        }
    }
    flush(&mut pending, &mut seq, &mut emit, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};

    /// Run batch_loop on a thread; returns (byte sender, emitted (chunk, seq) log).
    fn run(
        window: Duration,
        max_bytes: usize,
    ) -> (
        std::sync::mpsc::Sender<Vec<u8>>,
        Arc<Mutex<Vec<(String, u64)>>>,
        std::thread::JoinHandle<()>,
    ) {
        let (tx, rx) = channel::<Vec<u8>>();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let h = std::thread::spawn(move || {
            batch_loop(rx, window, max_bytes, move |chunk, seq| {
                e.lock().unwrap().push((chunk, seq));
            });
        });
        (tx, emitted, h)
    }

    #[test]
    fn coalesces_small_writes_into_one_emit() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        tx.send(b"a".to_vec()).unwrap();
        tx.send(b"b".to_vec()).unwrap();
        tx.send(b"c".to_vec()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1, "one window → one emit: {log:?}");
            assert_eq!(log[0].0, "abc");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn flushes_immediately_at_max_bytes() {
        let (tx, emitted, h) = run(Duration::from_secs(10), 8); // window never fires
        tx.send(b"0123456789".to_vec()).unwrap(); // 10 ≥ 8 → immediate flush
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(emitted.lock().unwrap().len(), 1, "size flush, no window wait");
        assert_eq!(emitted.lock().unwrap()[0].0, "0123456789");
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn multibyte_char_split_across_reads_stays_intact() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        let e = "é".as_bytes(); // [0xC3, 0xA9]
        tx.send(vec![e[0]]).unwrap();
        std::thread::sleep(Duration::from_millis(120)); // window fires — must hold the partial char
        assert!(emitted.lock().unwrap().is_empty(), "partial char must not emit");
        tx.send(vec![e[1]]).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].0, "é", "no U+FFFD from a split char");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn seq_accumulates_emitted_bytes() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        tx.send(b"abc".to_vec()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        tx.send("é".as_bytes().to_vec()).unwrap(); // 2 bytes
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log[0].1, 3, "seq after first flush = bytes so far");
            assert_eq!(log[1].1, 5, "seq is cumulative UTF-8 bytes");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn disconnect_flushes_remainder_lossy() {
        let (tx, emitted, h) = run(Duration::from_secs(10), 16 * 1024); // window never fires
        tx.send(b"tail".to_vec()).unwrap();
        tx.send(vec![0xC3]).unwrap(); // dangling partial char
        drop(tx); // PTY closed
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        assert_eq!(log.len(), 1, "final flush on disconnect");
        assert!(log[0].0.starts_with("tail"), "buffered bytes emitted");
        assert_eq!(log[0].1, 5, "seq counts the dangling byte too");
    }
}
