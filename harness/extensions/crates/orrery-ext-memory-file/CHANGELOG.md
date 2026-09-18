# Changelog

## Unreleased

- Implemented. JSONL under `$STATE/memory/`, `global` and `session` scopes only,
  substring-plus-recency retrieval, `forget` by selector. Passes
  `orrery_memory::conformance::run_conformance`. Off by default: nothing in
  `orrery-harness`'s `default` feature set names it.
