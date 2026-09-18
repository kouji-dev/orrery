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
