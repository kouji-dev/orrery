//! `AguiSession`: one connection, whichever transport it is over.
//!
//! The same six calls whether the kernel is in this process, behind a pipe or
//! behind HTTP — which is what "there is no privileged client" has to mean in
//! code rather than in a paragraph.

use std::sync::Arc;

use orrery_agui::Frame;
use orrery_proto::{
    ConsentAnswerKind, PromptId, ReqId, Request, Seq, SessionId, SurfaceId, TurnId, UserInput,
};
use orrery_transport::listener::pipe::{self, PipeClient};
use orrery_transport::listener::{ControlHandler, http as http_listener};
use orrery_transport::{ClientStream, Hub};

use crate::endpoint::Endpoint;
use crate::error::ClientError;

/// The bits of a connection that differ by transport.
enum Wire {
    /// The kernel in this process: frames as themselves, control as a call.
    Inproc {
        stream: ClientStream,
        control: Arc<dyn ControlHandler>,
        pending: Vec<Frame>,
    },
    /// A named pipe or UDS.
    Pipe {
        client: Box<PipeClient<interprocess::local_socket::tokio::Stream>>,
    },
    /// HTTP + SSE.
    Http {
        client: reqwest::Client,
        url: String,
        token: Option<String>,
        events: Option<SseStream>,
    },
}

/// A connected session.
pub struct AguiSession {
    wire: Wire,
    session: Option<SessionId>,
    last_seq: Option<u64>,
}

impl AguiSession {
    /// Connect to an endpoint.
    ///
    /// `inproc:` cannot be reached this way — there is nothing to look up, so
    /// the caller hands over the hub with [`AguiSession::in_process`].
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the endpoint is unreachable or the handshake fails.
    pub async fn connect(endpoint: &Endpoint) -> Result<Self, ClientError> {
        match endpoint {
            Endpoint::Inproc => Err(ClientError::BadEndpoint(
                "inproc: needs a hub; use AguiSession::in_process".into(),
            )),
            Endpoint::Pipe { name } => {
                let client = pipe::connect(name, orrery_transport::frame::Hello::default()).await?;
                Ok(Self {
                    wire: Wire::Pipe {
                        client: Box::new(client),
                    },
                    session: None,
                    last_seq: None,
                })
            }
            Endpoint::Http { url, token } => Ok(Self {
                wire: Wire::Http {
                    client: reqwest::Client::new(),
                    url: url.clone(),
                    token: token.clone(),
                    events: None,
                },
                session: None,
                last_seq: None,
            }),
        }
    }

    /// Attach to the kernel in this process.
    ///
    /// The frames are [`Frame`]s, not bytes: the in-process transport does not
    /// serialise, and this is the constructor that proves it — no codec, no
    /// endpoint, no URL.
    #[must_use]
    pub fn in_process(
        hub: &Hub,
        control: Arc<dyn ControlHandler>,
        tick: std::time::Duration,
    ) -> Self {
        Self {
            wire: Wire::Inproc {
                stream: hub.subscribe(tick),
                control,
                pending: Vec::new(),
            },
            session: None,
            last_seq: None,
        }
    }

    /// The last `seq` this session has seen.
    #[must_use]
    pub fn last_seq(&self) -> Option<u64> {
        self.last_seq
    }

    /// Attach to a session, optionally replaying from where this client left off.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the kernel refuses.
    pub async fn attach(
        &mut self,
        session: SessionId,
        since: Option<Seq>,
    ) -> Result<(), ClientError> {
        self.session = Some(session);
        self.control(Request::SessionAttach {
            id: ReqId::new(),
            session,
            since,
        })
        .await
        .map(|_| ())
    }

