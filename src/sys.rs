//! What a participant asks of the computer it runs on, the same way on Linux and macOS.

use std::path::PathBuf;

/// The directory a session's sockets and other runtime files live in: `$XDG_RUNTIME_DIR`, or
/// on macOS, which has none, the per-user `$TMPDIR`.
pub fn runtime_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    var("XDG_RUNTIME_DIR").or_else(|| if cfg!(target_os = "macos") { var("TMPDIR") } else { None })
}

/// This machine's name, as the operating system has it.
pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length, and gethostname writes at most that many bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
    // A Mac on a network often reports `name.local`; the name is the first label.
    let name = name.strip_suffix(".local").unwrap_or(&name).to_string();
    (!name.is_empty() && name != "localhost").then_some(name)
}

/// The executable a process runs, as the kernel names it.
pub fn executable(pid: i32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(target_os = "macos")]
    {
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer is valid for its length, which is what proc_pidpath is told.
        let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        if len <= 0 {
            return None;
        }
        buf.truncate(len as usize);
        Some(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// Open `target`, a URL or a file, the way the person's desktop opens things.
pub fn open(target: &str) -> std::io::Result<()> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(target).spawn().map(drop)
}
