# CLAUDE.md

za is a small Rust CLI that resumes AI agent sessions (claude, copilot, kimi,
agy, grok) across Zellij restarts. README.md is the user-facing spec: keep it
in sync when behaviour, commands, settings or supported agents change.

## Build and check

```sh
cargo build
cargo test
cargo clippy --all-targets   # keep warning-free
```

The code isn't rustfmt-clean, so don't run `cargo fmt` over whole files: it
would bury real changes in reformatting.

No CLI-parsing crate: `src/main.rs` dispatches subcommands by hand. Keep
dependencies minimal (libc, serde, serde_json, toml).

## Layout

| File | Role |
|---|---|
| `main.rs` | Subcommand dispatch, usage text, `za status` |
| `restore.rs` | The launcher (`za`, `za attach`, `za new`): restores sessions, polls them to tell a deliberate quit from a reboot, `za gc` |
| `run.rs` | `za run`: per-pane wrapper. Re-execs as `za run --slot <id> ...` so Zellij saves the slot id in the pane command, then supervises the agent |
| `hook.rs` | `za hook`: called by agent session-start hooks, records the session id for the slot |
| `agents.rs` | Per-agent table: resume/continue flags, flags dropped on resume, one-shot flags, where the hook payload carries the session id |
| `install.rs` | `za install`/`uninstall` (edits agent configs, with backups) and `za init` (shell functions) |
| `state.rs` | `state.json`; every write goes through `state::update` (exclusive lock + atomic replace), since hooks, wrappers and launchers write concurrently |
| `zellij.rs` | Zellij CLI calls and its cache dir (serialized layouts) |
| `config.rs` | Optional `config.toml` settings |
| `proc.rs` | Process-tree checks (`/proc` on Linux, `ps` elsewhere) |

The wrapper passes `ZA_SLOT`, `ZA_PID` and `ZA_AGENT` to the agent; the hook
uses them, plus a process-tree check, to find its slot.

## Conventions

- `za hook` must be fast, silent and always exit 0: a failing hook blocks or
  clutters the agent it runs in.
- Adding an agent means an `Agent` entry in `agents.rs`, an `Integration` in
  `install.rs`, the shell functions in `za init`, and the README tables.
- Unit tests live in `#[cfg(test)]` modules next to the code. Prefer pure
  functions (flag stripping, layout parsing) so they're testable without
  Zellij.
- Set `ZA_HOME` to a temp dir when running za by hand so the real
  `~/.config/za` isn't touched.

## Testing against a real Zellij

End-to-end tests that attach a client to a throwaway Zellij can take down the
user's real terminal if done carelessly:

- Never feed a test client stdin from `/dev/zero`. Hold stdin open with
  `sleep N | setsid timeout N script -qfc "..." /dev/null`.
- Never clean up with `pkill -f`. Stop only the test's own session with
  `zellij kill-session <name>`.
- Use a short socket dir such as `/tmp/zl.XXXX`; long paths exceed Zellij's
  socket path limit.

## Git commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

- Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`,
  `chore`, `style`, `revert`.
- Scope is optional and names the area, usually a module or agent:
  `fix(hook): ...`, `feat(kimi): ...`, `docs(readme): ...`.
- Description: imperative mood, lowercase, no trailing period, under ~72
  characters (`feat: resume grok sessions`, not `Added grok support.`).
- Body explains why, wrapped at 72 columns.
- Breaking changes: `!` after the type/scope (`feat!: ...`) and a
  `BREAKING CHANGE: ...` footer, e.g. for changes to `state.json` or
  `config.toml` that old versions can't read.
- One logical change per commit.
