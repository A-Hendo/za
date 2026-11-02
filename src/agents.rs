//! What za knows about each agent CLI: how to resume a session, which flags
//! take values, and which invocations aren't interactive sessions at all.
//!
//! On resume the original command line is replayed minus anything that would
//! start a different session (a prompt, `--continue`, `--worktree`, ...).

use serde_json::Value;
use std::path::Path;

/// How many arguments a flag consumes after itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Val {
    No,
    One,
    /// An optional value: the next arg, unless it looks like a flag.
    Opt,
    /// Every following arg up to the next flag (commander-style `<x...>`).
    Many,
}

use Val::*;

pub struct Agent {
    pub name: &'static str,
    /// Appended with the session id to resume it (a flag, or a subcommand
    /// like codex's `resume` that may follow the other flags).
    pub resume_flag: &'static str,
    /// Appended to continue the most recent session in this directory.
    pub continue_args: &'static [&'static str],
    /// Flags removed from the command line when resuming.
    pub drop: &'static [(&'static str, Val)],
    /// Value-taking flags that are kept when resuming.
    pub values: &'static [(&'static str, Val)],
    /// Flags meaning "not an interactive session" (print mode, help, ...).
    pub oneshot: &'static [&'static str],
    pub subcommands: &'static [&'static str],
    /// Positional args are an initial prompt, which must not be resent.
    pub positional_prompt: bool,
    /// Where the hook payload carries the session id.
    pub session_env: Option<&'static str>,
    pub session_keys: &'static [&'static str],
    /// A payload field that marks the hook as coming from a subagent.
    pub subagent_key: Option<&'static str>,
    /// What the hook must print on stdout for the agent to accept it.
    pub hook_stdout: &'static str,
    /// Flags that set the agent's working directory (dropped on resume, which
    /// runs in the recorded directory instead).
    pub cwd_flags: &'static [&'static str],
    /// An argument on the command line of a shared background server that may
    /// run this agent's hooks outside its pane, without the pane's environment.
    /// Hooks fired from there are matched to panes by working directory.
    pub server_arg: Option<&'static str>,
}

pub enum Launch<'a> {
    Resume(&'a str),
    Continue,
    Fresh,
}

