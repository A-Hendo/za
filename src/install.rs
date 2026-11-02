//! `za install` / `za uninstall`: register `za hook <agent>` with each
//! agent's session-start hook, and `za init`: shell functions that route
//! agent commands through `za run` inside Zellij.

use crate::agents::AGENTS;
use crate::{config, state};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

struct Integration {
    agent: &'static str,
    /// Present when the agent is installed for this user.
    home: fn() -> PathBuf,
    install: fn(&str) -> io::Result<PathBuf>,
    uninstall: fn() -> io::Result<Option<PathBuf>>,
}

const INTEGRATIONS: &[Integration] = &[
    Integration {
        agent: "claude",
        home: claude_home,
        install: claude_install,
        uninstall: claude_uninstall,
    },
    Integration {
        agent: "copilot",
        home: copilot_home,
        install: copilot_install,
        uninstall: copilot_uninstall,
    },
    Integration {
        agent: "kimi",
        home: kimi_home,
        install: kimi_install,
        uninstall: kimi_uninstall,
    },
    Integration {
        agent: "agy",
        home: agy_home,
        install: agy_install,
        uninstall: agy_uninstall,
    },
    Integration {
        agent: "grok",
        home: grok_home,
        install: grok_install,
        uninstall: grok_uninstall,
    },
];

/// Marks the kimi TOML block za owns.
const KIMI_MARKER: &str = "# za: records this session so Zellij restores can resume it";

fn selected(which: &str) -> Result<Vec<&'static Integration>, String> {
    if which == "all" {
        return Ok(INTEGRATIONS.iter().collect());
    }
    INTEGRATIONS
        .iter()
        .find(|i| i.agent == which)
        .map(|i| vec![i])
        .ok_or_else(|| format!("za: unknown agent '{which}' (expected one of: {}, all)", names()))
}

fn names() -> String {
    AGENTS.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
}

pub fn install(which: &str) -> i32 {
    let targets = match selected(which) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let za = match za_command_prefix() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("za: can't locate own binary: {e}");
            return 1;
        }
    };
    let mut failed = false;
    for i in targets {
        if which == "all" && !(i.home)().is_dir() {
            println!("  {:<8} skipped (not installed: no {})", i.agent, (i.home)().display());
            continue;
        }
        match (i.install)(&format!("{za} hook {}", i.agent)) {
            Ok(path) => println!("  {:<8} hook installed in {}", i.agent, path.display()),
            Err(e) => {
                failed = true;
                eprintln!("  {:<8} failed: {e}", i.agent);
            }
        }
    }
    let cfg = config::path();
    if !cfg.exists() {
        if let Err(e) = fs::create_dir_all(state::dir()).and_then(|_| fs::write(&cfg, config::DEFAULT_TOML)) {
            eprintln!("za: couldn't write {}: {e}", cfg.display());
        } else {
            println!("  config   written to {}", cfg.display());
        }
    }
    i32::from(failed)
}

pub fn uninstall(which: &str) -> i32 {
    let targets = match selected(which) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let mut failed = false;
    for i in targets {
        match (i.uninstall)() {
            Ok(Some(path)) => println!("  {:<8} hook removed from {}", i.agent, path.display()),
            Ok(None) => {}
            Err(e) => {
                failed = true;
                eprintln!("  {:<8} failed: {e}", i.agent);
            }
        }
    }
    i32::from(failed)
}

pub fn init(shell: &str) -> i32 {
    let za = match za_command_prefix() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("za: can't locate own binary: {e}");
            return 1;
        }
    };
    let names: Vec<&str> = AGENTS.iter().map(|a| a.name).collect();
    match shell {
        // `function name { }` rather than `name() { }`, so an existing alias
        // of the same name (alias agy="agy --flag") keeps working on top.
        "zsh" | "bash" => {
            println!("# za: inside Zellij, run agents through za so their sessions resume");
            println!("if [ -n \"${{ZELLIJ:-}}\" ]; then");
            for n in names {
                println!("  function {n} {{ command {za} run {n} \"$@\"; }}");
            }
            println!("fi");
        }
        "fish" => {
            println!("# za: inside Zellij, run agents through za so their sessions resume");
            println!("if set -q ZELLIJ");
            for n in names {
                println!("    function {n} --wraps {n}; command {za} run {n} $argv; end");
            }
            println!("end");
        }
        _ => {
            eprintln!("usage: za init <zsh|bash|fish>");
            return 2;
        }
    }
    0
}

