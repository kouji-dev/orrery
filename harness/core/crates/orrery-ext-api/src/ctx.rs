//! What an extension gets when it is called, and the ceiling it runs under.

use std::sync::Arc;

use orrery_proto::{CallId, Cell, ExtId, Surface, SurfaceKind, TextStyle};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::broker::{BrokerFacade, DeniesEverything};

/// What one tool call is allowed to consume.
///
/// # Why this is not `orrery_tools::ToolBudget`
///
/// It is the same three numbers, and it is deliberately a second type: this
/// crate is **published** and `orrery-tools` is not, so the extension-facing
/// ceiling cannot be the registry's own. `orrery-host` converts at the boundary
/// — one function, `orrery_host::host::ceiling_of`, and both halves are asserted
/// to agree. When the registry's crate publishes, this type should be deleted
/// in favour of it and the conversion should go with it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolBudget {
    /// How long, in milliseconds of wall clock.
    pub wall_clock_ms: u64,
    /// How many bytes the tool may emit.
    pub output_bytes: u64,
    /// How much memory a spawned process may take. `None` for in-process tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

impl ToolBudget {
    /// A ceiling on time and output, with no memory limit.
    #[must_use]
    pub const fn new(wall_clock_ms: u64, output_bytes: u64) -> Self {
        Self {
            wall_clock_ms,
            output_bytes,
            memory_bytes: None,
        }
    }

    /// Also cap the memory of a spawned process.
    #[must_use]
    pub const fn with_memory(mut self, memory_bytes: u64) -> Self {
        self.memory_bytes = Some(memory_bytes);
        self
    }

    /// The tighter of two ceilings, field by field. Narrowing never loosens.
    #[must_use]
    pub fn narrow(self, other: Self) -> Self {
        Self {
            wall_clock_ms: self.wall_clock_ms.min(other.wall_clock_ms),
            output_bytes: self.output_bytes.min(other.output_bytes),
            memory_bytes: match (self.memory_bytes, other.memory_bytes) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, b) => b,
            },
        }
    }
}

impl Default for ToolBudget {
    /// Thirty seconds and a megabyte: enough for a test, small enough that
    /// forgetting to set one is visible rather than unlimited.
    fn default() -> Self {
        Self::new(30_000, 1 << 20)
    }
}

/// Somewhere a described surface goes.
pub trait SurfaceEmit: Send + Sync {
    /// Take one surface.
    fn emit(&self, surface: &Surface);
}

/// Every surface a sink was handed, in order.
#[derive(Clone, Debug, Default)]
pub struct SurfaceLog(Arc<Mutex<Vec<Surface>>>);

impl SurfaceLog {
    /// Everything emitted so far.
    #[must_use]
    pub fn all(&self) -> Vec<Surface> {
        self.0.lock().clone()
    }

    /// How many surfaces have been emitted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.lock().len()
    }

    /// Whether nothing has been emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.lock().is_empty()
    }
}

impl SurfaceEmit for SurfaceLog {
    fn emit(&self, surface: &Surface) {
        self.0.lock().push(surface.clone());
    }
}

/// `ctx.ui` — describes, never draws.
///
/// `ctx.ui.table(..)` produces a [`Surface`]: ratatui renders a widget, Ink
/// renders a component, `--json` emits the payload, all from one description.
/// An extension that wanted to draw would have to know which client it was
/// talking to, and then there would be extensions that only work in one.
#[derive(Clone)]
pub struct SurfaceSink {
    sink: Option<Arc<dyn SurfaceEmit>>,
}

impl std::fmt::Debug for SurfaceSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurfaceSink")
            .field("attached", &self.sink.is_some())
            .finish()
    }
}

impl Default for SurfaceSink {
    fn default() -> Self {
        Self::discarding()
    }
}

impl SurfaceSink {
    /// A sink that describes surfaces and throws them away. Still returns them,
    /// so a tool's `Outcome` carries the surface either way.
    #[must_use]
    pub fn discarding() -> Self {
        Self { sink: None }
    }

