//! Entorno: carpeta personal, estado de la app y Papelera.
//!
//! Variables reconocidas (todas opcionales):
//!
//! - `ROTTEN_HOME`: carpeta personal sobre la que trabajar (por defecto
//!   `HOME`). Las pruebas la apuntan a un directorio temporal.
//! - `ROTTEN_STATE_DIR`: estado de la app (configuración, journal,
//!   manifiestos). Por defecto `<home>/Library/Application Support/<bundle-id>`.
//! - `ROTTEN_TRASH_DIR`: en lugar de la Papelera del sistema, mover a esta
//!   carpeta (mismo volumen, sin sobrescribir). Para pruebas y entornos
//!   aislados; `doctor` lo indica.

use std::fs;
use std::path::{Path, PathBuf};

use crate::TrashBackend;

/// Bundle ID provisional (D-08: falta fijarlo antes de la beta).
pub const BUNDLE_ID: &str = "io.github.netsirius.RottenApple";

#[derive(Debug, Clone)]
pub struct Env {
    /// Carpeta personal, absoluta y canónica.
    pub home: PathBuf,
    /// De dónde salió `home`: `ROTTEN_HOME` o `HOME`.
    pub home_source: &'static str,
    pub state_dir: PathBuf,
    pub config_path: PathBuf,
    pub trash: TrashBackend,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("no se sabe cuál es la carpeta personal: define HOME")]
    NoHome,
    #[error("{var}: {path}: {detail}")]
    BadPath {
        var: &'static str,
        path: String,
        detail: String,
    },
}

fn canonical_dir(var: &'static str, raw: &str) -> Result<PathBuf, EnvError> {
    let p = Path::new(raw);
    let bad = |detail: String| EnvError::BadPath {
        var,
        path: raw.to_owned(),
        detail,
    };
    if !p.is_absolute() {
        return Err(bad("debe ser una ruta absoluta".into()));
    }
    // Se canonicaliza una vez al arrancar: así ningún componente de las rutas
    // que se construyen después es un enlace (p. ej. /var → /private/var).
    let c = fs::canonicalize(p).map_err(|e| bad(e.to_string()))?;
    if !c.is_dir() {
        return Err(bad("no es una carpeta".into()));
    }
    Ok(c)
}

impl Env {
    pub fn from_env() -> Result<Self, EnvError> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let (home_raw, home_source) = match (var("ROTTEN_HOME"), var("HOME")) {
            (Some(h), _) => (h, "ROTTEN_HOME"),
            (None, Some(h)) => (h, "HOME"),
            (None, None) => return Err(EnvError::NoHome),
        };
        let home = canonical_dir(home_source, &home_raw)?;
        let state_dir = match var("ROTTEN_STATE_DIR") {
            Some(s) => {
                let p = PathBuf::from(&s);
                if !p.is_absolute() {
                    return Err(EnvError::BadPath {
                        var: "ROTTEN_STATE_DIR",
                        path: s,
                        detail: "debe ser una ruta absoluta".into(),
                    });
                }
                p
            }
            None => default_state_dir(&home),
        };
        let trash = match var("ROTTEN_TRASH_DIR") {
            Some(t) => TrashBackend::Dir(canonical_dir("ROTTEN_TRASH_DIR", &t)?),
            None => TrashBackend::System,
        };
        let mut env = Self::new(home, state_dir, trash);
        env.home_source = home_source;
        Ok(env)
    }

    /// Entorno explícito (pruebas). `home` debe ser canónica.
    pub fn new(home: PathBuf, state_dir: PathBuf, trash: TrashBackend) -> Self {
        let config_path = state_dir.join("config.toml");
        Self {
            home,
            home_source: "explicit",
            state_dir,
            config_path,
            trash,
        }
    }
}

pub fn default_state_dir(home: &Path) -> PathBuf {
    home.join("Library/Application Support").join(BUNDLE_ID)
}
