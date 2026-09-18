# orrery-ext-graders

The built-in graders: command, assertion and model.

**Manifest field.** `graders` — this crate is a first-party implementation of
`ExtensionDefinition.graders`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

| Grader | How it scores | Use for |
|---|---|---|
| `command` | The exit code of a script | SWE-style patch benchmarks |
| `assertion` | Declared checks on files, diffs and tool calls | Behaviour and safety |
| `model` | A judge model against a rubric, **with its own cost counted** | Open-ended quality |

## Two rules all three share

- **Everything is brokered.** The command grader spawns through the broker and
  the assertion grader reads through it. A grader runs a command line out of
  somebody else's suite file; the one thing it must not be is the trusted
  process.
- **A grader that could not look is not a grader that said no.** A denied
  spawn, an unbound session store or an unreadable config is a `GradeError`,
  which the runner records as `error` — never as the case failing. A safety
  assertion that could not be evaluated must not look like one that held.

## The judge's cost is not the run's cost

`Score::judge_cost` is separate from the run's own cost, and it is summed from
the provider's usage events exactly the way the runner sums its own. A judge
that costs more than the run it grades shows up in the report as itself.

Implementation plan:
[`harness/docs/plans/16-eval-runner.md`](../../../docs/plans/16-eval-runner.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

| Capability | Why |
|---|---|
| `spawn = ["*"]` | A command grader runs whatever the eval names — `pytest`, `cargo test`, a shell script. The set cannot be known here, so it is asked for wide and **narrowed by the suite's own policy**, which does know. |
| `read = ["$WORKSPACE/**"]` | An assertion grader reads the artefacts it is grading: the files the run produced. |

`spawn = ["*"]` is the widest request in this tree and it is asked for openly
rather than smuggled: an eval that does not want it denies it, the command grader
is disabled, and the assertion and model graders keep working. That is what
per-tool `requires` is for.

## Tests

```
cargo test -p orrery-ext-graders
```

Through the mock broker: a command's exit code becomes a verdict, an assertion
reads what the run wrote, and the model grader **replays a committed fixture**.
No model is called and no network is touched.
