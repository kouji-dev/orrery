//! Finding skills — in the **one** discovery pass, not a second walk of the same tree.
//!
//! Plan 10 task 4 established that extensions, skills, prompts and MCP servers
//! are found in a single instrumented pass, because four walks over one tree can
//! disagree and §4.11 says that what the model can see is what the manifest
//! says. So this module does not walk anything: it asks
//! [`orrery_config::discover`] for the skills it already found and turns each
//! one into a [`SkillRef`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use orrery_config::discover::{Discovered, FsWalk, ItemKind, Walk};
use orrery_config::provenance::Provenanced;

use crate::error::SkillError;
use crate::parse;
use crate::scope::{SkillRef, SkillSettings};

pub use orrery_config::discover::LayerRoot;

/// What discovery found, in layer order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillSet {
    skills: Vec<SkillRef>,
}

impl SkillSet {
    /// Build a set from already-resolved references.
    #[must_use]
    pub fn new(skills: Vec<SkillRef>) -> Self {
        Self { skills }
    }

    /// Everything found, whoever it is for.
    #[must_use]
    pub fn all(&self) -> &[SkillRef] {
        &self.skills
    }

    /// How many there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether nothing was found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// One by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SkillRef> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// The skills this agent may load.
    ///
    /// This is the list that goes into the agent's context. A skill scoped
    /// elsewhere is **not in it** — not listed and then refused, simply absent.
    #[must_use]
    pub fn for_agent(&self, agent: &str) -> Vec<&SkillRef> {
        self.skills.iter().filter(|s| s.visible_to(agent)).collect()
    }
}

/// What configuration says about skills, by skill name.
pub type Settings = BTreeMap<String, SkillSettings>;

/// What discovery produced, and what it could not read.
///
/// A skill that does not parse is **not** fatal: it is one skill missing from
/// one agent's context, and the run carries on with the rest. The problems come
/// back alongside so the load ledger can say what happened.
#[derive(Debug)]
pub struct Found {
    /// The skills that loaded.
    pub skills: SkillSet,
    /// The ones that did not, and why.
    pub problems: Vec<SkillError>,
}

/// Turn the discovery manifest's skills into [`SkillRef`]s.
///
/// `roots` are the layer directories in force; `settings` is what configuration
/// says about each skill by name. Skills are returned in the order discovery
/// found them, which is layer order.
#[must_use]
pub fn discover(roots: &[LayerRoot], settings: &Settings) -> Found {
    discover_with(roots, settings, &FsWalk)
}

/// [`discover`] over a caller-supplied walker, so a test can count reads.
#[must_use]
pub fn discover_with(roots: &[LayerRoot], settings: &Settings, walk: &dyn Walk) -> Found {
    let manifest = orrery_config::discover::discover(roots, &Provenanced::new(), walk);
    let mut skills = Vec::new();
    let mut problems = Vec::new();

    for found in manifest.of(ItemKind::Skill) {
        match load_one(found, settings) {
            Ok(skill) => skills.push(skill),
            Err(e) => {
                tracing::warn!(
                    target: "orrery.load.skills",
                    skill = %found.name,
                    error = %e,
                    "skill not loaded"
                );
                problems.push(e);
            }
        }
    }

    Found {
        skills: SkillSet::new(skills),
        problems,
    }
}

/// The `SKILL.md` a discovered entry points at.
///
/// Discovery reports a directory (`skills/review/`) or a bare file
/// (`skills/review.md`); a skill is a directory with a `SKILL.md` in it, and the
/// flat form is accepted because people write it.
fn skill_md_of(found: &Discovered) -> PathBuf {
    if found.source.is_dir() {
        found.source.join("SKILL.md")
    } else {
        found.source.clone()
    }
}

fn load_one(found: &Discovered, settings: &Settings) -> Result<SkillRef, SkillError> {
    let path = skill_md_of(found);
    let doc = parse::skill_file(&path)?;
    let settings = settings.get(&doc.name).cloned().unwrap_or_default();
    Ok(SkillRef::new(&doc, found.layer, &settings))
}
