//! What za knows about each agent CLI: how to resume a session, which flags
//! take values, and which invocations aren't interactive sessions at all.
//!
//! On resume the original command line is replayed minus anything that would
//! start a different session (a prompt, `--continue`, `--worktree`, ...).

use serde_json::Value;

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
    pub resume_flag: &'static str,
    pub continue_flag: &'static str,
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
        continue_flag: "--continue",
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
    },
    Agent {
        // GitHub Copilot CLI. Flags are from its docs, not a local install.
        name: "copilot",
        resume_flag: "--resume",
        continue_flag: "--continue",
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
    },
    Agent {
        name: "kimi",
        resume_flag: "--session",
        continue_flag: "--continue",
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
    },
    Agent {
        // Antigravity CLI.
        name: "agy",
        resume_flag: "--conversation",
        continue_flag: "--continue",
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
    },
    Agent {
        name: "grok",
        resume_flag: "--resume",
        continue_flag: "--continue",
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
            Launch::Continue => out.push(self.continue_flag.to_string()),
            Launch::Fresh => {}
        }
        out
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
    fn session_id_from_payload() {
        let copilot = find("copilot").unwrap();
        let payload = serde_json::json!({"sessionId": "c-1", "cwd": "/x"});
        assert_eq!(copilot.session_id(&payload).as_deref(), Some("c-1"));
        let agy = find("agy").unwrap();
        let payload = serde_json::json!({"conversationId": ""});
        assert_eq!(agy.session_id(&payload), None);
    }
}
