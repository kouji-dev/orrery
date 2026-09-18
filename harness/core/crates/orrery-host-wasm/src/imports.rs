//! The five broker imports, and the arena on the way back.
//!
//! # Read the signatures
//!
//! Not one of them carries a capability token, and not one of them can: the
//! parameters are exactly what the `.wit` says, and the `.wit` has no such
//! type. The host looks the call's authority up on its own side, through
//! [`HostState::broker`](crate::HostState) — the guest asks, the host decides.
//!
//! # Read the return types
//!
//! Every one is `Result<T, Error>` where `Error` is the WIT variant, **not**
//! `wasmtime::Result`. That is deliberate and it is the whole of task 3: a
//! denial cannot trap here, because there is no path through these functions
//! that produces a trap. The guest receives a value it can branch on.

use wasmtime::component::Linker;

use crate::bindings::orrery::extension::broker::{
    CredUsage, Error, FetchOpts, FetchOut, FileOut, Host, ProcOut, RunOpts,
};
use crate::bindings::orrery::extension::surfaces as wit_surfaces;
use crate::store::HostState;

/// Put the broker on the linker.
///
/// # Errors
///
/// When an import is already defined under that name.
pub fn add_to_linker(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    crate::bindings::orrery::extension::broker::add_to_linker::<HostState, HostState>(
        linker,
        |state| state,
    )
}

impl wasmtime::component::HasData for HostState {
    type Data<'a> = &'a mut HostState;
}

impl Host for HostState {
    async fn run_proc(
        &mut self,
        cmd: String,
        args: Vec<String>,
        opts: RunOpts,
    ) -> Result<ProcOut, Error> {
        // Cancellation is checked before the call as well as inside the broker:
        // a guest that asks for one more process after the turn was cancelled
        // gets a value back and stops, rather than starting work nobody wants.
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.broker
            .run_proc(&cmd, &args, opts)
            .await
            .map_err(Into::into)
    }

    async fn read_file(&mut self, path: String, max_bytes: u64) -> Result<FileOut, Error> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.broker
            .read_file(&path, max_bytes)
            .await
            .map_err(Into::into)
    }

    async fn write_file(
        &mut self,
        path: String,
        bytes: Vec<u8>,
        atomic: bool,
    ) -> Result<(), Error> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.broker
            .write_file(&path, &bytes, atomic)
            .await
            .map_err(Into::into)
    }

    async fn fetch(&mut self, url: String, opts: FetchOpts) -> Result<FetchOut, Error> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.broker.fetch(&url, opts).await.map_err(Into::into)
    }

    async fn use_credential(&mut self, name: String, usage: CredUsage) -> Result<(), Error> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.broker
            .use_credential(&name, usage)
            .await
            .map_err(Into::into)
    }
}

// --- the arena, across the boundary --------------------------------------

/// The generated WIT arena as the one `orrery-wit` rebuilds from.
///
/// Two structurally identical types, kept apart on purpose: `orrery-wit` must
/// not depend on `wasmtime`, so the generated bindings cannot *be* its types.
/// This function is the seam, and `tests/roundtrip.rs` is what keeps the two
/// from drifting.
#[must_use]
pub fn from_wit(arena: wit_surfaces::Surface) -> orrery_wit::arena::SurfaceArena {
    orrery_wit::arena::SurfaceArena {
        nodes: arena
            .nodes
            .into_iter()
            .map(|n| orrery_wit::arena::SurfaceNode {
                kind: kind_from_wit(n.kind),
                id: n.id,
                status: n.status.map(status_from_wit),
                payload: n.payload,
                children: n.children,
            })
            .collect(),
        root: arena.root,
    }
}

/// The other direction, for a host that wants to hand a guest a surface.
#[must_use]
pub fn to_wit(arena: orrery_wit::arena::SurfaceArena) -> wit_surfaces::Surface {
    wit_surfaces::Surface {
        nodes: arena
            .nodes
            .into_iter()
            .map(|n| wit_surfaces::SurfaceNode {
                kind: kind_to_wit(n.kind),
                id: n.id,
                status: n.status.map(status_to_wit),
                payload: n.payload,
                children: n.children,
            })
            .collect(),
        root: arena.root,
    }
}

fn kind_from_wit(k: wit_surfaces::NodeKind) -> orrery_wit::arena::NodeKind {
    use orrery_wit::arena::NodeKind as A;
    use wit_surfaces::NodeKind as W;
    match k {
        W::Text => A::Text,
        W::Table => A::Table,
        W::Tree => A::Tree,
        W::Diff => A::Diff,
        W::Progress => A::Progress,
        W::Stream => A::Stream,
        W::Task => A::Task,
        W::Question => A::Question,
        W::Form => A::Form,
        W::Stack => A::Stack,
        W::Markdown => A::Markdown,
        W::Custom => A::Custom,
    }
}

fn kind_to_wit(k: orrery_wit::arena::NodeKind) -> wit_surfaces::NodeKind {
    use orrery_wit::arena::NodeKind as A;
    use wit_surfaces::NodeKind as W;
    match k {
        A::Text => W::Text,
        A::Table => W::Table,
        A::Tree => W::Tree,
        A::Diff => W::Diff,
        A::Progress => W::Progress,
        A::Stream => W::Stream,
        A::Task => W::Task,
        A::Question => W::Question,
        A::Form => W::Form,
        A::Stack => W::Stack,
        A::Markdown => W::Markdown,
        A::Custom => W::Custom,
    }
}

fn status_from_wit(s: wit_surfaces::NodeStatus) -> orrery_proto::surface::Status {
    use orrery_proto::surface::Status as P;
    use wit_surfaces::NodeStatus as W;
    match s {
        W::Pending => P::Pending,
        W::Running => P::Running,
        W::Done => P::Done,
        W::Failed => P::Failed,
        W::Cancelled => P::Cancelled,
    }
}

fn status_to_wit(s: orrery_proto::surface::Status) -> wit_surfaces::NodeStatus {
    use orrery_proto::surface::Status as P;
    use wit_surfaces::NodeStatus as W;
    match s {
        P::Pending => W::Pending,
        P::Running => W::Running,
        P::Done => W::Done,
        P::Failed => W::Failed,
        P::Cancelled => W::Cancelled,
        // `Status` is `#[non_exhaustive]`; a new one this seam has not been
        // taught is reported as "finished, unsuccessfully" rather than guessed.
        _ => W::Failed,
    }
}
