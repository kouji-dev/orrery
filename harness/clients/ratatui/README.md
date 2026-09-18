# orrery-client-ratatui

The default Orrery TUI: the terminal client that ships inside the `orrery`
binary. No Node at runtime, same language as the kernel.

Plan: [`harness/docs/plans/09b-client-ratatui.md`](../../docs/plans/09b-client-ratatui.md)

## The hybrid screen

```
┌─ native scrollback ────────────────────────────┐
│  turn 1  (printed once, never redrawn)         │   ← selection + scroll are the terminal's
│  turn 2  (printed once, never redrawn)         │
├─ live region (ratatui viewport) ───────────────┤
│  turn 3  streaming…                            │   ← redrawn at the frame budget
├─ consent bar (when there is one) ──────────────┤
├─ composer ─────────────────────────────────────┤
└─ footer ───────────────────────────────────────┘
```

A settled turn goes through `Terminal::insert_before` **once**, keyed by run id,
and is never touched again: selection, the scrollwheel and `grep` on the
terminal's own buffer all keep working. Only the live region costs anything per
frame.

Two consequences worth knowing before changing anything here:

- **No alternate screen.** That second buffer has no scrollback; entering it is
  how a full-screen TUI throws the transcript away on exit.
- **No mouse capture, ever.** Capturing the mouse takes click-and-drag away from
  the terminal, which is the selection the whole model exists to preserve. See
  `src/terminal.rs`.

## What this crate does and does not do

It **draws**. All decoding, `seq` tracking and store mutation live in
`orrery-client`; nothing here parses an AG-UI event. It attaches over the
in-process transport exactly the way an external client attaches over a pipe,
and it reads `SurfaceStore`, never kernel state.

## Layout

| Path | What |
|---|---|
| `src/app.rs` | the loop, the scrollback/live split, the key routing |
| `src/live.rs` | the turn in flight |
| `src/scrollback.rs` | the `Scrollback` trait and its recording test double |
| `src/terminal.rs` | raw mode, bracketed paste, the real `insert_before` |
| `src/composer.rs` | input, history, `^C`/`^D`/`^L` |
| `src/footer.rs` | one line of cost and keys |
| `src/theme.rs` | a small palette of its own, not the ADE's CSS tokens |
| `src/widgets/` | one widget per `SurfaceKind`, each a pure function |

## Degradation, as asserted

| Surface | Constrained terminal |
|---|---|
| `diff` | markers always drawn; colour is the second signal, never the only one |
| `form` | over four fields ⇒ sequential prompts, one at a time |
| `table` | narrower than content ⇒ elide middle columns, never wrap a cell |
| `custom` | ⇒ always the fallback, named |
| `markdown` | `complete: false` ⇒ plain text |
| `progress` | no `total` ⇒ no bar, because a bar that does not move is a lie |

Every one has a snapshot at 24 columns without colour in `tests/snapshots/`.

## Tests

```bash
cargo test -p orrery-client-ratatui
cargo test -p orrery-client-ratatui --test smoke -- --ignored   # the loop end to end
INSTA_UPDATE=always cargo test -p orrery-client-ratatui          # re-take snapshots
```

`tests/conformance.rs` runs all 16 shared fixtures and holds the check that
fails when `SurfaceKind` grows a variant with no widget.

## Not yet wired

`orrery-cli` still exits 2 for the bare `orrery`, so this client is not reachable
from the binary yet. That link belongs to plan 17.