/// This binary's path, single-quoted for sh.
fn za_command_prefix() -> io::Result<String> {
    let exe = std::env::current_exe()?;
    Ok(sh_quote(&exe.to_string_lossy()))
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn is_ours(command: &str) -> bool {
    command.contains("za' hook ") || command.starts_with("za hook ")
}

/// Rewrite a JSON config in place, keeping key order, with a backup alongside.
fn edit_json(path: &Path, edit: impl FnOnce(&mut Map<String, Value>)) -> io::Result<bool> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut root: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?
    };
    let obj = root.as_object_mut().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, format!("{}: not a JSON object", path.display()))
    })?;
    let before = obj.clone();
    edit(obj);
    if *obj == before {
        return Ok(false);
    }
    let mut out = serde_json::to_string_pretty(&root)?;
    out.push('\n');
    write_with_backup(path, &out)?;
    Ok(true)
}

fn write_with_backup(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".za-backup");
        fs::copy(path, PathBuf::from(backup))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".za-tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)
}

/// Remove `za hook` entries from a Claude-style `{"Event": [{"hooks": [...]}]}`
/// hook list, dropping groups and events left empty.
fn strip_claude_style(hooks: &mut Map<String, Value>, event: &str) {
    let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
        return;
    };
    groups.retain_mut(|group| {
        let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let had = list.len();
        list.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(is_ours));
        !(list.is_empty() && had > 0)
    });
    if groups.is_empty() {
        hooks.remove(event);
    }
}

fn claude_style_entry(command: &str) -> Value {
    json!({ "hooks": [{ "type": "command", "command": command, "timeout": 10 }] })
}

// claude: ~/.claude/settings.json, hooks.SessionStart

fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| state::home().join(".claude"))
}

fn claude_install(command: &str) -> io::Result<PathBuf> {
    let path = claude_home().join("settings.json");
    edit_json(&path, |root| {
        let hooks = root.entry("hooks").or_insert_with(|| json!({}));
        if let Some(hooks) = hooks.as_object_mut() {
            strip_claude_style(hooks, "SessionStart");
            let list = hooks.entry("SessionStart").or_insert_with(|| json!([]));
            if let Some(list) = list.as_array_mut() {
                list.push(claude_style_entry(command));
            }
        }
    })?;
    Ok(path)
}

fn claude_uninstall() -> io::Result<Option<PathBuf>> {
    let path = claude_home().join("settings.json");
    if !path.exists() {
        return Ok(None);
    }
    let changed = edit_json(&path, |root| {
        if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
            strip_claude_style(hooks, "SessionStart");
        }
    })?;
    Ok(changed.then_some(path))
}

// copilot: ~/.copilot/hooks/za.json (user-level hook files, Copilot CLI format)

fn copilot_home() -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| state::home().join(".copilot"))
}

fn copilot_install(command: &str) -> io::Result<PathBuf> {
    let path = copilot_home().join("hooks/za.json");
    let body = json!({
        "version": 1,
        "hooks": {
            "sessionStart": [{ "type": "command", "bash": command, "timeoutSec": 10 }]
        }
    });
    write_with_backup(&path, &(serde_json::to_string_pretty(&body)? + "\n"))?;
    Ok(path)
}

fn copilot_uninstall() -> io::Result<Option<PathBuf>> {
    remove_file_if_present(copilot_home().join("hooks/za.json"))
}

// kimi: ~/.kimi-code/config.toml, a [[hooks]] block (edited as text to keep comments)

fn kimi_home() -> PathBuf {
    state::home().join(".kimi-code")
}

