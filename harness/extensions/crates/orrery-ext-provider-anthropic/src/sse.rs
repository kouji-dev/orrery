//! Server-sent events, hand-rolled.
//!
//! The decision and its reasons are recorded in
//! `harness/docs/plans/03-provider-layer.md`, task 5. In short: Anthropic uses
//! none of what an SSE *library* exists to provide — no `id:`, no
//! `Last-Event-ID` resumption, no reconnection — so what is left is a byte
//! splitter, and this is it.
//!
//! The parser is a push machine rather than a `Stream` adapter, which is what
//! makes `handles_split_frames` a `for` loop over `chunks(7)` instead of a mock
//! stream and a runtime.

use orrery_provider::ProviderError;

/// One dispatched SSE frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, or `message` when the frame did not name one.
    pub name: String,
    /// The `data:` fields, joined with newlines.
    pub data: String,
}

/// Splits a byte stream into frames.
#[derive(Debug, Default)]
pub struct SseParser {
    /// Bytes not yet terminated by a newline. A frame may be split anywhere,
    /// including inside a UTF-8 sequence, so the buffer is bytes and decoding
    /// happens a whole line at a time.
    pending: Vec<u8>,
    name: Option<String>,
    data: String,
    /// True once any field of the current frame has been seen, so that a frame
    /// carrying only `event:` still dispatches.
    open: bool,
}

impl SseParser {
    /// A parser with nothing buffered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; get back whatever frames completed.
    ///
    /// # Errors
    ///
    /// [`ProviderError::BadRequest`] when a line is not UTF-8. That is a
    /// corrupt response rather than a transport hiccup, so it is terminal:
    /// retrying would decode the same bytes.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, ProviderError> {
        let mut out = Vec::new();
        for byte in bytes {
            if *byte == b'\n' {
                let line = std::mem::take(&mut self.pending);
                let line = String::from_utf8(line).map_err(|e| {
                    ProviderError::BadRequest(format!("server-sent event is not UTF-8: {e}"))
                })?;
                if let Some(ev) = self.line(line.strip_suffix('\r').unwrap_or(&line)) {
                    out.push(ev);
                }
            } else {
                self.pending.push(*byte);
            }
        }
        Ok(out)
    }

    /// Dispatch a frame the stream ended without blank-lining.
    ///
    /// Real servers terminate the last frame properly; a truncated one is
    /// exactly what a dropped connection produces, and losing the final
    /// `message_stop` would turn "the turn ended" into "the turn hung".
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned();
            if let Some(ev) = self.line(line.strip_suffix('\r').unwrap_or(&line)) {
                out.push(ev);
            }
        }
        if let Some(ev) = self.dispatch() {
            out.push(ev);
        }
        out
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        // A line beginning with a colon is a comment. Servers send them as
        // keep-alives; they are not frames and they are not errors.
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => {
                self.name = Some(value.to_owned());
                self.open = true;
            }
            "data" => {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.open = true;
            }
            // `id:` and `retry:` are the resumption protocol. Anthropic does
            // not use them and neither do we; ignoring them is the spec's own
            // answer for unknown fields too.
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if !self.open {
            return None;
        }
        self.open = false;
        Some(SseEvent {
            name: self.name.take().unwrap_or_else(|| "message".to_owned()),
            data: std::mem::take(&mut self.data),
        })
    }
}
