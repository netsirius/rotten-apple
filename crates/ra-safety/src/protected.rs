//! Lista fail-closed incrustada (RF-SEG-04, 05 §1.2).
//!
//! Tiene prioridad sobre todo lo demás: ni la configuración, ni el catálogo,
//! ni un plan pueden anularla. Un elemento está protegido si:
//!
//! - está dentro de una raíz protegida;
//! - es un **ancestro** de una raíz protegida (mover `~/Library` movería
//!   también `~/Library/Keychains`);
//! - es la carpeta personal, la raíz de un volumen o los datos de la propia app;
//! - algún componente es una fototeca (`*.photoslibrary`) o una copia de
//!   Time Machine (`Backups.backupdb`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::{self, PathError};

/// Quién propone el elemento. Las carpetas de documentos del usuario solo
/// son accesibles por selección explícita, nunca por reglas de categoría.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Rules,
    ExplicitSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtectionReason {
    #[error("ruta protegida ({root})")]
    InsideProtected { root: PathBuf },
    #[error("contiene una ruta protegida ({root})")]
    AncestorOfProtected { root: PathBuf },
    #[error("carpeta de documentos del usuario ({root}): solo por selección explícita")]
    UserFolder { root: PathBuf },
    #[error("es la carpeta personal")]
    Home,
    #[error("es la raíz de un volumen")]
    VolumeRoot,
    #[error("datos o journal de la propia app")]
    AppState,
    #[error("fototeca o copia de Time Machine")]
    ProtectedBundle,
    #[error("ruta no válida: {detail}")]
    InvalidPath { detail: String },
}

impl From<PathError> for ProtectionReason {
    fn from(e: PathError) -> Self {
        ProtectionReason::InvalidPath {
            detail: e.to_string(),
        }
    }
}

/// Raíces absolutas que nunca se tocan.
const SYSTEM_ROOTS: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr",
    "/private/var/db",
    "/private/var/vm",
    "/var/db",
    "/var/vm",
    "/Library/Apple",
    "/Library/Keychains",
];

/// Excepciones dentro de las raíces del sistema.
const SYSTEM_EXCEPTIONS: &[&str] = &["/usr/local"];

/// Relativas a la carpeta personal; nunca se tocan.
const HOME_ROOTS: &[&str] = &[
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Messages",
    "Library/Mail",
    "Library/Safari",
    "Library/Application Support/AddressBook",
    "Library/Application Support/com.apple.TCC",
    "Library/Calendars",
    "Music/Music",
    ".ssh",
    ".gnupg",
];

/// Relativas a la carpeta personal; solo por selección explícita.
const HOME_USER_FOLDERS: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
];

#[derive(Debug, Clone)]
pub struct ProtectedList {
    home: PathBuf,
    app_state: PathBuf,
    always: Vec<PathBuf>,
    exceptions: Vec<PathBuf>,
    user_folders: Vec<PathBuf>,
}

impl ProtectedList {
    /// Lista incrustada para una carpeta personal y un directorio de estado
    /// de la app concretos (ambos absolutos y canónicos).
    pub fn embedded(home: &Path, app_state: &Path) -> Self {
        let mut always: Vec<PathBuf> = SYSTEM_ROOTS.iter().map(PathBuf::from).collect();
        always.extend(HOME_ROOTS.iter().map(|r| home.join(r)));
        always.push(app_state.to_path_buf());
        Self {
            home: home.to_path_buf(),
            app_state: app_state.to_path_buf(),
            always,
            exceptions: SYSTEM_EXCEPTIONS.iter().map(PathBuf::from).collect(),
            user_folders: HOME_USER_FOLDERS.iter().map(|r| home.join(r)).collect(),
        }
    }