pub static AGENTS: &[Agent] = &[
    Agent {
        name: "claude",
        resume_flag: "--resume",
        continue_args: &["--continue"],
        drop: &[
            ("-r", Opt),
            ("--resume", Opt),
            ("-c", No),
            ("--continue", No),
            ("--session-id", One),
            ("--fork-session", No),
            ("-w", Opt),
            ("--worktree", Opt),
            ("--from-pr", Opt),
            ("--teleport", Opt),
        ],
        values: &[
            ("--add-dir", Many),
            ("--agent", One),
            ("--agents", One),
            ("--allowedTools", Many),
            ("--allowed-tools", Many),
            ("--append-system-prompt", One),
            ("--append-system-prompt-file", One),
            ("--autocompact", One),
            ("--betas", Many),
            ("-d", Opt),
            ("--debug", Opt),
            ("--debug-file", One),
            ("--disallowedTools", Many),
            ("--disallowed-tools", Many),
            ("--effort", One),
            ("--environment", One),
            ("--fallback-model", One),
            ("--file", Many),
            ("--input-format", One),
            ("--json-schema", One),
            ("--max-budget-usd", One),
            ("--mcp-config", Many),
            ("--model", One),
            ("-n", One),
            ("--name", One),
            ("--output-format", One),
            ("--permission-mode", One),
            ("--permission-prompts", One),
            ("--plugin-dir", One),
            ("--plugin-url", One),
            ("--prompt-suggestions", Opt),
            ("--remote-control", Opt),
            ("--remote-control-session-name-prefix", One),
            ("--setting-sources", One),
            ("--settings", One),
            ("--system-prompt", One),
            ("--system-prompt-file", One),
            ("--system-prompt-snapshot", One),
            ("--tools", Many),
        ],
        oneshot: &[
            "-p", "--print", "--bg", "--background", "--cloud", "--desktop", "-h", "--help", "-v",
            "--version",
        ],
        subcommands: &[
            "agents", "attach", "auth", "auto-mode", "config", "doctor", "gateway", "import",
            "install", "logs", "mcp", "migrate-installer", "plugin", "purge", "respawn", "rm",
            "setup-token", "stop", "ultrareview", "update",
        ],
        positional_prompt: true,
        session_env: None,
        session_keys: &["session_id"],
        subagent_key: Some("agent_id"),
        hook_stdout: "",
        cwd_flags: &[],
        server_arg: None,
    },
    Agent {
        // GitHub Copilot CLI. Flags are from its docs, not a local install.
        name: "copilot",
        resume_flag: "--resume",
        continue_args: &["--continue"],
        drop: &[
            ("--resume", Opt),
            ("--continue", No),
            ("-i", One),
            ("--interactive", One),
        ],
        values: &[
            ("--model", One),
            ("--add-dir", One),
            ("--agent", One),
            ("--allow-tool", Many),
            ("--deny-tool", Many),
            ("--additional-mcp-config", One),
            ("--config-dir", One),
            ("--log-dir", One),
            ("--log-level", One),
        ],
        oneshot: &["-p", "--prompt", "-h", "--help", "-v", "--version"],
        subcommands: &["help", "login", "logout", "mcp", "plugin", "update", "version"],
        positional_prompt: false,
        session_env: None,
        session_keys: &["sessionId", "session_id"],
        subagent_key: None,
        hook_stdout: "",
        cwd_flags: &[],
        server_arg: None,
    },
    Agent {
        name: "kimi",
        resume_flag: "--session",
        continue_args: &["--continue"],
        drop: &[
            ("-S", Opt),
            ("--session", Opt),
            ("-c", No),
            ("--continue", No),
            // Both refuse to combine with --session/--continue.
            ("--agent", One),
            ("--agent-file", One),
        ],
        values: &[
            ("-m", One),
            ("--model", One),
            ("--skills-dir", One),
            ("--add-dir", One),
            ("--output-format", One),
        ],
        oneshot: &["-p", "--prompt", "-h", "--help", "-V", "--version"],
        subcommands: &[
            "export", "fork", "provider", "session", "acp", "web", "server", "rc", "login",
            "doctor", "vis", "install-desktop", "app", "migrate", "upgrade",
        ],
        positional_prompt: false,
        session_env: None,
        session_keys: &["session_id"],
        subagent_key: None,
        hook_stdout: "",
        cwd_flags: &[],
        server_arg: None,
    },
    Agent {
        // Antigravity CLI.
        name: "agy",
        resume_flag: "--conversation",
        continue_args: &["--continue"],
        drop: &[
            ("--conversation", One),
            ("-c", No),
            ("--continue", No),
            ("-i", One),
            ("--prompt-interactive", One),
            ("--new-project", No),
        ],
        values: &[
            ("--add-dir", One),
            ("--agent", One),
            ("--effort", One),
            ("--input-format", One),
            ("--json-schema", One),
            ("--log-file", One),
            ("--mode", One),
            ("--model", One),
            ("--output-format", One),
            ("--print-timeout", One),
            ("--project", One),
        ],
        oneshot: &["-p", "--print", "--prompt", "-h", "--help"],
        subcommands: &[
            "agent", "agents", "changelog", "help", "install", "mcp", "mic-serve", "models",
            "plugin", "plugins", "remote-control", "update",
        ],
        positional_prompt: false,
        session_env: None,
        session_keys: &["conversationId", "conversation_id"],
        subagent_key: None,
        // Antigravity expects a JSON object back from every hook.
        hook_stdout: "{}\n",
        cwd_flags: &[],
        server_arg: None,
    },
    Agent {
        name: "grok",
        resume_flag: "--resume",
        continue_args: &["--continue"],
        drop: &[
            ("-r", Opt),
            ("--resume", Opt),
            ("-c", No),
            ("--continue", No),
            ("-s", One),
            ("--session-id", One),
            ("--fork-session", No),
            ("-w", Opt),
            ("--worktree", Opt),
            ("--worktree-ref", One),
            ("--cwd", One),
            ("--prompt-file", One),
            ("--prompt-json", One),
        ],
        values: &[
            ("--agent", One),
            ("--agents", One),
            ("--allow", One),
            ("--debug-file", One),
            ("--deny", One),
            ("--disallowed-tools", One),
            ("--json-schema", One),
            ("--leader-socket", One),
            ("-m", One),
            ("--model", One),
            ("--max-turns", One),
            ("--output-format", One),
            ("--permission-mode", One),
            ("--reasoning-effort", One),
            ("--rules", One),
            ("--sandbox", One),
            ("--system-prompt-override", One),
            ("--tools", One),
        ],
        oneshot: &["-p", "--single", "-h", "--help", "-V", "--version"],
        subcommands: &[
            "agent", "clone", "completions", "cursor-worker", "dashboard", "doctor", "du",
            "export", "help", "inspect", "leader", "login", "logout", "mcp", "memory", "models",
            "plugin", "sessions", "setup", "trace", "update", "usage", "version", "worktree",
            "wrap",
        ],
        positional_prompt: true,
        session_env: Some("GROK_SESSION_ID"),
        session_keys: &["session_id", "sessionId"],
        subagent_key: None,
        hook_stdout: "",
        cwd_flags: &[],
        server_arg: None,
    },
    Agent {
        // OpenAI Codex CLI. Flags are from its source and `--help` (0.166.0).
        // Resuming is a subcommand (`codex [flags] resume <id>`),
        // and root flags still apply to it.
        name: "codex",
        resume_flag: "resume",
        continue_args: &["resume", "--last"],
        drop: &[
            ("-i", Many),
            ("--image", Many),
            ("--worktree", No),
            ("-C", One),
            ("--cd", One),
            // `codex resume`'s own flags, if the pane was started with it.
            ("--last", No),
            ("--all", No),
            ("--include-non-interactive", No),
        ],
        values: &[
            ("-m", One),
            ("--model", One),
            ("--local-provider", One),
            ("-p", One),
            ("--profile", One),
            ("-s", One),
            ("--sandbox", One),
            ("-a", One),
            ("--ask-for-approval", One),
            ("-c", One),
            ("--config", One),
            ("--enable", One),
            ("--disable", One),
            ("--remote", One),
            ("--remote-auth-token-env", One),
            ("--add-dir", One),
        ],
        oneshot: &["-h", "--help", "-V", "--version"],
        // Not `resume` or `fork`: those are interactive sessions to track.
        subcommands: &[
            "agents", "tcp-tunnel", "exec", "e", "review", "login", "logout", "mcp", "plugin",
            "app-server", "remote-control", "app", "completion", "doctor", "sandbox", "debug",
            "execpolicy", "apply", "a", "queue", "archive", "delete", "migrate-rollouts",
            "migrate-nfs-db", "unarchive", "cloud", "cloud-tasks", "responses-api-proxy",
            "stdio-to-uds", "exec-server", "features", "help", "update",
        ],
        // Positionals are the prompt, or `resume`/`fork` and their target.
        positional_prompt: true,
        session_env: None,
        session_keys: &["session_id"],
        // Subagents fire SubagentStart, never SessionStart.
        subagent_key: None,
        hook_stdout: "",
        cwd_flags: &["-C", "--cd"],
        // By default codex runs sessions, and their hooks, in a shared
        // `codex app-server` daemon.
        server_arg: Some("app-server"),
    },
];

