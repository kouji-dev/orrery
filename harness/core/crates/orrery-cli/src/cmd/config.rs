//! `orrery config explain` - which layer won, and where it is written.
//!
//! [`orrery_config::explain`] does the work; this prints it. The one thing the
//! command insists on is that the **shadowed** values are printed too, because
//! naming only the winner is how somebody spends an afternoon editing a file
//! that is not in force.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 7,
//! surfaced by `harness/docs/plans/17-cli.md` task 6.

use orrery_config::explain::Contribution;
use orrery_config::{Explanation, Fold};

use crate::args::{Cli, ConfigCommand};
use crate::cmd::layers;
use crate::exit::Exit;

/// Dispatch a `config` subcommand.
pub fn dispatch(cli: &Cli, command: &ConfigCommand) -> ! {
    let ConfigCommand::Explain { key } = command;
    let resolved = layers::resolve(cli);
    let explanation = resolved.explain(key);

    if layers::wants_json(cli) {
        println!("{}", json(&explanation));
    } else {
        println!("{explanation}");
    }
    Exit::Ok.exit()
}

fn contribution(c: &Contribution) -> serde_json::Value {
    serde_json::json!({
        // The key this one was read from: a table answers with its leaves, and
        // they are not all the key that was typed.
        "from": c.from,
        "value": serde_json::to_value(&c.value).unwrap_or(serde_json::Value::Null),
        "layer": format!("{:?}", c.origin.layer).to_lowercase(),
        "file": c.origin.file.display().to_string(),
        "line": c.origin.line,
        "in_force": c.in_force,
    })
}

/// One object on one line: the key, how it folds, the winner and everything it
/// beat. A key nobody set is not an error — it is an empty `contributions` and
/// a null `winner`.
fn json(explanation: &Explanation) -> serde_json::Value {
    serde_json::json!({
        "key": explanation.key,
        "resolved_from": explanation.resolved_from,
        "fold": match explanation.fold {
            Fold::Union => "union",
            Fold::Override => "override",
        },
        "winner": explanation.winner().map(contribution),
        "contributions": explanation
            .contributions
            .iter()
            .map(contribution)
            .collect::<Vec<_>>(),
    })
}
