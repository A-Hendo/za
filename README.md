# za

Resume AI agent sessions across Zellij restarts.

Zellij already saves your sessions, tabs, panes and their working directories,
and can resurrect them. What it can't do is bring an agent back to the
*conversation* it was in: it saves the command a pane was started with
(`claude`), so a restore starts a fresh session. And plain `zellij` starts
a new empty session instead of putting back what you had open.

za fills those two gaps:

- **Agent panes resume their conversation.** Agents started in a Zellij pane
  run through `za run`, which gives the pane a stable slot id inside its
  command line (`za run --slot k3f9ab12 claude ...`). Zellij saves that
  command line verbatim. A session-start hook records which conversation the
  slot is in, so after a restore the pane runs `claude --resume <id>`.
- **`za` brings back everything that was open.** While you work, the
  launcher keeps track of which sessions are open and which one you're in.
  After a reboot or crash, `za` resurrects all of them (agents resumed) and
  attaches you to the one you were in. Sessions you quit on purpose stay
  quit.

Status panels like zj-agent-mob keep working unchanged:
resumed agents fire their normal hooks.

## Setup

```sh
cargo install --path .
za install                      # add za's hook to each installed agent
echo 'eval "$(za init zsh)"' >> ~/.zshrc   # or bash / fish
```

Then launch Zellij with `za` instead of `zellij` (or `alias zj=za`).

Inside Zellij you keep typing `claude`, `copilot`, `kimi`, `agy` or `grok` as
usual. The shell functions only exist inside Zellij and keep any existing
alias working (`alias agy="agy --dangerously-skip-permissions"` still applies).

## Commands

| Command | |
|---|---|
| `za` | Restore the sessions open last time and attach to the one you were in |
| `za attach <name>` | Attach to (or resurrect) one session |
| `za new [name]` | Start a new session |
| `za status` | Show tracked sessions and agent panes |
| `za install [agent\|all]` / `za uninstall` | Manage agent hooks |
| `za init <zsh\|bash\|fish>` | Print the shell functions |
| `za gc` | Forget agent panes no saved layout refers to |
| `za run <agent> [args]` | The pane wrapper (used by the shell functions) |
| `za hook <agent>` | The hook entry point (used by agent configs) |

## Supported agents

| Agent | Resumes with | Hook installed in |
|---|---|---|
| claude | `--resume <id>` | `~/.claude/settings.json` (SessionStart) |
| copilot | `--resume <id>` | `~/.copilot/hooks/za.json` (sessionStart) |
| kimi | `--session <id>` | `~/.kimi-code/config.toml` (SessionStart) |
| agy | `--conversation <id>` | `~/.gemini/config/hooks.json` (PreInvocation) |
| grok | `--resume <id>` | `~/.grok/hooks/za.json` (SessionStart) |

On resume the original command line is replayed minus anything that would
pick a different session: the initial prompt, `--continue`, `--resume`,
`--worktree` and similar are dropped, other flags (`--model`, permission
flags, ...) are kept. Print mode (`-p`), `--help` and subcommands
(`claude mcp ...`) are passed straight through and never tracked.

If a saved conversation can't be resumed (deleted, or the agent exits with an
error straight away), the pane starts a fresh session instead of dying. If an
agent has no hook installed, za falls back to the agent's "continue most
recent session in this directory" flag.

## How restore decides what to bring back

Zellij can't tell a reboot from a deliberate quit: both leave the session
EXITED. The `za` launcher watches sessions while it runs:

- a session that disappears while the launcher is alive was quit on purpose
  and is not restored;
- sessions still open when the launcher dies (reboot, crash, closed terminal)
  are restored next time.

Sessions started with plain `zellij` are picked up as soon as any `za`
launcher is running.

Sessions you quit can still be brought back with `za attach <name>`, with their
agents resumed.

## How agent panes are forgotten

- **You quit the agent:** it won't be resumed again. A pane that was itself
  restored has no shell behind it, so za gives it one, and that pane stays a
  shell on later restores. Ctrl+D then closes the pane like any other.
- **You close the pane or tab:** za checks that the session is still running
  without the pane, then marks it closed. Zellij saves layouts only
  periodically, so if a restore still uses an older layout, the pane closes
  itself straight away.
- **A quit, reboot or crash** doesn't count as closing a pane, so those agents
  come back with their session.
- **An agent killed by a signal** (out of memory, say) rather than quitting is
  still resumed.

Closing the terminal window only detaches: the session keeps running, and `za`
reattaches to it.

`za status` lists only what the next `za` would bring back. Finished panes
leave `state.json` once no saved layout refers to them.

## Files

`~/.config/za/state.json` holds slots and session tracking.
`~/.config/za/config.toml` (written by `za install`) holds settings:

| Setting | Default | |
|---|---|---|
| `run_commands_on_restore` | `true` | Start pane commands right away instead of Zellij's "press ENTER to run". za also updates the saved layouts of exited sessions so agent panes start right away from the session manager; other commands still ask there |
| `shell_after_exit` | `true` | A restored pane becomes a shell when its agent quits |
| `continue_fallback` | `true` | Use the agent's continue flag when no session id was recorded |
| `poll_interval_secs` | `2` | How often the launcher checks sessions |

`ZA_HOME` overrides the directory.

## Notes

- Linux and macOS. Process checks use `/proc` on Linux, `ps` elsewhere.
- Tested with Zellij 0.45. Closing panes relies on `zellij action list-panes`
  and `close-pane --pane-id`; without them za can't detect closed panes, and a
  restored pane shows Zellij's "EXIT CODE" screen when its shell exits.
- copilot's flags and hook format come from GitHub's docs, not a local
  install. Run `za status` after starting copilot in a pane to confirm a
  session id was recorded.
- A nested agent (e.g. `claude -p` run from inside a wrapped claude) inherits
  the pane's environment; the hook checks the process tree so it can't
  overwrite the pane's session.
