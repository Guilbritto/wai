//! Installed extensions: each in `<config>/extensions/<id>/`, with what the user consented to in
//! `<config>/extensions.json`.

use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::install::{
    self, InstallError, MANIFEST_FILE, Source, Staged, data_root, extensions_dir, staging_path,
};
use crate::{ExtId, Manifest, Permission, parse_manifest, parse_permission};

#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub manifest: Manifest,
    /// `<config>/extensions/<id>`.
    pub dir: PathBuf,
    /// `<config>/extension-data/<id>`: survives updates and uninstalls; may not exist yet.
    pub data: PathBuf,
    pub enabled: bool,
    /// What the user consented to. Calls are checked against this, not against the manifest,
    /// which can change on disk.
    pub granted: Vec<Permission>,
}

impl Installed {
    /// A command the manifest names: a bare name is looked up on PATH, a relative path is inside
    /// the extension's folder.
    pub fn program(&self, command: &str) -> PathBuf {
        if command.contains(['/', '\\']) {
            self.dir.join(command)
        } else {
            PathBuf::from(command)
        }
    }
}

/// One entry of `extensions.json`.
#[derive(Serialize, Deserialize)]
struct Record {
    id: String,
    version: String,
    enabled: bool,
    granted: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtStore {
    config_dir: PathBuf,
    installed: Vec<Installed>,
}

impl ExtStore {
    /// A missing or corrupt `extensions.json` is an empty store.
    pub fn open(config_dir: &Path) -> ExtStore {
        let records: Vec<Record> = std::fs::read(config_dir.join("extensions.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // ponytail: an extension that no longer loads is left out (and its record goes with the
        // next save); its folder is replaced when it is installed again.
        let installed = records
            .iter()
            .filter_map(|r| {
                load(config_dir, r)
                    .inspect_err(|e| eprintln!("wai: extension {:?} left out: {e}", r.id))
                    .ok()
            })
            .collect();
        ExtStore {
            config_dir: config_dir.to_path_buf(),
            installed,
        }
    }

    /// Copies or clones `source` into the staging area and parses its manifest. Nothing is
    /// installed, recorded or enabled; dropping the stage removes it (drop it off the main
    /// thread).
    pub fn stage(&self, source: &Source) -> Result<Staged, InstallError> {
        install::stage(&self.config_dir, source)
    }

    /// Moves the stage into `<config>/extensions/<id>/` and records the consent, enabled. v1
    /// consent is all or nothing: `granted` must be exactly what the manifest asks for. v1 has no
    /// upgrade: an installed id is refused (uninstall it first).
    pub fn commit(
        &mut self,
        staged: Staged,
        granted: Vec<Permission>,
    ) -> Result<ExtId, InstallError> {
        let asked = &staged.manifest().permissions;
        if !(granted.iter().all(|p| asked.contains(p)) && asked.iter().all(|p| granted.contains(p)))
        {
            return Err(InstallError::Consent);
        }
        let id = staged.manifest().id.clone();
        if self.position(&id).is_ok() {
            return Err(InstallError::AlreadyInstalled(id.as_str().into()));
        }
        let dir = extensions_dir(&self.config_dir).join(id.as_str());
        // A folder without a record is what an interrupted uninstall left.
        remove_dir(&dir)?;
        std::fs::rename(staged.dir(), &dir)?;
        self.installed.push(Installed {
            manifest: staged.manifest().clone(),
            dir: dir.clone(),
            data: data_root(&self.config_dir).join(id.as_str()),
            enabled: true,
            granted,
        });
        if let Err(e) = self.save() {
            self.installed.pop();
            std::fs::remove_dir_all(&dir).ok();
            return Err(e.into());
        }
        Ok(id)
    }

    pub fn list(&self) -> Vec<Installed> {
        self.installed.clone()
    }

    pub fn set_enabled(&mut self, id: &ExtId, on: bool) -> std::io::Result<()> {
        let ix = self.position(id)?;
        let was = std::mem::replace(&mut self.installed[ix].enabled, on);
        let saved = self.save();
        if saved.is_err() {
            self.installed[ix].enabled = was;
        }
        saved
    }

    /// Removes the record and moves the folder into the staging area, both quick, and returns
    /// where it went: the caller deletes it off the main thread (launch clears what a crash
    /// leaves). Callers stop the extension's processes first (Review Focus 3): nothing here
    /// knows about them.
    pub fn uninstall(&mut self, id: &ExtId) -> std::io::Result<PathBuf> {
        let ix = self.position(id)?;
        let removed = self.installed.remove(ix);
        if let Err(e) = self.save() {
            self.installed.insert(ix, removed);
            return Err(e);
        }
        let trash = staging_path(&self.config_dir)?;
        match std::fs::rename(&removed.dir, &trash) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
            _ => Ok(trash),
        }
    }

    fn position(&self, id: &ExtId) -> std::io::Result<usize> {
        let ix = self.installed.iter().position(|i| &i.manifest.id == id);
        ix.ok_or_else(|| {
            std::io::Error::new(
                ErrorKind::NotFound,
                format!("{} is not installed", id.as_str()),
            )
        })
    }

    /// Atomic: a temp file, created with create_new after removing whatever held its name
    /// (R40), renamed over `extensions.json`.
    fn save(&self) -> std::io::Result<()> {
        let records: Vec<Record> = (self.installed.iter())
            .map(|i| Record {
                id: i.manifest.id.as_str().into(),
                version: i.manifest.version.clone(),
                enabled: i.enabled,
                granted: i.granted.iter().map(ToString::to_string).collect(),
            })
            .collect();
        std::fs::create_dir_all(&self.config_dir)?;
        let tmp = self.config_dir.join("extensions.json.tmp");
        match std::fs::remove_file(&tmp) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let mut file = std::fs::File::create_new(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(&records)?)?;
        file.sync_all()?;
        std::fs::rename(&tmp, self.config_dir.join("extensions.json"))
    }
}

fn load(config_dir: &Path, r: &Record) -> Result<Installed, String> {
    // Validated: the id names a folder that uninstall removes.
    let id = ExtId::new(&r.id).map_err(|e| e.to_string())?;
    let dir = extensions_dir(config_dir).join(id.as_str());
    let text = wai_settings::read_config(&dir.join(MANIFEST_FILE))
        .map_err(|e| e.to_string())?
        .ok_or("its manifest is gone")?;
    let manifest = parse_manifest(&text, &dir).map_err(|e| e.to_string())?;
    if manifest.id != id {
        return Err(format!("its manifest names {:?}", manifest.id.as_str()));
    }
    let granted = (r.granted.iter())
        .map(|p| parse_permission(p))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(Installed {
        manifest,
        dir,
        data: data_root(config_dir).join(id.as_str()),
        enabled: r.enabled,
        granted,
    })
}

/// `remove_dir_all`, where a folder already gone is fine.
fn remove_dir(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
