//! Windows install step: hand the downloaded NSIS setup to a hidden PowerShell that waits for
//! every wai process to exit, runs it silently and relaunches wai.
//! Only the installer-managed copy can update.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

/// Starts the installer to run once every wai process has exited. No window; the child outlives wai (children
/// are not killed with the parent) and holds none of its handles.
pub fn install_after_exit(setup: &Path) -> std::io::Result<()> {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let powershell = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &super::installer_script(setup),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
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
