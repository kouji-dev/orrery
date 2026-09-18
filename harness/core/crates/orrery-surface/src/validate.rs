//! What can be checked without knowing the client.
//!
//! Two kinds of answer, deliberately:
//!
//! - an **error** is a surface no renderer could draw — an un-namespaced
//!   `custom.kind`, a tree deeper than anything will lay out. It never reaches a
//!   client.
//! - a **warning** is a surface that will draw and probably should not — a
//!   custom fallback that says "open the web UI". We cannot judge content, so we
//!   nudge at load time rather than refuse.

use orrery_proto::{Surface, SurfaceKind};

use crate::SurfaceError;

/// How deep a surface may nest.
///
/// Sixty-four is far past anything a person reads and far short of what blows a
/// recursive renderer's stack. The number exists so that the refusal happens
/// here, once, instead of once per client.
pub const MAX_DEPTH: usize = 64;

/// A fallback shorter than this is almost certainly not describing anything.
pub const MIN_FALLBACK_CHARS: usize = 16;

/// Phrases that mean "this fallback gave up".
///
/// Crude on purpose — see the plan's open question 3. A phrase list cannot
/// judge content; it can catch the four sentences everybody writes when they
/// are not trying.
const LAZY_PHRASES: &[&str] = &[
    "open the",
    "see the",
    "view in",
    "not supported",
    "unsupported",
    "no renderer",
    "use the web",
    "use the ade",
];

/// Something worth saying about a surface that is still going to be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// Where in the surface, as a `/`-joined path from its root.
    pub path: String,
    /// What to tell whoever wrote it.
    pub message: String,
}

/// Check a surface, and collect what is only worth a nudge.
///
/// # Errors
///
/// [`SurfaceError`] for the first thing that makes the surface undrawable.
pub fn validate(surface: &Surface) -> Result<Vec<Warning>, SurfaceError> {
    surface.validate()?;
    let mut warnings = Vec::new();
    walk(surface, 1, &mut String::new(), &mut warnings)?;
    Ok(warnings)
}

fn walk(
    surface: &Surface,
    depth: usize,
    path: &mut String,
    warnings: &mut Vec<Warning>,
) -> Result<(), SurfaceError> {
    if depth > MAX_DEPTH {
        return Err(SurfaceError::TooDeep {
            depth,
            max: MAX_DEPTH,
        });
    }
    match &surface.kind {
        SurfaceKind::Stack { children, .. } => {
            let base = path.len();
            for (index, child) in children.iter().enumerate() {
                path.push_str(&format!("/children/{index}"));
                walk(child, depth + 1, path, warnings)?;
                path.truncate(base);
            }
        }
        SurfaceKind::Custom { fallback, .. } => {
            if let Some(message) = trivial_fallback(fallback) {
                warnings.push(Warning {
                    path: format!("{path}/fallback"),
                    message,
                });
            }
            let base = path.len();
            path.push_str("/fallback");
            walk(fallback, depth + 1, path, warnings)?;
            path.truncate(base);
        }
        _ => {}
    }
    Ok(())
}

/// Why this fallback looks like a shrug, when it does.
///
/// Only `text` fallbacks are judged: a table or a tree fallback is, by
/// construction, saying something.
fn trivial_fallback(fallback: &Surface) -> Option<String> {
    let SurfaceKind::Text { value, .. } = &fallback.kind else {
        return None;
    };
    let trimmed = value.trim();
    // The phrase check comes first: "open the web UI" is also short, and naming
    // the phrase is the more useful of the two things to say about it.
    let lowered = trimmed.to_lowercase();
    if let Some(phrase) = LAZY_PHRASES.iter().find(|p| lowered.contains(*p)) {
        return Some(format!(
            "a custom surface's fallback says `{phrase}`; it is meant to describe \
             what the rich version shows, not to send the reader somewhere else"
        ));
    }
    if trimmed.chars().count() < MIN_FALLBACK_CHARS {
        return Some(format!(
            "a custom surface's fallback is {} characters long; a client with no \
             renderer for it will show that and nothing else",
            trimmed.chars().count()
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use orrery_proto::{StackDir, Surface, SurfaceKind};

    use super::{MAX_DEPTH, validate};
    use crate::SurfaceError;

    fn text(value: &str) -> Surface {
        Surface::new(SurfaceKind::Text {
            value: value.to_owned(),
            style: None,
        })
    }

    fn custom(kind: &str, fallback: Surface) -> Surface {
        Surface::new(SurfaceKind::Custom {
            kind: kind.to_owned(),
            payload: serde_json::json!({}),
            fallback: Box::new(fallback),
        })
    }

    /// `kind: "flamegraph"` is rejected; `kind: "buildgraph.flamegraph"` is not.
    /// Two extensions must not be able to claim the same renderer.
    #[test]
    fn custom_kind_is_namespaced() {
        let bare = custom("flamegraph", text("the build took 41s, mostly in codegen"));
        assert!(
            matches!(
                validate(&bare),
                Err(SurfaceError::Malformed(
                    orrery_proto::SurfaceError::UnnamespacedCustomKind { .. }
                ))
            ),
            "an un-namespaced kind is refused"
        );

        let namespaced = custom(
            "buildgraph.flamegraph",
            text("the build took 41s, mostly in codegen"),
        );
        assert_eq!(validate(&namespaced).expect("it validates"), Vec::new());
    }

    /// A fallback that gave up is a warning, never an error: we cannot judge
    /// content, but we can nudge.
    #[test]
    fn fallback_must_not_be_trivial() {
        let lazy = custom("buildgraph.dag", text("open the web UI"));
        let warnings = validate(&lazy).expect("still drawable");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("open the"));
        assert_eq!(warnings[0].path, "/fallback");

        let terse = custom("buildgraph.dag", text("a dag"));
        let warnings = validate(&terse).expect("still drawable");
        assert_eq!(warnings.len(), 1, "too short to be saying anything");

        let real = custom("buildgraph.dag", text("proto -> agui -> kernel, 3 crates"));
        assert!(validate(&real).expect("fine").is_empty());

        // A non-text fallback is, by construction, describing something.
        let tabular = custom(
            "buildgraph.dag",
            Surface::new(SurfaceKind::Table {
                columns: vec!["crate".into()],
                rows: vec![],
            }),
        );
        assert!(validate(&tabular).expect("fine").is_empty());
    }

    /// A thousand-deep stack is refused before it reaches a renderer.
    #[test]
    fn nesting_depth_is_capped() {
        let mut deep = text("bottom");
        for _ in 0..1000 {
            deep = Surface::new(SurfaceKind::Stack {
                dir: StackDir::Column,
                title: None,
                collapsed: false,
                children: vec![deep],
            });
        }
        assert!(
            matches!(
                validate(&deep),
                Err(SurfaceError::TooDeep { max: MAX_DEPTH, .. })
            ),
            "a thousand deep is refused"
        );

        let mut fine = text("bottom");
        for _ in 0..(MAX_DEPTH - 1) {
            fine = Surface::new(SurfaceKind::Stack {
                dir: StackDir::Column,
                title: None,
                collapsed: false,
                children: vec![fine],
            });
        }
        assert!(validate(&fine).is_ok(), "and the cap itself is not");
    }
}
