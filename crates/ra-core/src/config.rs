//! `rotten config get|set|path` (T-13, 07 §3) y el enlace
//! `~/.config/rotten/config.toml` → configuración canónica (07 §2).
//!
//! Cuándo se crea el enlace: solo en un `config set` sin `--dry-run`, una
//! vez escrita la configuración. Es el momento en que la persona decide
//! tener una configuración propia, y el enlace nace apuntando a un fichero
//! que existe. `config get`, `config path`, `doctor` y el resto de lecturas
//! nunca lo crean.

use std::fs;
use std::path::PathBuf;

use ra_safety::config_link;
use ra_safety::{Config, LinkState, SettingValue};
use serde::Serialize;

use crate::{Core, CoreError};

/// Ruta del enlace y lo que hay en ella.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigLink {
    pub path: PathBuf,
    #[serde(flatten)]
    pub state: LinkState,
}

/// Resultado de `config set`.
#[derive(Debug, Clone)]
pub struct ConfigSet {
    /// El valor tal como queda guardado.
    pub value: SettingValue,
    /// Si el fichero cambia (o cambiaría, con `dry_run`).
    pub changed: bool,
    pub dry_run: bool,
    /// El enlace tras el cambio.
    pub link: ConfigLink,
    /// Si este `set` ha creado el enlace.
    pub link_created: bool,
    /// Por qué no se pudo crear el enlace (la configuración sí se guardó).
    pub link_error: Option<String>,
}

impl Core {
    /// Lo que hay en la ruta del enlace. No cambia nada.
    pub fn config_link(&self) -> ConfigLink {
        let env = self.env();
        ConfigLink {
            path: env.config_link_path.clone(),
            state: config_link::inspect(&env.config_link_path, &env.config_path),
        }
    }

    /// Aborta (5) si en la ruta del enlace hay otra cosa (RF-SEG-05 c).
    pub(crate) fn config_link_or_abort(&self) -> Result<(), CoreError> {
        match self.config_link() {
            ConfigLink {
                path,
                state: LinkState::Conflict { found },
            } => Err(CoreError::ConfigLinkConflict {
                path,
                target: self.env().config_path.clone(),
                found,
            }),
            _ => Ok(()),
        }
    }

    /// Configuración válida para escribirla: nunca sobre un fichero corrupto
    /// (CU-07 A3) ni con otra cosa en la ruta del enlace.
    pub(crate) fn writable_config(&self) -> Result<Config, CoreError> {
        let config = self.editable_config()?;
        self.config_link_or_abort()?;
        Ok(config)
    }

    /// `config get <clave>`: el valor guardado o el que se aplica por
    /// defecto. Con la configuración corrupta no se puede saber (5).
    pub fn config_get(&self, key: &str) -> Result<SettingValue, CoreError> {
        Ok(self.editable_config()?.get(key)?)
    }

    /// `config set <clave> <valor>`: valida la clave y el valor, escribe de
    /// forma atómica y, sin `dry_run`, crea el enlace si en su ruta no hay
    /// nada. Con la configuración corrupta o con otra cosa en la ruta del
    /// enlace no cambia nada (5).
    pub fn config_set(&self, key: &str, raw: &str, dry_run: bool) -> Result<ConfigSet, CoreError> {
        let env = self.env();
        let mut config = self.writable_config()?;
        let before = config.clone();
        let value = config.set(key, raw)?;
        // Las mismas reglas que al cargar: lo que se escribe se podrá leer.
        config.validate(&env.home).map_err(|reason| {
            CoreError::Other(format!("the new configuration is invalid: {reason}"))
        })?;
        let changed = config != before;
        let mut link_created = false;
        let mut link_error = None;
        if !dry_run {
            if changed {
                config.save(&env.config_path)?;
            }
            if fs::symlink_metadata(&env.config_path).is_ok() {
                let was = config_link::inspect(&env.config_link_path, &env.config_path);
                match config_link::create(&env.config_link_path, &env.config_path) {
                    Ok(now) => link_created = was == LinkState::Missing && now == LinkState::Linked,
                    Err(e) => link_error = Some(e.to_string()),
                }
            }
        }
        Ok(ConfigSet {
            value,
            changed,
            dry_run,
            link: self.config_link(),
            link_created,
            link_error,
        })
    }
}