    /// Submit a turn.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the kernel refuses, or does not name the turn it
    /// started.
    pub async fn submit(&mut self, input: UserInput) -> Result<TurnId, ClientError> {
        let session = self.session()?;
        let answer = self
            .control(Request::TurnSubmit {
                id: ReqId::new(),
                session,
                input,
            })
            .await?;
        answer
            .get("turn")
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| ClientError::Refused(format!("no turn id in {answer}")))
    }

    /// Cancel a turn in flight.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the kernel refuses.
    pub async fn cancel(&mut self, turn: TurnId) -> Result<(), ClientError> {
        let session = self.session()?;
        self.control(Request::TurnCancel {
            id: ReqId::new(),
            session,
            turn,
        })
        .await
        .map(|_| ())
    }

    /// Answer a consent prompt.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the kernel refuses — including when the deadline
    /// had already passed, which is not a failure of the client.
    pub async fn answer(
        &mut self,
        prompt: PromptId,
        answer: ConsentAnswerKind,
    ) -> Result<(), ClientError> {
        self.control(Request::ConsentAnswer {
            id: ReqId::new(),
            prompt,
            answer,
        })
        .await
        .map(|_| ())
    }

    /// Send what a surface produced.
    ///
    /// This is the whole of a client's write path. AG-UI's shared state is
    /// bidirectional by design and ours is not: a client edit arrives here, as
    /// an intent the kernel validates, never as a state mutation.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the kernel refuses.
    pub async fn intent(
        &mut self,
        surface: SurfaceId,
        value: serde_json::Value,
    ) -> Result<(), ClientError> {
        let session = self.session()?;
        self.control(Request::Intent {
            id: ReqId::new(),
            session,
            surface,
            value,
        })
        .await
        .map(|_| ())
    }

    /// The next frame, or `None` when the session ends.
    pub async fn next_frame(&mut self) -> Option<Result<Frame, ClientError>> {
        let frame = match &mut self.wire {
            Wire::Inproc {
                stream, pending, ..
            } => {
                if pending.is_empty() {
                    pending.extend(stream.next_batch().await?);
                    pending.reverse();
                }
                pending.pop()?
            }
            Wire::Pipe { client } => client.next_frame().await?,
            Wire::Http { events, .. } => match events.as_mut()?.next().await? {
                Ok(frame) => frame,
                Err(err) => return Some(Err(err)),
            },
        };
        let gap = self
            .last_seq
            .filter(|prev| frame.first_seq() != prev + 1)
            .map(|prev| ClientError::Gap {
                expected: prev + 1,
                got: frame.first_seq(),
            });
        self.last_seq = Some(frame.seq);
        Some(gap.map_or(Ok(frame), Err))
    }

    /// Every frame, as a stream.
    pub fn events(&mut self) -> impl futures_util::Stream<Item = Result<Frame, ClientError>> + '_ {
        futures_util::stream::unfold(self, |session| async move {
            session.next_frame().await.map(|item| (item, session))
        })
    }

    fn session(&self) -> Result<SessionId, ClientError> {
        self.session
            .ok_or_else(|| ClientError::Refused("not attached to a session".into()))
    }

    async fn control(&mut self, request: Request) -> Result<serde_json::Value, ClientError> {
        match &mut self.wire {
            Wire::Inproc { control, .. } => Ok(control.control(request)?),
            Wire::Pipe { client } => {
                client.send(&request).await?;
                // The byte-stream transports are fire-and-forget for control:
                // the answer arrives as events, which is what a client draws
                // from anyway.
                Ok(serde_json::Value::Null)
            }
            Wire::Http {
                client, url, token, ..
            } => {
                let mut post = client.post(format!("{url}/control")).json(&request);
                if let Some(token) = token {
                    post = post.bearer_auth(token);
                }
                let response = post
                    .send()
                    .await
                    .map_err(|e| ClientError::Http(e.to_string()))?;
                if !response.status().is_success() {
                    return Err(ClientError::Refused(format!("{}", response.status())));
                }
                response
                    .json()
                    .await
                    .map_err(|e| ClientError::Http(e.to_string()))
            }
        }
    }

    /// Start a run over HTTP and take its SSE stream.
    ///
    /// AG-UI's own entry point, so an off-the-shelf client's `POST /run` and
    /// ours are the same request.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the run cannot be started.
    pub async fn run(&mut self, input: &UserInput) -> Result<(), ClientError> {
        let session = self.session()?;
        let Wire::Http {
            client,
            url,
            token,
            events,
        } = &mut self.wire
        else {
            return Err(ClientError::Refused(
                "`run` is HTTP's entry point; other transports submit and subscribe".into(),
            ));
        };
        let body = http_listener::RunAgentInput {
            thread_id: Some(session.to_string()),
            messages: vec![http_listener::RunMessage {
                role: "user".into(),
                content: Some(input.text.clone()),
                id: None,
            }],
            ..http_listener::RunAgentInput::default()
        };
        let mut post = client.post(format!("{url}/run")).json(&body);
        if let Some(token) = token {
            post = post.bearer_auth(token);
        }
        let response = post
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ClientError::Refused(format!("{}", response.status())));
        }
        *events = Some(SseStream::new(response));
        Ok(())
    }
}

/// An SSE body, parsed one `data:` line at a time.
struct SseStream {
    body: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Sync>,
    >,
    buffer: String,
    ready: std::collections::VecDeque<Frame>,
}

impl SseStream {
    fn new(response: reqwest::Response) -> Self {
        Self {
            body: Box::pin(response.bytes_stream()),
            buffer: String::new(),
            ready: std::collections::VecDeque::new(),
        }
    }

    async fn next(&mut self) -> Option<Result<Frame, ClientError>> {
        use futures_util::StreamExt;
        loop {
            if let Some(frame) = self.ready.pop_front() {
                return Some(Ok(frame));
            }
            let chunk = match self.body.next().await? {
                Ok(chunk) => chunk,
                Err(e) => return Some(Err(ClientError::Http(e.to_string()))),
            };
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(at) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..at + 2).collect();
                for line in block.lines() {
                    if let Some(data) = line.strip_prefix("data: ")
                        && let Ok(frame) = serde_json::from_str::<Frame>(data)
                    {
                        self.ready.push_back(frame);
                    }
                }
            }
        }
    }
}
