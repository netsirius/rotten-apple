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
//!   Time Machine (`Backups.backupdb`);
//! - viene de una selección explícita y está fuera de la carpeta personal
//!   (RF-SEG-19), o es una de las carpetas de documentos del usuario en sí
//!   (su contenido sí se puede elegir).

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
    /// La carpeta de documentos en sí, por selección explícita: se puede
    /// mover su contenido, no la carpeta.
    #[error("es una carpeta del usuario ({root}): se puede mover su contenido, no la carpeta")]
    UserFolderRoot { root: PathBuf },
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
    /// Selección explícita fuera de la carpeta personal (RF-SEG-19).
    #[error("está fuera de la carpeta personal: solo se pueden elegir rutas dentro de ~")]
    OutsideHome,
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
    /// Origen mínimo con el que se comprueba (ver [`ProtectedList::for_origin`]).
    floor: Origin,
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
            floor: Origin::Rules,
        }
    }

    /// La misma lista, pero comprobando siempre al menos con `origin`.
    ///
    /// Sirve para revalidar al ejecutar un plan cuyos elementos vienen todos
    /// de una selección explícita: el ejecutor comprueba con
    /// [`Origin::Rules`], y con `for_origin(Origin::ExplicitSelection)` el
    /// contenido de las carpetas de documentos deja de rechazarse. Todo lo
    /// demás (raíces protegidas, ancestros, carpeta personal, estado de la
    /// app, fototecas) no depende del origen y se sigue rechazando. Con
    /// selección explícita, además, nada fuera de la carpeta personal.
    pub fn for_origin(mut self, origin: Origin) -> Self {
        self.floor = origin;
        self
    }

    /// La carpeta de documentos del usuario (Documentos, Escritorio,
    /// Descargas, Imágenes, Películas, Música) que es exactamente `path`.
    pub fn user_folder_root(&self, path: &Path) -> Option<&Path> {
        let comps = paths::folded_components(path);
        self.user_folders
            .iter()
            .find(|r| paths::folded_components(r) == comps)
            .map(PathBuf::as_path)
    }

    /// Raíces protegidas en todo caso (para `doctor` y las pruebas).
    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        self.always.iter().map(PathBuf::as_path)
    }

    /// `Ok(())` si el elemento puede entrar en un plan con ese origen.
    pub fn check(&self, path: &Path, origin: Origin) -> Result<(), ProtectionReason> {
        let origin = if self.floor == Origin::ExplicitSelection {
            Origin::ExplicitSelection
        } else {
            origin
        };
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
            let is_root = paths::folded_components(root) == comps;
            if paths::is_within(root, path) && !is_root {
                return Err(ProtectionReason::AncestorOfProtected { root: root.clone() });
            }
            if origin == Origin::Rules && paths::is_within(path, root) {
                return Err(ProtectionReason::UserFolder { root: root.clone() });
            }
            if is_root {
                return Err(ProtectionReason::UserFolderRoot { root: root.clone() });
            }
        }
        // La carpeta personal es ancestro de casi todo lo anterior, pero se
        // comprueba aparte por si la lista cambia.
        if paths::is_within(&self.home, path) {
            return Err(ProtectionReason::AncestorOfProtected {
                root: self.home.clone(),
            });
        }
        // RF-SEG-19: lo elegido a mano, solo dentro de la carpeta personal.
        if origin == Origin::ExplicitSelection && !paths::is_within(path, &self.home) {
            return Err(ProtectionReason::OutsideHome);
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
        // La carpeta en sí, nunca: por reglas ni por selección explícita.
        for f in HOME_USER_FOLDERS {
            let d = Path::new("/Users/ana").join(f);
            assert_eq!(l.user_folder_root(&d), Some(d.as_path()));
            assert!(l.check(&d, Origin::Rules).is_err());
            assert!(l.check(&d.join("x"), Origin::ExplicitSelection).is_ok());
            if *f == "Music" {
                // Ya era ancestro de `~/Music/Music`, que es protegida.
                assert!(l.check(&d, Origin::ExplicitSelection).is_err());
                continue;
            }
            assert_eq!(
                l.check(&d, Origin::ExplicitSelection),
                Err(ProtectionReason::UserFolderRoot { root: d.clone() }),
                "{f}"
            );
        }
        // Sin distinguir mayúsculas, como APFS.
        assert!(matches!(
            l.check(Path::new("/Users/ana/downloads"), Origin::ExplicitSelection),
            Err(ProtectionReason::UserFolderRoot { .. })
        ));
        assert_eq!(l.user_folder_root(Path::new("/Users/ana/Library")), None);
    }

    #[test]
    fn explicit_floor_only_relaxes_user_folders() {
        let l = list().for_origin(Origin::ExplicitSelection);
        assert!(
            l.check(Path::new("/Users/ana/Downloads/setup.dmg"), Origin::Rules)
                .is_ok()
        );
        for p in [
            "/Users/ana",
            "/Users/ana/Library",
            "/Users/ana/Library/Keychains/login.keychain-db",
            "/Users/ana/Pictures/Fotos.photoslibrary",
            "/Users/ana/Library/Application Support/app.test/config.toml",
            "/System/Library",
        ] {
            assert!(l.check(Path::new(p), Origin::Rules).is_err(), "{p}");
        }
    }

    #[test]
    fn explicit_selection_only_inside_home() {
        let l = list();
        for p in [
            "/tmp/x",
            "/private/tmp/x",
            "/Volumes/Disco/x",
            "/usr/local/bin/x",
            "/Users/otra/x",
        ] {
            assert_eq!(
                l.check(Path::new(p), Origin::ExplicitSelection),
                Err(ProtectionReason::OutsideHome),
                "{p}"
            );
        }
        // Con reglas, lo de fuera de `~` sigue igual que antes.
        assert!(
            l.check(Path::new("/usr/local/var/cache/x"), Origin::Rules)
                .is_ok()
        );
        // El suelo de selección explícita también lo aplica al ejecutar.
        let floor = list().for_origin(Origin::ExplicitSelection);
        assert_eq!(
            floor.check(Path::new("/tmp/x"), Origin::Rules),
            Err(ProtectionReason::OutsideHome)
        );
        assert!(
            l.check(
                Path::new("/Users/ana/Downloads/x"),
                Origin::ExplicitSelection
            )
            .is_ok()
        );
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
