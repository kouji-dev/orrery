//! Discovery across the layers, and the scope that decides who loads what.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use orrery_proto::Layer;
use orrery_skills::discover::{LayerRoot, Settings};
use orrery_skills::{SkillSettings, SkillSource, discover};

/// `<root>/.orrery/skills/<name>/SKILL.md`, the layout plan 10 discovers.
fn write_skill(root: &Path, name: &str, description: &str) -> PathBuf {
    let dir = root.join(".orrery/skills").join(name);
    std::fs::create_dir_all(&dir).expect("a skill directory");
    let path = dir.join("SKILL.md");
    std::fs::write(
        &path,
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
    )
    .expect("a SKILL.md");
    path
}

struct Tree {
    _temp: tempfile::TempDir,
    roots: Vec<LayerRoot>,
    root: PathBuf,
}

fn three_layers() -> Tree {
    let temp = tempfile::tempdir().expect("a temp dir");
    let root = dunce::canonicalize(temp.path()).unwrap_or_else(|_| temp.path().to_path_buf());

    write_skill(&root.join("user"), "shared-checklist", "the user's");
    write_skill(&root.join("workspace"), "house-style", "the workspace's");
    write_skill(&root.join("project"), "release-notes", "the project's");

    let roots = vec![
        LayerRoot {
            layer: Layer::Project,
            dir: root.join("project/.orrery"),
        },
        LayerRoot {
            layer: Layer::Workspace,
            dir: root.join("workspace/.orrery"),
        },
        LayerRoot {
            layer: Layer::User,
            dir: root.join("user/.orrery"),
        },
    ];
    Tree {
        _temp: temp,
        roots,
        root,
    }
}

#[test]
fn found_across_layers() {
    let tree = three_layers();
    let found = discover(&tree.roots, &Settings::new());

    assert!(found.problems.is_empty(), "{:?}", found.problems);
    let mut names: Vec<&str> = found.skills.all().iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["house-style", "release-notes", "shared-checklist"]);

    // The layer a skill came from rides along, and so does the source it maps
    // to — `plan 04`'s precedence needs the first and the load ledger the
    // second.
    let user = found.skills.get("shared-checklist").expect("the user's");
    assert_eq!(user.layer, Layer::User);
    assert_eq!(user.source, SkillSource::User);

    let project = found.skills.get("release-notes").expect("the project's");
    assert_eq!(project.layer, Layer::Project);
    assert_eq!(project.source, SkillSource::Workspace);

    // And the description came from the front matter, not from the directory
    // name: this is a parse plus a walk, in one pass.
    assert_eq!(project.description, "the project's");
}

#[test]
fn scope_limits_which_agents_load_it() {
    let tree = three_layers();
    let mut settings: Settings = BTreeMap::new();
    settings.insert(
        "release-notes".to_owned(),
        SkillSettings::default().for_agents(["releaser"]),
    );

    let found = discover(&tree.roots, &settings);

    let releaser: Vec<&str> = found
        .skills
        .for_agent("releaser")
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(releaser.contains(&"release-notes"));

    let reviewer: Vec<&str> = found
        .skills
        .for_agent("reviewer")
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        !reviewer.contains(&"release-notes"),
        "a scoped skill must be ABSENT from another agent's context, not merely refused there"
    );
    // The unscoped ones are still there for everybody.
    assert!(reviewer.contains(&"house-style"));
    assert!(reviewer.contains(&"shared-checklist"));
}

#[test]
fn a_skill_that_does_not_parse_is_one_skill_missing_not_a_failed_load() {
    let tree = three_layers();
    let broken = tree.root.join("project/.orrery/skills/broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("SKILL.md"), "# no front matter\n").unwrap();

    let found = discover(&tree.roots, &Settings::new());
    assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    assert_eq!(found.skills.len(), 3, "the other three still loaded");
}

#[test]
fn a_skill_carries_its_grant_from_configuration_not_from_the_file() {
    let tree = three_layers();
    let mut settings: Settings = BTreeMap::new();
    settings.insert(
        "house-style".to_owned(),
        SkillSettings::default().granting(orrery_proto::GrantSpec::default()),
    );

    let found = discover(&tree.roots, &settings);
    assert!(found.skills.get("house-style").unwrap().grant.is_some());
    // And a skill nobody wrote a grant for has none, which is what stops its
    // scripts running at all.
    assert!(found.skills.get("release-notes").unwrap().grant.is_none());
}
