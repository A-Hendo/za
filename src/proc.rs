//! Minimal process-tree inspection.

use std::path::PathBuf;

pub struct Info {
    pub ppid: i32,
    pub exe: Option<PathBuf>,
}

#[cfg(target_os = "linux")]
pub fn info(pid: i32) -> Option<Info> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // "pid (comm) state ppid ..."; comm may itself contain spaces or parens.
    let after_comm = stat.get(stat.rfind(')')? + 2..)?;
    let ppid = after_comm.split_whitespace().nth(1)?.parse().ok()?;
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    Some(Info { ppid, exe })
}

#[cfg(not(target_os = "linux"))]
pub fn info(pid: i32) -> Option<Info> {
    let out = std::process::Command::new("ps")
        .args(["-o", "ppid=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (ppid, comm) = text.trim().split_once(char::is_whitespace)?;
    Some(Info {
        ppid: ppid.trim().parse().ok()?,
        exe: Some(PathBuf::from(comm.trim())),
    })
}

/// Whether the hook running in this process was fired by the agent that the
/// za wrapper `wrapper` launched, and not by another agent nested inside it
/// (e.g. `claude -p` run from one of claude's tools, which inherits ZA_SLOT).
pub fn hook_belongs_to(wrapper: i32) -> bool {
    let mut seen = Vec::new();
    let mut pid = std::process::id() as i32;
    for depth in 0..64 {
        let Some(info) = info(pid) else {
            // No way to inspect processes at all: trust the environment.
            return depth == 0;
        };
        if info.ppid == wrapper {
            // `pid` is the agent the wrapper spawned. A second copy of the same
            // executable between it and us means a nested agent fired this hook.
            return info.exe.is_none() || !seen.contains(&info.exe);
        }
        if info.ppid <= 1 {
            return false;
        }
        seen.push(info.exe);
        pid = info.ppid;
    }
    false
}

/// While a child owns the terminal, Ctrl-C and Ctrl-\ reach our whole process
/// group. The child decides what they mean; za must just stay alive to wait.
pub fn ignore_interrupts(ignore: bool) {
    let action = if ignore { libc::SIG_IGN } else { libc::SIG_DFL };
    // SAFETY: installing SIG_IGN/SIG_DFL is always sound.
    unsafe {
        libc::signal(libc::SIGINT, action);
        libc::signal(libc::SIGQUIT, action);
    }
}

pub fn exit_code(status: std::io::Result<std::process::ExitStatus>) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(s) => s.code().or_else(|| s.signal().map(|n| 128 + n)).unwrap_or(1),
        Err(_) => 1,
    }
}

/// True when this process was started directly by Zellij (a resurrected
/// command pane) rather than from a shell.
pub fn parent_is_zellij() -> bool {
    // SAFETY: getppid has no preconditions.
    let ppid = unsafe { libc::getppid() };
    info(ppid)
        .and_then(|i| i.exe)
        .and_then(|exe| exe.file_name().map(|n| n.to_string_lossy().starts_with("zellij")))
        .unwrap_or(false)
}
