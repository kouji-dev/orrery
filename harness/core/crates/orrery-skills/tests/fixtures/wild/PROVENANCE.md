# Where these fixtures came from

Every `*.SKILL.md` here is a **verbatim, unedited copy** of a skill published
for another agent runtime. Nothing was reformatted, reordered or trimmed to
suit `orrery-skills`: `parse::real_skills_from_the_wild` is only evidence if the
bytes are the bytes somebody else shipped.

| fixture | upstream | what it exercises |
|---|---|---|
| `test-driven-development.SKILL.md` | `superpowers` plugin (Anthropic plugin marketplace), v6.3.0, `skills/test-driven-development` | the bare spec: `name` + `description`, nothing else |
| `frontend-design.SKILL.md` | `claude-plugins-official/frontend-design`, `skills/frontend-design` | an unknown scalar key (`license`) |
| `playwright-cli.SKILL.md` | `playwright-core`, `lib/tools/skills/playwright-cli` | `allowed-tools` — a plain scalar full of `(`, `)` and `:` |
| `angular-developer.SKILL.md` | Google, `~/.agents/skills/angular-developer` | an unknown **nested mapping** (`metadata.author`, `metadata.version`) |
| `kouji-analyzer.SKILL.md` | `kouji` plugin v1.1.0, `skills/analyzer` | a `name` containing a colon (`kouji:analyzer`) |
| `docx.SKILL.md` | Anthropic, synced `docx` skill | a double-quoted multi-sentence description containing `'`, `:` and `.` |

These files are copies of third-party documentation held for testing only. They
are not part of the built artefact: no `orrery-*` crate reads them outside
`cfg(test)`.
