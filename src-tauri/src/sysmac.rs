//! Small macOS system queries. None of these need extra privacy permissions:
//! idle time and the process list are public, and Shortcuts is run through
//! its own command-line tool.

pub const FOCUS_ON_SHORTCUT: &str = "Chronos Focus On";
pub const FOCUS_OFF_SHORTCUT: &str = "Chronos Focus Off";

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Command, Stdio};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
    }
    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT: u32 = u32::MAX;

    /// Milliseconds since the last keyboard, mouse or trackpad input.
    pub fn idle_ms() -> i64 {
        let secs = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) };
        if secs.is_finite() && secs > 0.0 { (secs * 1000.0) as i64 } else { 0 }
    }

    /// Names of all running processes (as `ps -c` shows them).
    pub fn process_names() -> Vec<String> {
        let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if n <= 0 {
            return Vec::new();
        }
        let mut pids = vec![0 as libc::c_int; n as usize + 64];
        let bytes = (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int;
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) }.max(0) as usize;
        let mut names = Vec::with_capacity(count);
        let mut buf = [0u8; 256];
        for &pid in pids.iter().take(count).filter(|&&p| p > 0) {
            let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
            if len > 0 {
                names.push(String::from_utf8_lossy(&buf[..len as usize]).into_owned());
            }
        }
        names
    }

    pub fn shortcut_names() -> Vec<String> {
        Command::new("/usr/bin/shortcuts")
            .arg("list")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect())
            .unwrap_or_default()
    }

    /// Fire-and-forget; a slow shortcut must never block the timer.
    pub fn run_shortcut(name: &str) {
        let child = Command::new("/usr/bin/shortcuts")
            .args(["run", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    pub fn run_shortcut_blocking(name: &str) {
        let _ = Command::new("/usr/bin/shortcuts").args(["run", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }

    /// Sign the bundled Do Not Disturb shortcuts with this Mac's Shortcuts and
    /// open them, which shows macOS's own "Add Shortcut" dialog for each.
    pub fn install_focus_shortcuts() -> Result<(), String> {
        let dir = std::env::temp_dir().join("chronos-shortcuts");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let files: [(&str, &[u8]); 2] = [
            (super::FOCUS_ON_SHORTCUT, include_bytes!("../resources/focus-on.unsigned.shortcut")),
            (super::FOCUS_OFF_SHORTCUT, include_bytes!("../resources/focus-off.unsigned.shortcut")),
        ];
        for (name, bytes) in files {
            let unsigned = dir.join(format!("{name}.unsigned.shortcut"));
            let signed = dir.join(format!("{name}.shortcut"));
            std::fs::write(&unsigned, bytes).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&signed);
            let ok = Command::new("/usr/bin/shortcuts")
                .args(["sign", "-m", "anyone", "-i"])
                .arg(&unsigned)
                .arg("-o")
                .arg(&signed)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok || !signed.exists() {
                return Err("macOS couldn't sign the shortcut. Create it by hand in Shortcuts instead.".into());
            }
            Command::new("/usr/bin/open").arg(&signed).status().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn idle_ms() -> i64 {
        0
    }
    pub fn process_names() -> Vec<String> {
        Vec::new()
    }
    pub fn shortcut_names() -> Vec<String> {
        Vec::new()
    }
    pub fn run_shortcut(_: &str) {}
    pub fn run_shortcut_blocking(_: &str) {}
    pub fn install_focus_shortcuts() -> Result<(), String> {
        Err("Only available on macOS".into())
    }
}

pub use imp::*;

/// Which of `wanted` are running, matched case-insensitively.
pub fn running_matches(wanted: &[String], running: &[String]) -> Vec<String> {
    wanted.iter().filter(|w| running.iter().any(|r| r.eq_ignore_ascii_case(w))).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_are_case_insensitive() {
        let running = vec!["Finder".to_string(), "CptHost".to_string(), "obs".to_string()];
        let wanted = vec!["cpthost".to_string(), "Keynote".to_string(), "OBS".to_string()];
        assert_eq!(running_matches(&wanted, &running), vec!["cpthost".to_string(), "OBS".to_string()]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_queries_work_without_permissions() {
        assert!(idle_ms() >= 0);
        let names = process_names();
        assert!(names.len() > 10, "expected a real process list, got {}", names.len());
        assert!(names.iter().any(|n| n == "launchd" || n == "WindowServer" || n == "Finder"));
    }
}
