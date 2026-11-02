//! Optional settings in `~/.config/za/config.toml`.

use crate::state;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Start every pane's command straight away when resurrecting, instead of
    /// Zellij's "press ENTER to run".
    pub run_commands_on_restore: bool,
    /// When a resumed agent exits, drop into a shell in that pane (as if the
    /// agent had been started from one) instead of leaving a dead pane.
    pub shell_after_exit: bool,
    /// If a pane has no recorded session id (hook not installed), resume the
    /// agent's most recent session in that directory.
    pub continue_fallback: bool,
    /// How often the launcher checks which Zellij sessions are running.
    pub poll_interval_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            run_commands_on_restore: true,
            shell_after_exit: true,
            continue_fallback: true,
            poll_interval_secs: 2,
        }
    }
}

pub const DEFAULT_TOML: &str = r#"# za settings. Delete a line to fall back to its default.

# Start every pane's command straight away when resurrecting a session,
# instead of Zellij's "press ENTER to run".
run_commands_on_restore = true

# When a resumed agent exits, drop into a shell in that pane rather than
# leaving a dead command pane behind.
shell_after_exit = true

# If a pane has no recorded session id (its agent has no za hook installed),
# resume that agent's most recent session in the pane's directory.
continue_fallback = true

# How often (seconds) the launcher checks which Zellij sessions are running.
poll_interval_secs = 2
"#;

pub fn path() -> std::path::PathBuf {
    state::dir().join("config.toml")
}

pub fn try_load() -> Result<Config, String> {
    match fs::read_to_string(path()) {
        Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
        Err(_) => Ok(Config::default()),
    }
}

/// Settings with defaults for anything missing or unreadable. Hooks must
/// never fail on a bad config, so errors are only surfaced by `za status`.
pub fn load() -> Config {
    try_load().unwrap_or_default()
}