pub fn find(name: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.name == name)
}

fn flag_name(arg: &str) -> &str {
    arg.split_once('=').map_or(arg, |(f, _)| f)
}

impl Agent {
    fn arity(&self, flag: &str) -> Val {
        self.drop
            .iter()
            .chain(self.values)
            .find(|(f, _)| *f == flag)
            .map_or(No, |(_, v)| *v)
    }

    /// How many of `rest` the flag `arg` consumes.
    fn consumed(&self, arg: &str, rest: &[String]) -> usize {
        if arg.starts_with("--") && arg.contains('=') {
            return 0;
        }
        let is_value = |s: &String| !s.starts_with('-');
        match self.arity(arg) {
            No => 0,
            One => rest.len().min(1),
            Opt => usize::from(rest.first().is_some_and(is_value)),
            Many => rest.iter().take_while(|s| is_value(s)).count(),
        }
    }

    /// True for invocations that aren't an interactive session (print mode,
    /// `--help`, subcommands). za runs those untouched.
    pub fn is_passthrough(&self, args: &[String]) -> bool {
        let mut i = 0;
        while i < args.len() {
            let arg = args[i].as_str();
            if arg == "--" {
                return false;
            }
            if arg.starts_with('-') {
                if self.oneshot.contains(&flag_name(arg)) {
                    return true;
                }
                i += 1 + self.consumed(arg, &args[i + 1..]);
                continue;
            }
            return self.subcommands.contains(&arg);
        }
        false
    }

