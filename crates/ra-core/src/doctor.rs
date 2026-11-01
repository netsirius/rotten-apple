//! `rotten doctor`: permisos, plataforma, configuración y journal (RF-CLI-07).

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use ra_exec::ExecLock;
use ra_safety::ConfigState;
use serde::Serialize;

use crate::{Core, running_as_root};

/// Acceso total al disco: no hay API para consultarlo, así que se prueba
/// leyendo una ubicación protegida (05 §1.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FdaState {
    Granted,
    Denied,
    Undetermined,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub macos_version: Option<String>,
    pub arch: String,
    /// D-01: macOS 26 o posterior y Apple silicon.
    pub platform_supported: bool,
    pub running_as_root: bool,
    pub home: PathBuf,
    pub home_source: String,
    pub state_dir: PathBuf,
    pub config_path: PathBuf,
    /// `ok`, `missing` o `corrupt`.
    pub config: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    pub journal_dir: PathBuf,
    pub journal_records: usize,
    pub journal_problems: Vec<String>,
    pub trash: String,
    pub exec_lock_free: bool,
    pub full_disk_access: FdaState,
    pub running_apps_probe: Result<usize, String>,
    pub catalog_version: String,
}

fn macos_version() -> Option<String> {
    let out = Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn platform_supported(version: Option<&str>, arch: &str) -> bool {
    let major = version
        .and_then(|v| v.split('.').next())
        .and_then(|m| m.parse::<u32>().ok());
    arch == "aarch64" && major.is_some_and(|m| m >= 26)
}

fn fda_preflight(home: &std::path::Path) -> FdaState {
    // Ubicaciones que solo se pueden leer con Acceso total al disco.
    let mut saw_missing = true;
    for rel in ["Library/Safari", "Library/Mail", "Library/Messages"] {
        match fs::read_dir(home.join(rel)) {
            Ok(_) => return FdaState::Granted,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                saw_missing = false;
            }
            Err(_) => {}
        }
    }
    if saw_missing {
        FdaState::Undetermined
    } else {
        FdaState::Denied
    }
}

pub(crate) fn run(core: &Core) -> DoctorReport {
    let env = core.env();
    let macos = macos_version();
    let arch = std::env::consts::ARCH.to_owned();
    let (config, config_error) = match core.config_state() {
        ConfigState::Missing => ("missing", None),
        ConfigState::Loaded(_) => ("ok", None),
        ConfigState::Corrupt { reason, .. } => ("corrupt", Some(reason)),
    };
    let history = core.history_paths();
    let (records, problems) = match ra_history::read_journal(&history) {
        Ok((r, p)) => (r.len(), p),
        Err(e) => (0, vec![e.to_string()]),
    };
    let exec_lock_free = matches!(ExecLock::try_acquire(&env.state_dir), Ok(Some(_)));
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        platform_supported: platform_supported(macos.as_deref(), &arch),
        macos_version: macos,
        arch,
        running_as_root: running_as_root(),
        home: env.home.clone(),
        home_source: env.home_source.to_owned(),
        state_dir: env.state_dir.clone(),
        config_path: env.config_path.clone(),
        config: config.into(),
        config_error,
        journal_dir: history.journal_dir,
        journal_records: records,
        journal_problems: problems,
        trash: env.trash.describe(),
        exec_lock_free,
        full_disk_access: fda_preflight(&env.home),
        running_apps_probe: core
            .apps
            .running_bundle_ids()
            .map(|s| s.len())
            .map_err(|e| e.to_string()),
        catalog_version: core.catalog().catalog_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d01_platform_check() {
        assert!(platform_supported(Some("27.0.1"), "aarch64"));
        assert!(platform_supported(Some("26.0"), "aarch64"));
        assert!(!platform_supported(Some("15.6"), "aarch64"));
        assert!(!platform_supported(Some("27.0"), "x86_64"));
        assert!(!platform_supported(None, "aarch64"));
    }
}
