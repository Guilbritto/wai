mod agents;
mod agents_view;
mod app;
mod aurora;
mod chat;
mod claude_config;
mod diagnostics;
mod editor;
mod elsewhere;
mod ext_host;
mod ext_panel;
mod extensions_view;
mod files;
mod git;
mod instance;
mod layout;
mod log;
mod notify;
mod palette;
mod search_view;
mod settings_view;
#[cfg(target_os = "macos")]
mod shell_path;
mod sidebar;
mod start;
mod terminal_view;
mod terminals;
mod topbar;
mod update;
#[cfg(all(test, unix))]
mod waycarbon;
mod workspace;

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use wai_settings::{AppState, Settings, config_dir, load_templates};

fn main() -> anyhow::Result<()> {
    // Very first: it removes an environment variable, sound only while no other thread exists, and
    // may hand over to the copy moved out of /Applications.
    #[cfg(target_os = "macos")]
    let update_notice = match update::macos::on_start() {
        update::macos::Start::Exit => return Ok(()),
        update::macos::Start::Continue(notice) => notice,
    };
    #[cfg(not(target_os = "macos"))]
    let update_notice = None;
    // Then: it may set PATH, which is sound only while no other thread exists.
    #[cfg(target_os = "macos")]
    shell_path::import();
    // `WAI_STATE_PATH=<file>` keeps a manual run away from the real state.
    let state_path = std::env::var_os("WAI_STATE_PATH").map_or_else(
        || Ok(config_dir().join("state.json")),
        |p| std::path::absolute(PathBuf::from(p)),
    )?;
    // A first launch has no folder yet, and the lock lives in it.
    if let Some(dir) = state_path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    // One wai per state folder; the guard lives to the end of `main`.
    let mut unlocked = None;
    let _instance =
        match instance::acquire(&state_path.with_file_name("wai.lock"), instance::LOCK_WAIT) {
            instance::Acquired::Held(guard) => Some(guard),
            instance::Acquired::Taken => {
                let arg = std::env::args_os().nth(1).map(PathBuf::from);
                instance::tell_taken(arg.as_deref().unwrap_or(Path::new("wai")));
                return Ok(());
            }
            instance::Acquired::Unusable(e) => {
                unlocked = Some(e);
                None
            }
        };
    // After the lock, so a refused second launch cannot rotate the first one's log.
    if let Some(dir) = state_path.parent() {
        log::init(dir);
    }
    if let Some(e) = unlocked {
        eprintln!("wai: not locking the state folder: {e}");
    }
    let state = AppState::load(&state_path);
    let project = resolve_project(std::env::args_os().nth(1).map(PathBuf::from), state.project)?;
    // Everything relative is resolved by now; agents spawn from a neutral dir, not a repo.
    wai_agent::enter_neutral_dir(&config_dir()).context("cannot enter the config dir")?;
    let (settings, notices, settings_error) = load_or_default(&config_dir(), &project);
    let (templates, template_notices) = load_templates(&config_dir(), Some(&project));
    // R21: the last run's tool outputs.
    std::fs::remove_dir_all(config_dir().join("scratch")).ok();
    // Stages a closed run never committed or dropped.
    std::fs::remove_dir_all(wai_ext::staging_dir(&config_dir())).ok();
    for notice in notices.iter().chain(&template_notices) {
        eprintln!("wai: {notice}");
    }
    app::run(
        project,
        settings,
        templates,
        notices,
        template_notices,
        settings_error,
        state_path,
        update_notice,
    );
    Ok(())
}

/// A `settings.toml` that does not parse must not stop wai: it opens with the defaults and shows
/// the error in Settings.
fn load_or_default(user_dir: &Path, project: &Path) -> (Settings, Vec<String>, Option<String>) {
    match Settings::load(user_dir, Some(project)) {
        Ok((settings, notices)) => (settings, notices, None),
        Err(e) => (Settings::default(), Vec::new(), Some(e.to_string())),
    }
}

/// `wai [PATH]`: the argument, else the last project while it exists, else the current dir (see
/// `fallback_project`).
fn resolve_project(arg: Option<PathBuf>, last: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = match (arg, last.filter(|p| p.is_dir())) {
        (Some(path), _) | (None, Some(path)) => path,
        (None, None) => {
            let canonical = wai_settings::canonical;
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(canonical));
            let cwd = canonical(&std::env::current_dir()?);
            fallback_project(cwd, exe_dir.as_deref(), std::env::home_dir())
        }
    };
    let shown = wai_settings::shown;
    let project = wai_settings::try_canonical(&path)
        .with_context(|| format!("cannot open {}", shown(&path)))?;
    anyhow::ensure!(project.is_dir(), "{} is not a folder", shown(&project));
    Ok(project)
}

/// The current dir, unless a launcher picked it: Finder, the Dock and `open` start the app at `/`,
/// a Windows shortcut in its install dir. Then the home folder, never a whole disk.
fn fallback_project(cwd: PathBuf, exe_dir: Option<&Path>, home: Option<PathBuf>) -> PathBuf {
    let launched = cwd.parent().is_none() || exe_dir == Some(cwd.as_path());
    match home {
        Some(home) if launched => home,
        _ => cwd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_user_file_opens_with_defaults() {
        let dir = std::env::temp_dir().join(format!("wai-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.toml"), "theme = [").unwrap();
        let (settings, notices, error) = load_or_default(&dir, &dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(settings.default_agent, None);
        assert!(notices.is_empty());
        assert!(error.unwrap().starts_with("invalid settings in"));
    }

    #[test]
    fn project_comes_from_arg_then_last_then_cwd() {
        let tmp = wai_settings::canonical(&std::env::temp_dir());
        let cwd = wai_settings::canonical(&std::env::current_dir().unwrap());
        let missing = PathBuf::from("/nonexistent-wai-project");

        assert_eq!(resolve_project(Some(tmp.clone()), None).unwrap(), tmp);
        assert_eq!(resolve_project(None, Some(tmp.clone())).unwrap(), tmp);
        assert_eq!(resolve_project(None, Some(missing.clone())).unwrap(), cwd);
        assert_eq!(resolve_project(None, None).unwrap(), cwd);
        assert!(resolve_project(Some(missing), None).is_err());

        // Finder, the Dock and `open` start the app at `/`; a Windows shortcut, in its own dir.
        let (home, exe_dir) = (PathBuf::from("/home/me"), Path::new("/apps/wai"));
        let fallback = |cwd: &str| fallback_project(cwd.into(), Some(exe_dir), Some(home.clone()));
        assert_eq!(fallback("/"), home);
        assert_eq!(fallback("/apps/wai"), home);
        assert_eq!(fallback("/src/project"), PathBuf::from("/src/project"));
        assert_eq!(fallback("/apps"), PathBuf::from("/apps"));
        assert_eq!(fallback_project("/".into(), None, None), PathBuf::from("/"));
    }
}
