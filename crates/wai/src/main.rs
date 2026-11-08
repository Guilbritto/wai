mod agents;
mod app;
mod chat;
mod sidebar;
mod workspace;

use std::path::PathBuf;

use anyhow::Context as _;
use wai_settings::{AppState, Settings, config_dir};

fn main() -> anyhow::Result<()> {
    let state = AppState::load(&config_dir().join("state.json"));
    let project = resolve_project(std::env::args_os().nth(1).map(PathBuf::from), state.project)?;
    let settings = Settings::load(&config_dir(), Some(&project))?;
    app::run(project, settings);
    Ok(())
}

/// `wai [PATH]`: the argument, else the last project while it exists, else the current dir.
fn resolve_project(arg: Option<PathBuf>, last: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = match (arg, last.filter(|p| p.is_dir())) {
        (Some(path), _) | (None, Some(path)) => path,
        (None, None) => std::env::current_dir()?,
    };
    let project = path
        .canonicalize()
        .with_context(|| format!("cannot open {}", path.display()))?;
    anyhow::ensure!(project.is_dir(), "{} is not a folder", project.display());
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_comes_from_arg_then_last_then_cwd() {
        let tmp = std::env::temp_dir().canonicalize().unwrap();
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let missing = PathBuf::from("/nonexistent-wai-project");

        assert_eq!(resolve_project(Some(tmp.clone()), None).unwrap(), tmp);
        assert_eq!(resolve_project(None, Some(tmp.clone())).unwrap(), tmp);
        assert_eq!(resolve_project(None, Some(missing.clone())).unwrap(), cwd);
        assert_eq!(resolve_project(None, None).unwrap(), cwd);
        assert!(resolve_project(Some(missing), None).is_err());
    }
}