fn kimi_install(command: &str) -> io::Result<PathBuf> {
    let path = kimi_home().join("config.toml");
    let mut text = strip_kimi_block(&fs::read_to_string(&path).unwrap_or_default());
    if !text.is_empty() && !text.ends_with("\n\n") {
        text.push_str(if text.ends_with('\n') { "\n" } else { "\n\n" });
    }
    let command = toml::Value::String(command.to_string());
    text.push_str(&format!(
        "{KIMI_MARKER}\n[[hooks]]\nevent = \"SessionStart\"\ncommand = {command}\ntimeout = 10\n"
    ));
    write_with_backup(&path, &text)?;
    Ok(path)
}

fn kimi_uninstall() -> io::Result<Option<PathBuf>> {
    let path = kimi_home().join("config.toml");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    let stripped = strip_kimi_block(&text);
    if stripped == text {
        return Ok(None);
    }
    write_with_backup(&path, &stripped)?;
    Ok(Some(path))
}

/// Drop the marker line and the four-line block za wrote after it.
fn strip_kimi_block(text: &str) -> String {
    let mut out = Vec::new();
    let mut skip = 0;
    for line in text.lines() {
        if line == KIMI_MARKER {
            skip = 4;
            continue;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        out.push(line);
    }
    let mut s = out.join("\n").trim_end().to_string();
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

// agy (Antigravity): ~/.gemini/config/hooks.json, a "za" group of PreInvocation hooks

fn agy_home() -> PathBuf {
    state::home().join(".gemini")
}

fn agy_install(command: &str) -> io::Result<PathBuf> {
    let path = agy_home().join("config/hooks.json");
    edit_json(&path, |root| {
        root.insert(
            "za".into(),
            json!({ "PreInvocation": [{ "command": command, "timeout": 10, "type": "command" }] }),
        );
    })?;
    Ok(path)
}

fn agy_uninstall() -> io::Result<Option<PathBuf>> {
    let path = agy_home().join("config/hooks.json");
    if !path.exists() {
        return Ok(None);
    }
    let changed = edit_json(&path, |root| {
        root.remove("za");
    })?;
    Ok(changed.then_some(path))
}

// grok: ~/.grok/hooks/za.json (Claude-style hook file)

fn grok_home() -> PathBuf {
    state::home().join(".grok")
}

fn grok_install(command: &str) -> io::Result<PathBuf> {
    let path = grok_home().join("hooks/za.json");
    let body = json!({ "hooks": { "SessionStart": [claude_style_entry(command)] } });
    write_with_backup(&path, &(serde_json::to_string_pretty(&body)? + "\n"))?;
    Ok(path)
}

fn grok_uninstall() -> io::Result<Option<PathBuf>> {
    remove_file_if_present(grok_home().join("hooks/za.json"))
}

fn remove_file_if_present(path: PathBuf) -> io::Result<Option<PathBuf>> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(Some(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_style_strip_keeps_other_hooks() {
        let mut hooks = json!({
            "SessionStart": [
                { "matcher": "*", "hooks": [{ "type": "command", "command": "other.sh" }] },
                claude_style_entry("'/x/za' hook claude"),
            ]
        });
        let map = hooks.as_object_mut().unwrap();
        strip_claude_style(map, "SessionStart");
        assert_eq!(map["SessionStart"].as_array().unwrap().len(), 1);
        strip_claude_style(map, "SessionStart");
        assert_eq!(map["SessionStart"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn kimi_block_round_trips() {
        let original = "default_model = \"k2\"\n\n[[hooks]]\nevent = \"Stop\"\ncommand = \"x\"\n";
        let with = format!(
            "{original}\n{KIMI_MARKER}\n[[hooks]]\nevent = \"SessionStart\"\ncommand = \"'/za' hook kimi\"\ntimeout = 10\n"
        );
        assert_eq!(strip_kimi_block(&with), original);
        assert_eq!(strip_kimi_block(original), original);
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/a b/za"), "'/a b/za'");
        assert_eq!(sh_quote("/it's/za"), r"'/it'\''s/za'");
        assert!(is_ours(&format!("{} hook claude", sh_quote("/home/u/.cargo/bin/za"))));
        assert!(!is_ours("bash '/home/u/.claude/hooks/herdr-agent-state.sh' session"));
    }
}
