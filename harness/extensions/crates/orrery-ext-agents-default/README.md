# orrery-ext-agents-default

The shipped role agents: planner, executor, verifier, compactor and summariser.

**Manifest field.** `agents` — this crate is a first-party implementation of
`ExtensionDefinition.agents`, and it loads through `orrery-host` like any third-party
extension: the ledger shows it, a deny rule disables it, `orrery ext test` runs it.

**Why an extension.** The kernel *names* the roles; it does not bind them. Compiling
the five defaults into the loop would give one path for the roles everybody uses and a
different one for everybody else's — and "plan with a large model, compact with a
cheap one" would stop being configuration. Replacing the planner is a `[roles]` line in
a config layer, and layer precedence decides between this bundle's planner and
somebody else's.

**No model id is named here.** An agent declares a model *class* — `default`, `large`
or `cheap` — and a profile binds the class to a model. The same indirection as the
provider layer, for the same reason: model names change every few months, and a
shipped default that names one is wrong on a schedule.

**Every agent declares a budget**, because an agent that cannot terminate is a cost
incident. A profile narrows those budgets; narrowing is the only direction available,
and the same is true of what each agent asks for — a binding is intersected with the
step's grant and never widens it.

**`router` and `grader` are not shipped bound.** Leaving `router` unbound keeps routing
declarative and reproducible, which is what eval comparison needs; binding it to an
agent is how a model router happens, and it is the same mechanism rather than a new
one. A grader belongs to the eval suite that defines what a good answer is.

Implementation plan:
[`harness/docs/plans/11-router-roles-orchestrator.md`](../../../docs/plans/11-router-roles-orchestrator.md).

See [`orrery.toml`](orrery.toml) for what it provides and what it requires.

## What it asks for, and why

**Nothing.** `[requires]` is empty. Agents are declarations — a model class, a
prompt, a budget, a tool allow-list — and a declaration reads no file and runs no
program. Every effect happens later, in the tools a step is allowed, under that
step's own grant.

That emptiness is worth noticing: it is what lets this bundle load under a grant
that denies everything, and it is why replacing the planner cannot widen anyone's
access.

## Tests

```
cargo test -p orrery-ext-agents-default
```

The suite runs against the mock broker in `orrery-ext-api::testing` — the same
harness `orrery ext test` gives a community author, and the same one a third
party would use on their own agent bundle.