    /// The original args with anything session-selecting removed, plus the
    /// flags for `launch`.
    pub fn launch_args(&self, args: &[String], launch: Launch) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--" {
                if !self.positional_prompt {
                    out.extend_from_slice(&args[i..]);
                }
                break;
            }
            if arg.starts_with('-') {
                let n = self.consumed(arg, &args[i + 1..]);
                let flag = flag_name(arg);
                if !self.drop.iter().any(|(f, _)| *f == flag) {
                    out.extend_from_slice(&args[i..=i + n]);
                }
                i += 1 + n;
                continue;
            }
            if !self.positional_prompt {
                out.push(arg.clone());
            }
            i += 1;
        }
        match launch {
            Launch::Resume(id) => {
                out.push(self.resume_flag.to_string());
                out.push(id.to_string());
            }
            Launch::Continue => out.extend(self.continue_args.iter().map(|s| s.to_string())),
            Launch::Fresh => {}
        }
        out
    }

    /// The directory the agent will work in: `here`, unless a cwd flag
    /// (`codex -C dir`) points elsewhere.
    pub fn effective_cwd(&self, args: &[String], here: &str) -> String {
        let mut dir = None;
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--" {
                break;
            }
            if arg.starts_with('-') {
                let n = self.consumed(arg, &args[i + 1..]);
                if self.cwd_flags.contains(&flag_name(arg)) {
                    dir = match arg.split_once('=') {
                        Some((_, v)) if arg.starts_with("--") => Some(v),
                        _ if n == 1 => Some(args[i + 1].as_str()),
                        _ => dir,
                    };
                }
                i += 1 + n;
                continue;
            }
            i += 1;
        }
        match dir {
            Some(d) => Path::new(here).join(d).to_string_lossy().into_owned(),
            None => here.to_string(),
        }
    }

    pub fn session_id(&self, payload: &Value) -> Option<String> {
        let from_env = self
            .session_env
            .and_then(|var| std::env::var(var).ok())
            .filter(|v| !v.is_empty());
        from_env
            .or_else(|| {
                self.session_keys
                    .iter()
                    .find_map(|k| payload.get(k)?.as_str().filter(|s| !s.is_empty()))
                    .map(String::from)
            })
            .filter(|id| id.len() <= 256 && !id.starts_with('-'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_resume_keeps_flags_and_drops_prompt() {
        let claude = find("claude").unwrap();
        let args = v(&["--model", "opus", "--dangerously-skip-permissions", "fix the bug"]);
        assert_eq!(
            claude.launch_args(&args, Launch::Resume("abc")),
            v(&["--model", "opus", "--dangerously-skip-permissions", "--resume", "abc"])
        );
    }

    #[test]
    fn claude_resume_replaces_old_session_selection() {
        let claude = find("claude").unwrap();
        let args = v(&["--resume", "old", "-w", "feat", "--continue"]);
        assert_eq!(claude.launch_args(&args, Launch::Resume("new")), v(&["--resume", "new"]));
        let args = v(&["-r", "--model=sonnet"]);
        assert_eq!(
            claude.launch_args(&args, Launch::Continue),
            v(&["--model=sonnet", "--continue"])
        );
    }

    #[test]
    fn variadic_flags_swallow_following_values() {
        let claude = find("claude").unwrap();
        let args = v(&["--add-dir", "../a", "../b", "--effort", "high"]);
        assert_eq!(
            claude.launch_args(&args, Launch::Fresh),
            v(&["--add-dir", "../a", "../b", "--effort", "high"])
        );
    }

    #[test]
    fn kimi_drops_agent_profile_on_resume() {
        let kimi = find("kimi").unwrap();
        let args = v(&["--agent", "coder", "-m", "k2", "--yolo"]);
        assert_eq!(
            kimi.launch_args(&args, Launch::Resume("s1")),
            v(&["-m", "k2", "--yolo", "--session", "s1"])
        );
    }

    #[test]
    fn passthrough_detection() {
        let claude = find("claude").unwrap();
        assert!(claude.is_passthrough(&v(&["-p", "hi"])));
        assert!(claude.is_passthrough(&v(&["--model", "opus", "mcp", "list"])));
        assert!(claude.is_passthrough(&v(&["--version"])));
        assert!(!claude.is_passthrough(&v(&["--model", "mcp"])));
        assert!(!claude.is_passthrough(&v(&["fix", "the", "bug"])));
        assert!(!claude.is_passthrough(&v(&[])));
        let agy = find("agy").unwrap();
        assert!(!agy.is_passthrough(&v(&["--dangerously-skip-permissions"])));
        assert!(agy.is_passthrough(&v(&["models"])));
    }

    #[test]
    fn codex_resumes_with_a_subcommand_after_its_flags() {
        let codex = find("codex").unwrap();
        let args = v(&["-m", "gpt-5.5", "--yolo", "-i", "a.png", "b.png", "fix the bug"]);
        assert_eq!(
            codex.launch_args(&args, Launch::Resume("t-1")),
            v(&["-m", "gpt-5.5", "--yolo", "resume", "t-1"])
        );
        assert_eq!(
            codex.launch_args(&v(&["-C", "../x", "-s", "read-only"]), Launch::Continue),
            v(&["-s", "read-only", "resume", "--last"])
        );
        // A pane started with `codex resume ...` resumes whatever it ended up in.
        let args = v(&["resume", "--last", "--model", "o3"]);
        assert_eq!(
            codex.launch_args(&args, Launch::Resume("t-2")),
            v(&["--model", "o3", "resume", "t-2"])
        );
    }

    #[test]
    fn codex_passthrough_and_cwd() {
        let codex = find("codex").unwrap();
        assert!(codex.is_passthrough(&v(&["exec", "hi"])));
        assert!(codex.is_passthrough(&v(&["-m", "o3", "e", "hi"])));
        assert!(!codex.is_passthrough(&v(&["resume", "abc"])));
        assert!(!codex.is_passthrough(&v(&["-p", "work", "fork"])));
        assert_eq!(codex.effective_cwd(&v(&["-C", "sub", "hi"]), "/r"), "/r/sub");
        assert_eq!(codex.effective_cwd(&v(&["--cd=/abs"]), "/r"), "/abs");
        assert_eq!(codex.effective_cwd(&v(&["-m", "-C"]), "/r"), "/r");
        assert_eq!(find("claude").unwrap().effective_cwd(&v(&["-C", "x"]), "/r"), "/r");
    }

    #[test]
    fn session_id_from_payload() {
        let copilot = find("copilot").unwrap();
        let payload = serde_json::json!({"sessionId": "c-1", "cwd": "/x"});
        assert_eq!(copilot.session_id(&payload).as_deref(), Some("c-1"));
        let agy = find("agy").unwrap();
        let payload = serde_json::json!({"conversationId": ""});
        assert_eq!(agy.session_id(&payload), None);
    }
}
