//! `orrery skills` - what the discovery pass found, and what each one asks for.
//!
//! # Why this command exists
//!
//! A skill is documentation that can run code. `orrery-skills` makes that safe
//! — a skill's `scripts/` run under a declared grant, brokered and audited,
//! and a skill with no declared grant cannot run a script at all — and until
//! this command landed none of it was visible from the binary. "Which skills
//! are in force, and which of them may execute something" is the question the
//! design exists to answer, so it needs somewhere to be asked.
//!
//! # The grant column is the point
//!
//! `list` prints what each skill's scripts may do, or `-` when they may do
//! nothing. Everywhere else a bundled script runs with the user's full
//! privileges; here the absence of a grant is a visible, ordinary state.
//!
//! # No kernel, no provider
//!
//! Discovery is a read of the layer directories. It does not need a model, and
//! this command does not build one.
//!
//! Implementation plan: `harness/docs/plans/13-skills-mcp.md`, rendered by
//! `harness/docs/plans/17-cli.md` task 11.

use orrery_skills::discover::{LayerRoot, Settings};
use orrery_skills::{SkillRef, SkillSettings};

use crate::args::{Cli, SkillsCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch a `skills` subcommand.
pub fn dispatch(cli: &Cli, command: &SkillsCommand) -> ! {
    match command {
        SkillsCommand::List { agent } => list(cli, agent.as_deref()),
        SkillsCommand::Show { name } => show(cli, name),
    }
}

/// Every skill in force, with its layer and what its scripts may do.
fn list(cli: &Cli, agent: Option<&str>) -> ! {
    let found = discover(cli);
    // `for_agent` is not "list then refuse": a skill scoped elsewhere is
    // **absent** from that agent's context, so it is absent here too.
    let owned: Vec<SkillRef>;
    let skills: Vec<&SkillRef> = match agent {
        Some(agent) => found.skills.for_agent(agent),
        None => {
            owned = found.skills.all().to_vec();
            owned.iter().collect()
        }
    };

    let json = layers::wants_json(cli);
    for skill in &skills {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "name": skill.name,
                    "description": skill.description,
                    "layer": format!("{:?}", skill.layer).to_lowercase(),
                    "path": skill.path.display().to_string(),
                    "scope": skill.scope,
                    // `null`, not `{}`: a skill whose scripts cannot run is a
                    // different thing from one granted nothing in particular.
                    "grant": skill.grant,
                })
            );
        } else {
            println!(
                "{:<24} {:<10} {:<8} {}",
                skill.name,
                format!("{:?}", skill.layer).to_lowercase(),
                if skill.grant.is_some() {
                    "scripts"
                } else {
                    "-"
                },
                skill.description.lines().next().unwrap_or_default()
            );
        }
    }
    if skills.is_empty() {
        eprintln!("orrery: no skills found in the layers in force");
    }
    // A skill that would not parse is one skill missing, never a failed
    // command — but it is said out loud, on stderr.
    for problem in &found.problems {
        eprintln!("orrery: {problem}");
    }
    Exit::Ok.exit()
}

/// One skill: its front matter, and the body an agent would be given.
fn show(cli: &Cli, name: &str) -> ! {
    let found = discover(cli);
    let Some(skill) = found.skills.get(name) else {
        fail(
            Exit::Usage,
            format!("no skill `{name}`. `orrery skills list` shows what there is."),
        );
    };
    let doc =
        orrery_skills::parse::skill_file(&skill.path).unwrap_or_else(|e| fail(Exit::Usage, e));

    if layers::wants_json(cli) {
        println!(
            "{}",
            serde_json::json!({
                "name": doc.name,
                "description": doc.description,
                "path": doc.path.display().to_string(),
                "layer": format!("{:?}", skill.layer).to_lowercase(),
                "grant": skill.grant,
                "body": doc.body,
            })
        );
        Exit::Ok.exit();
    }

    println!("name        {}", doc.name);
    println!("description {}", doc.description);
    println!("layer       {:?}", skill.layer);
    println!("path        {}", doc.path.display());
    match &skill.grant {
        Some(_) => println!("scripts     run under a declared grant"),
        None => println!("scripts     cannot run: no grant declared"),
    }
    println!();
    println!("{}", doc.body);
    Exit::Ok.exit()
}

/// Discover over the layers in force.
///
/// The same layer roots `config explain` resolves, so a skill this command
/// lists is a skill that configuration would have loaded — not a second walk
/// that can disagree with the first.
///
/// The settings come from configuration too, and they have to: the `SKILL.md`
/// format is adopted **unchanged**, so `scope` and `grant` are not front-matter
/// keys. A skill cannot grant itself anything; somebody with a config file has
/// to, which is the whole reason a skill here is safer than a skill anywhere
/// else.
fn discover(cli: &Cli) -> orrery_skills::discover::Found {
    let resolved = layers::resolve(cli);
    let roots: Vec<LayerRoot> = resolved
        .layers
        .iter()
        .filter_map(|l| {
            l.path.parent().map(|dir| LayerRoot {
                layer: l.layer,
                dir: dir.to_path_buf(),
            })
        })
        .collect();
    orrery_skills::discover(&roots, &settings(&resolved))
}

/// What configuration says about each skill, by name.
///
/// `[skills.<name>]` with `scope` and `grant` under it. Read back out of the
/// flattened, provenanced values rather than re-parsing the files, so what this
/// command reports is what the layers actually resolved to.
fn settings(resolved: &orrery_config::ResolvedConfig) -> Settings {
    let mut out = Settings::new();
    for key in resolved.values.keys_under("skills") {
        let mut parts = key.split('.').skip(1);
        let Some(name) = parts.next() else { continue };
        if out.contains_key(name) {
            continue;
        }
        let mut table = toml::map::Map::new();
        for field in ["scope", "grant"] {
            if let Some(slot) = resolved.values.winner(&format!("skills.{name}.{field}")) {
                table.insert(field.to_owned(), slot.value.clone());
            }
        }
        if table.is_empty() {
            continue;
        }
        match toml::Value::Table(table).try_into::<SkillSettings>() {
            Ok(settings) => {
                out.insert(name.to_owned(), settings);
            }
            // A malformed `[skills.<name>]` is one skill's settings missing,
            // said out loud, not a failed command.
            Err(e) => eprintln!("orrery: `[skills.{name}]` did not parse: {e}"),
        }
    }
    out
}
