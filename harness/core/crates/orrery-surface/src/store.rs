//! Per-turn surface storage, and the seal at `turn.settled`.
//!
//! # Why sealing is here and not in the client
//!
//! A client that has closed a turn has nowhere to put a patch for it. If the
//! kernel sent one anyway, every renderer would need its own rule for what to
//! do with it, and the five would not agree. So the refusal happens once, at
//! the store, and goes back **to the extension** that tried — which is the one
//! place it can be fixed.
//!
//! An extension with something more to say after its turn ended is not stuck:
//! it emits a *new* surface in the turn that is current now. Sealing stops a
//! late edit to a closed transcript, not late output.

use indexmap::IndexMap;
use orrery_proto::{Surface, SurfaceId, SurfacePatch, TurnId};

use crate::hash::{HashTree, hash_tree};
use crate::validate::{Warning, validate};
use crate::{SurfaceError, diff::diff_with_hashes};

/// What one turn holds.
#[derive(Debug, Default)]
struct TurnSurfaces {
    sealed: bool,
    surfaces: IndexMap<SurfaceId, (Surface, HashTree)>,
}

/// The surfaces of every turn in a session, and which of them are closed.
#[derive(Debug, Default)]
pub struct SurfaceStore {
    turns: IndexMap<TurnId, TurnSurfaces>,
    warnings: Vec<Warning>,
}

impl SurfaceStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take an extension's whole surface and answer with what changed.
    ///
    /// The extension re-emits everything; this is where that becomes a small
    /// wire message. A surface the store has not seen produces one
    /// [`SurfacePatch::Replace`], which is also what creates it on a client.
    ///
    /// # Errors
    ///
    /// - [`SurfaceError::Sealed`] when the turn has settled. **No patch is
    ///   produced**: the refusal goes back to the caller and no frame is sent.
    /// - [`SurfaceError::Malformed`] / [`SurfaceError::TooDeep`] when the
    ///   surface is one no renderer could draw.
    pub fn emit(
        &mut self,
        turn: TurnId,
        id: SurfaceId,
        next: Surface,
    ) -> Result<Vec<SurfacePatch>, SurfaceError> {
        let warnings = validate(&next)?;
        for warning in &warnings {
            tracing::warn!(
                target: "orrery.surface.validate",
                surface = %id,
                path = %warning.path,
                "{}", warning.message
            );
        }
        self.warnings.extend(warnings);

        let entry = self.turns.entry(turn).or_default();
        if entry.sealed {
            return Err(SurfaceError::Sealed { turn, surface: id });
        }

        let next_hash = hash_tree(&next);
        let mut out = Vec::new();
        match entry.surfaces.get(&id) {
            None => out.push(SurfacePatch::Replace {
                id,
                value: next.clone(),
            }),
            Some((prev, prev_hash)) => {
                diff_with_hashes(prev, prev_hash, &next, &next_hash, id, &mut out);
            }
        }
        entry.surfaces.insert(id, (next, next_hash));
        Ok(out)
    }

    /// Take a surface away.
    ///
    /// # Errors
    ///
    /// [`SurfaceError::Sealed`] when the turn has settled.
    pub fn remove(
        &mut self,
        turn: TurnId,
        id: SurfaceId,
    ) -> Result<Vec<SurfacePatch>, SurfaceError> {
        let entry = self.turns.entry(turn).or_default();
        if entry.sealed {
            return Err(SurfaceError::Sealed { turn, surface: id });
        }
        match entry.surfaces.shift_remove(&id) {
            Some(_) => Ok(vec![SurfacePatch::Remove { id }]),
            None => Ok(Vec::new()),
        }
    }

    /// Close a turn. Everything it holds is now history.
    pub fn seal(&mut self, turn: TurnId) {
        self.turns.entry(turn).or_default().sealed = true;
    }

    /// Whether this turn is closed.
    #[must_use]
    pub fn is_sealed(&self, turn: &TurnId) -> bool {
        self.turns.get(turn).is_some_and(|t| t.sealed)
    }

    /// One turn's surface, as the store last saw it.
    #[must_use]
    pub fn get(&self, turn: &TurnId, id: &SurfaceId) -> Option<&Surface> {
        self.turns.get(turn)?.surfaces.get(id).map(|(s, _)| s)
    }

    /// One turn's surfaces, in the order they were first emitted.
    #[must_use]
    pub fn surfaces(&self, turn: &TurnId) -> Vec<(SurfaceId, &Surface)> {
        self.turns
            .get(turn)
            .map(|t| t.surfaces.iter().map(|(id, (s, _))| (*id, s)).collect())
            .unwrap_or_default()
    }

    /// Everything validation nudged about, in emission order.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}
