//! `SKILL.md`: YAML front matter and a Markdown body, and nothing clever.
//!
//! # Adopt, do not extend
//!
//! The agentskills.io shape is `---`, a YAML mapping, `---`, then Markdown. Two
//! keys are load-bearing — `name` and `description` — and **every other key is
//! kept and ignored**. Not skipped: kept, in [`SkillDoc::front`], so that
//! [`SkillDoc::to_markdown`] round-trips a file this crate has never seen the
//! schema of.
//!
//! That is deliberate. We do not own this format. A parser that rejects an
//! unknown key turns every upstream addition into a bug report here, and a
//! parser that drops one turns a round trip into data loss. `tests/parse.rs`
//! holds six published skills, byte for byte, and asserts both.
//!
//! There is **no Orrery-only key**. Scoping and the script grant are
//! configuration (open question 2, decided), so a `SKILL.md` written for Claude
//! Code or Codex is the same file here.

use std::path::{Path, PathBuf};

use serde_yaml_ng::{Mapping, Value};

use crate::error::SkillError;

/// The front-matter delimiter.
const FENCE: &str = "---";

/// One parsed `SKILL.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillDoc {
    /// The `name` key. Required.
    pub name: String,
    /// The `description` key, or empty when there is none.
    pub description: String,
    /// **Every** front-matter key, `name` and `description` included, exactly
    /// as it was written.
    pub front: Mapping,
    /// The Markdown after the closing fence, verbatim.
    pub body: String,
    /// Where it was read from.
    pub path: PathBuf,
}

impl SkillDoc {
    /// A front-matter value by key, whether or not this crate knows it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.front.get(&Value::from(key))
    }

    /// Render back to `SKILL.md`.
    ///
    /// Keys this crate does not know go back out, which is what makes
    /// "preserved" checkable rather than promised.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let front = serde_yaml_ng::to_string(&self.front)
            .unwrap_or_default()
            .trim_end()
            .to_owned();
        format!("{FENCE}\n{front}\n{FENCE}\n{}", self.body)
    }
}

/// Parse a `SKILL.md`.
///
/// `path` is only ever used to name the file in an error; nothing is read from
/// disk here, so a caller can parse text it already holds.
///
/// # Errors
///
/// [`SkillError::MissingFrontMatter`] when there is no `---` block,
/// [`SkillError::BadFrontMatter`] when it is not a YAML mapping, and
/// [`SkillError::MissingName`] when the mapping has no `name` — each naming the
/// file, because "a skill somewhere did not parse" is not an actionable
/// message.
pub fn skill_md(text: &str, path: impl AsRef<Path>) -> Result<SkillDoc, SkillError> {
    let path = path.as_ref();
    let (front_text, body) = split(text).ok_or_else(|| SkillError::MissingFrontMatter {
        path: path.to_path_buf(),
    })?;

    let value: Value =
        serde_yaml_ng::from_str(front_text).map_err(|e| SkillError::BadFrontMatter {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let front = match value {
        Value::Mapping(front) => front,
        // An empty front-matter block deserialises to null. Treat it as an
        // empty mapping so the error is the useful one (`no name`) rather than
        // the shape complaint.
        Value::Null => Mapping::new(),
        other => {
            return Err(SkillError::BadFrontMatter {
                path: path.to_path_buf(),
                message: format!("expected a mapping, found {}", kind_of(&other)),
            });
        }
    };

    let name = string(&front, "name").ok_or_else(|| SkillError::MissingName {
        path: path.to_path_buf(),
    })?;

    Ok(SkillDoc {
        name,
        description: string(&front, "description").unwrap_or_default(),
        front,
        body: body.to_owned(),
        path: path.to_path_buf(),
    })
}

/// Read and parse a `SKILL.md` from disk.
///
/// # Errors
///
/// [`SkillError::Io`] when the file cannot be read, then whatever
/// [`skill_md`] raises.
pub fn skill_file(path: impl AsRef<Path>) -> Result<SkillDoc, SkillError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| SkillError::io(path, &e))?;
    skill_md(&text, path)
}

/// Split a document into its front matter and its body.
///
/// The opening fence must be the first non-empty line — a `---` halfway down a
/// document is a horizontal rule, not front matter — and a byte-order mark is
/// tolerated because editors on Windows write them.
fn split(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))?;

    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == FENCE {
            let front = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((front, body));
        }
        offset += line.len();
    }
    // A final fence with no trailing newline.
    if rest[offset..].trim_end() == FENCE {
        return Some((&rest[..offset], ""));
    }
    None
}

/// A string-valued key. YAML scalars that are not strings are rendered rather
/// than refused: a `name: 2029` is a name somebody wrote, not a parse error.
fn string(front: &Mapping, key: &str) -> Option<String> {
    match front.get(&Value::from(key))? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Sequence(_) => "a sequence",
        Value::Mapping(_) => "a mapping",
        Value::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests {
    use super::{skill_md, split};
    use std::path::Path;

    #[test]
    fn a_rule_further_down_is_not_front_matter() {
        assert!(split("# title\n\n---\n\nmore\n").is_none());
    }

    #[test]
    fn crlf_front_matter_still_splits() {
        let doc = skill_md("---\r\nname: x\r\n---\r\nbody\r\n", Path::new("x")).unwrap();
        assert_eq!(doc.name, "x");
        assert_eq!(doc.body, "body\r\n");
    }

    #[test]
    fn a_body_that_is_only_front_matter_is_fine() {
        let doc = skill_md("---\nname: x\n---", Path::new("x")).unwrap();
        assert_eq!(doc.body, "");
    }
}
