//! SKILL.md front matter, the discovery pass, and running a skill's scripts under a grant.
//!
//! # Adopt the format; add exactly one thing
//!
//! [`parse`] takes the agentskills.io `SKILL.md` shape **unchanged**, so a skill
//! that works in Claude Code, Cursor or Codex works here with no edit: unknown
//! front-matter keys are preserved and ignored, and there is no Orrery-only key
//! to add. [`discover`] finds skills in the same single pass that finds
//! extensions, prompts and MCP servers.
//!
//! The one addition is [`scripts`]. Elsewhere a skill's bundled `scripts/` run
//! with the user's full privileges, which makes a skill a **better attack
//! vector than an extension because it looks like documentation**. Here they
//! run under a declared grant — brokered, audited, budgeted — the same path a
//! tool takes, and a skill with no declared grant cannot run a script at all.
//!
//! Implementation plan: `harness/docs/plans/13-skills-mcp.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod discover;
pub mod error;
pub mod parse;
pub mod scope;
pub mod scripts;

pub use discover::{LayerRoot, SkillSet, discover};
pub use error::SkillError;
pub use parse::SkillDoc;
pub use scope::{SkillRef, SkillSettings, SkillSource};
pub use scripts::{EffectOutcome, EffectRecord, ScriptRun, ScriptRunner};