    /// A sink that keeps everything, for a test.
    #[must_use]
    pub fn recording() -> (Self, SurfaceLog) {
        let log = SurfaceLog::default();
        (Self::to(Arc::new(log.clone())), log)
    }

    /// A sink over somewhere real.
    #[must_use]
    pub fn to(sink: Arc<dyn SurfaceEmit>) -> Self {
        Self { sink: Some(sink) }
    }

    /// Emit a surface already built, and hand it back.
    pub fn emit(&self, surface: Surface) -> Surface {
        if let Some(sink) = &self.sink {
            sink.emit(&surface);
        }
        surface
    }

    /// Rows under headers.
    pub fn table<C, R, S>(&self, columns: C, rows: R) -> Surface
    where
        C: IntoIterator,
        C::Item: Into<String>,
        R: IntoIterator<Item = S>,
        S: IntoIterator,
        <S as IntoIterator>::Item: Into<String>,
    {
        let kind = SurfaceKind::Table {
            columns: columns.into_iter().map(Into::into).collect(),
            rows: rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|c| Cell {
                            text: c.into(),
                            style: None,
                        })
                        .collect()
                })
                .collect(),
        };
        self.emit(Surface::new(kind))
    }

    /// A run of text.
    pub fn text(&self, value: impl Into<String>) -> Surface {
        self.emit(Surface::new(SurfaceKind::Text {
            value: value.into(),
            style: None,
        }))
    }

    /// A run of text that means something in particular.
    pub fn styled(&self, value: impl Into<String>, style: TextStyle) -> Surface {
        self.emit(Surface::new(SurfaceKind::Text {
            value: value.into(),
            style: Some(style),
        }))
    }

    /// Markdown. `complete` is false while it is still streaming.
    pub fn markdown(&self, value: impl Into<String>, complete: bool) -> Surface {
        self.emit(Surface::new(SurfaceKind::Markdown {
            value: value.into(),
            complete,
        }))
    }
}

/// Everything a tool call gets that is not its input.
///
/// Nothing here is a raw handle. The broker is the only door out, the cancel
/// token is the only way a call learns it should stop, and the sink describes
/// rather than draws.
#[derive(Clone, Debug)]
pub struct CallCtx {
    /// Which call, so the audit can be joined to it.
    pub call: CallId,
    /// Which extension is being called.
    pub ext: ExtId,
    /// Which of its tools.
    pub tool: String,
    /// The ceiling this call runs under.
    pub budget: ToolBudget,
    /// Fired when this one call should stop. `turn.cancel` reaches the call,
    /// not the connection.
    pub cancel: CancellationToken,
    /// Files, processes, the network, credentials — each policy-checked.
    pub broker: Arc<dyn BrokerFacade>,
    /// `ctx.ui.table(..)`.
    pub ui: SurfaceSink,
}

impl std::fmt::Debug for dyn BrokerFacade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrokerFacade")
    }
}

impl CallCtx {
    /// A context for one call.
    #[must_use]
    pub fn new(
        call: CallId,
        ext: ExtId,
        tool: impl Into<String>,
        budget: ToolBudget,
        cancel: CancellationToken,
        broker: Arc<dyn BrokerFacade>,
    ) -> Self {
        Self {
            call,
            ext,
            tool: tool.into(),
            budget,
            cancel,
            broker,
            ui: SurfaceSink::discarding(),
        }
    }

    /// A context that denies everything and records nothing: for a unit test of
    /// a tool that does not touch the outside world.
    #[must_use]
    pub fn inert(ext: ExtId, tool: impl Into<String>) -> Self {
        Self::new(
            CallId::new(),
            ext,
            tool,
            ToolBudget::default(),
            CancellationToken::new(),
            Arc::new(DeniesEverything),
        )
    }

    /// Attach a surface sink.
    #[must_use]
    pub fn with_ui(mut self, ui: SurfaceSink) -> Self {
        self.ui = ui;
        self
    }

    /// Whether this call has been told to stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}
