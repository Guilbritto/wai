//! Windows install step: hand the downloaded NSIS setup to a detached PowerShell that waits for
//! wai to exit, runs it silently and relaunches wai. Only the installer-managed copy can update.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Why this copy can't update itself, or `None` when the installer put it here (it writes
/// `uninstall.exe` next to `wai.exe`; a portable or dev build has none).
pub fn blocker(exe: &Path) -> Option<String> {
    let installed = exe
        .parent()
        .is_some_and(|dir| dir.join("uninstall.exe").exists());
    (!installed).then(|| {
        "This copy of wai was not installed by its installer, so it can't update itself. \
         Install wai from the release page and updates will install on their own."
            .to_string()
    })
}

/// Starts the installer to run once process `pid` exits. Detached with no console so it outlives
/// wai and shows no window.
pub fn install_after_exit(setup: &Path, pid: u32) -> std::io::Result<()> {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &super::installer_script(setup, pid),
        ])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_blocker() {
        let dir = std::env::temp_dir().join(format!("wai-blocker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("wai.exe");
        std::fs::write(&exe, b"").unwrap();
        assert!(blocker(&exe).is_some());
        std::fs::write(dir.join("uninstall.exe"), b"").unwrap();
        assert!(blocker(&exe).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
