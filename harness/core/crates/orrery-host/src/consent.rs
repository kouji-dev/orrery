//! What a person is shown when an extension asks for more than it has.
//!
//! §4.7 sketches this as a terminal prompt. It is a [`Surface`] instead, for the
//! same reason everything else is: ratatui draws a widget, Ink draws a
//! component, `--json` emits the payload, and `orrery ext test` asserts on it as
//! data — from one description. A prompt written with `println!` would work in
//! exactly one client.
//!
//! The decision (open question 2 in the plan): **this shape is owned here**,
//! because `ext test` needs the same one `ext install` shows. Plan 15 wires it
//! to the installer and plan 07 turns an answer into a stored grant; neither
//! gets to invent a second shape for the same question.

use orrery_ext_api::ExtensionManifest;
use orrery_ext_api::broker::aspect_name;
use orrery_ext_api::manifest::covers;
use orrery_proto::{Capability, Cell, Choice, Grant, StackDir, Surface, SurfaceKind, TextStyle};

/// What an extension is asking for that it does not already have.
///
/// `None` when the grant already covers the manifest: there is nothing to ask,
/// and a prompt with nothing in it is worse than no prompt.
#[must_use]
pub fn grant_diff(manifest: &ExtensionManifest, granted: &Grant) -> Option<Surface> {
    let asks = manifest.capabilities();
    if asks.is_empty() {
        return None;
    }

    let rows: Vec<Vec<Cell>> = asks
        .iter()
        .map(|want| {
            let held = is_held(want, granted);
            vec![
                Cell {
                    text: aspect_name(want.aspect).to_owned(),
                    style: Some(TextStyle::Code),
                },
                Cell {
                    text: if want.scope.is_empty() {
                        "(everything)".to_owned()
                    } else {
                        want.scope.join(", ")
                    },
                    style: Some(TextStyle::Code),
                },
                Cell {
                    text: if held { "already granted" } else { "NEW" }.to_owned(),
                    style: Some(if held {
                        TextStyle::Muted
                    } else {
                        TextStyle::Warning
                    }),
                },
            ]
        })
        .collect();

    // Everything it wants, it already has.
    if rows.iter().all(|row| row[2].text != "NEW") {
        return None;
    }

    Some(Surface::new(SurfaceKind::Stack {
        dir: StackDir::Column,
        title: Some(format!(
            "`{name}` {version} asks for",
            name = manifest.name,
            version = manifest.version
        )),
        collapsed: false,
        children: vec![
            Surface::new(SurfaceKind::Table {
                columns: vec!["aspect".into(), "over".into(), "status".into()],
                rows,
            }),
            Surface::new(SurfaceKind::Question {
                prompt: format!(
                    "Load `{name}` with these capabilities?",
                    name = manifest.name
                ),
                choices: vec![
                    Choice {
                        value: "allow".into(),
                        label: "Allow, and remember this".into(),
                    },
                    Choice {
                        value: "once".into(),
                        label: "Allow for this session only".into(),
                    },
                    Choice {
                        value: "deny".into(),
                        label: "Deny; start without it".into(),
                    },
                ],
                multi: false,
                free: false,
                // No default, and no deadline: a capability grant is not
                // something to time out into.
                default: None,
                deadline_ms: None,
            }),
        ],
    }))
}

/// Whether a grant already covers one ask, scope by scope.
fn is_held(want: &Capability, granted: &Grant) -> bool {
    if want.scope.is_empty() {
        return covers(&granted.capabilities, want.aspect, None);
    }
    want.scope
        .iter()
        .all(|entry| covers(&granted.capabilities, want.aspect, Some(entry)))
}

#[cfg(test)]
mod tests {
    use super::grant_diff;
    use orrery_ext_api::ExtensionManifest;
    use orrery_proto::{Aspect, Capability, Consent, Grant, SurfaceKind};

    const MANIFEST: &str = r#"
[extension]
api     = "orrery-ext/1"
name    = "buildgraph"
version = "1.2.0"
runtime = "native"

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
"#;

    fn manifest() -> ExtensionManifest {
        ExtensionManifest::from_toml_str(MANIFEST, "orrery.toml").unwrap()
    }

    #[test]
    fn what_is_new_is_marked_new() {
        let granted = Grant {
            capabilities: vec![Capability::scoped(Aspect::Read, ["$WORKSPACE/**"])],
            consent: Consent::Once,
        };
        let surface = grant_diff(&manifest(), &granted).expect("spawn is new");

        let SurfaceKind::Stack { children, .. } = &surface.kind else {
            panic!("expected a stack, got {:?}", surface.kind);
        };
        let SurfaceKind::Table { rows, .. } = &children[0].kind else {
            panic!("expected a table first");
        };
        let statuses: Vec<&str> = rows.iter().map(|r| r[2].text.as_str()).collect();
        assert_eq!(statuses, ["already granted", "NEW"]);

        // And the question is a Question, so every client can draw it.
        assert!(matches!(children[1].kind, SurfaceKind::Question { .. }));
        surface.validate().unwrap();
    }

    #[test]
    fn nothing_new_is_nothing_to_ask() {
        let granted = Grant {
            capabilities: vec![
                Capability::scoped(Aspect::Read, ["$WORKSPACE/**"]),
                Capability::all(Aspect::Spawn),
            ],
            consent: Consent::Always,
        };
        assert!(grant_diff(&manifest(), &granted).is_none());
    }

    #[test]
    fn an_extension_that_asks_for_nothing_is_not_asked_about() {
        let src = MANIFEST
            .split("[requires]")
            .next()
            .expect("the manifest has a head");
        let manifest = ExtensionManifest::from_toml_str(src, "orrery.toml").unwrap();
        assert!(grant_diff(&manifest, &Grant::nothing()).is_none());
    }
}