    /// Raíces protegidas en todo caso (para `doctor` y las pruebas).
    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        self.always.iter().map(PathBuf::as_path)
    }

    /// `Ok(())` si el elemento puede entrar en un plan con ese origen.
    pub fn check(&self, path: &Path, origin: Origin) -> Result<(), ProtectionReason> {
        paths::check_absolute_normal(path)?;
        let comps = paths::folded_components(path);

        if comps.is_empty() {
            return Err(ProtectionReason::VolumeRoot);
        }
        if comps.len() == 2 && comps[0] == "volumes" {
            return Err(ProtectionReason::VolumeRoot);
        }
        if comps
            .iter()
            .any(|c| c.ends_with(".photoslibrary") || c == "backups.backupdb")
        {
            return Err(ProtectionReason::ProtectedBundle);
        }
        if paths::folded_components(&self.home) == comps {
            return Err(ProtectionReason::Home);
        }
        if paths::is_within(path, &self.app_state) {
            return Err(ProtectionReason::AppState);
        }

        let excepted = self.exceptions.iter().any(|e| paths::is_within(path, e));
        for root in &self.always {
            if paths::is_within(path, root) && !excepted {
                return Err(ProtectionReason::InsideProtected { root: root.clone() });
            }
            if paths::is_within(root, path) {
                return Err(ProtectionReason::AncestorOfProtected { root: root.clone() });
            }
        }
        for root in &self.user_folders {
            if paths::is_within(root, path) && root.as_path() != path {
                return Err(ProtectionReason::AncestorOfProtected { root: root.clone() });
            }
            if origin == Origin::Rules && paths::is_within(path, root) {
                return Err(ProtectionReason::UserFolder { root: root.clone() });
            }
        }
        // La carpeta personal es ancestro de casi todo lo anterior, pero se
        // comprueba aparte por si la lista cambia.
        if paths::is_within(&self.home, path) {
            return Err(ProtectionReason::AncestorOfProtected {
                root: self.home.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> ProtectedList {
        ProtectedList::embedded(
            Path::new("/Users/ana"),
            Path::new("/Users/ana/Library/Application Support/app.test"),
        )
    }

    fn allowed(p: &str) -> bool {
        list().check(Path::new(p), Origin::Rules).is_ok()
    }

    #[test]
    fn caches_are_allowed() {
        assert!(allowed("/Users/ana/Library/Caches/com.example.Editor"));
        assert!(allowed("/Users/ana/Library/Logs/old.log"));
        assert!(allowed("/usr/local/var/cache/x"));
    }

    #[test]
    fn protected_roots_and_their_contents() {
        assert!(!allowed("/System/Library"));
        assert!(!allowed("/usr/bin/true"));
        assert!(!allowed("/Users/ana/Library/Keychains/login.keychain-db"));
        assert!(!allowed("/Users/ana/library/mobile documents/x"));
        assert!(!allowed("/Users/ana/.ssh/id_ed25519"));
    }

    #[test]
    fn ancestors_of_protected_roots() {
        assert!(!allowed("/Users/ana/Library"));
        assert!(!allowed("/Users/ana"));
        assert!(!allowed("/Users"));
        assert!(!allowed("/"));
        assert!(!allowed("/Users/ana/Library/Application Support"));
    }

    #[test]
    fn user_folders_only_by_explicit_selection() {
        let l = list();
        let p = Path::new("/Users/ana/Downloads/setup.dmg");
        assert!(l.check(p, Origin::Rules).is_err());
        assert!(l.check(p, Origin::ExplicitSelection).is_ok());
        // La carpeta en sí: por selección explícita sí, por reglas nunca.
        let d = Path::new("/Users/ana/Documents");
        assert!(l.check(d, Origin::ExplicitSelection).is_ok());
        assert!(l.check(d, Origin::Rules).is_err());
    }

    #[test]
    fn bundles_volumes_and_app_state() {
        assert!(!allowed(
            "/Users/ana/Pictures/Photos Library.photoslibrary/x"
        ));
        assert!(!allowed("/Users/ana/Library/Caches/x/Fotos.PhotosLibrary"));
        assert!(!allowed("/Volumes/Backup"));
        assert!(!allowed("/Volumes/Backup/Backups.backupdb/a"));
        assert!(!allowed(
            "/Users/ana/Library/Application Support/app.test/journal/2026-11.jsonl"
        ));
    }

    #[test]
    fn invalid_paths_are_rejected() {
        assert!(!allowed("relative/path"));
        assert!(!allowed("/Users/ana/Library/Caches/../Keychains"));
    }
}
