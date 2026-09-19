//! Discovered skills, rendered into the system prompt of the same turn.
//!
//! # Why this is here and not in a command
//!
//! [`KernelConfig::skills`](orrery_kernel::KernelConfig) has been a
//! `Vec<String>` the kernel renders as section 4 of the system prompt since
//! plan 05, and **nothing filled it**. `orrery skills list` read `SKILL.md`
//! files off the layer roots and printed them; a turn never saw one. A skill
//! the model is not given is a file on disk, not a skill — the same shape of
//! defect as the inert `[permissions]`, one subsystem further along.
//!
//! # What the model is given
//!
//! The name, the description and the body, verbatim: the `SKILL.md` format is
//! adopted unchanged (plan 13), so nothing here rewrites what an author wrote.
//! A skill whose file has since been deleted or will not parse is left out and
//! said out loud in the log; one bad file is one skill missing, never a failed
//! session.

use orrery_skills::SkillRef;

/// Render each skill as the block the system prompt carries.
#[must_use]
pub fn render(skills: &[SkillRef]) -> Vec<String> {
    let mut out = Vec::with_capacity(skills.len());
    for skill in skills {
        match orrery_skills::parse::skill_file(&skill.path) {
            Ok(doc) => {
                tracing::debug!(
                    target: "orrery.harness.build",
                    skill = %doc.name,
                    layer = ?skill.layer,
                    path = %skill.path.display(),
                    "a discovered skill goes into this session's system prompt"
                );
                let mut block = format!("## skill: {}\n{}\n", doc.name, doc.description);
                if !doc.body.trim().is_empty() {
                    block.push('\n');
                    block.push_str(doc.body.trim_end());
                    block.push('\n');
                }
                out.push(block);
            }
            Err(e) => tracing::warn!(
                target: "orrery.harness.build",
                skill = %skill.name,
                path = %skill.path.display(),
                error = %e,
                "a discovered skill could not be read, so this turn does not carry it"
            ),
        }
    }
    out
}
