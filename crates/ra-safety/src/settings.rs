//! Claves de `config.toml` que se leen y cambian con `rotten config get|set`
//! (T-13). Lista cerrada: una clave desconocida o un valor de otro tipo se
//! rechazan antes de escribir nada, y al cargar el fichero se aplican las
//! mismas reglas (lo que se escribe es lo que se leerá).
//!
//! Las exclusiones se leen aquí, pero se cambian con `rotten exclude
//! add|remove`, que las valida una a una.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::exclusions::{Config, Exclusions};

/// Idioma de la interfaz (RF-AJU-07): el del sistema o uno fijo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    System,
    Es,
    En,
}

impl Language {
    const NAMES: &'static [&'static str] = &["system", "es", "en"];

    fn name(self) -> &'static str {
        match self {
            Language::System => "system",
            Language::Es => "es",
            Language::En => "en",
        }
    }
}

/// Todas las claves, en el orden en que se documentan.
///
/// `journal.retention_days` (RF-HIS-08) no se expone hasta que algo la use
/// (T-105); si está en el fichero, se tolera como clave desconocida.
pub const KEYS: &[&str] = &[
    "language",
    "exclusions.paths",
    "exclusions.globs",
    "exclusions.rules",
    "exclusions.categories",
    "exclusions.bundle_ids",
];

/// El valor de una clave, con su tipo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SettingValue {
    Text(String),
    List(Vec<String>),
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Text(s) => f.write_str(s),
            SettingValue::List(l) => f.write_str(&l.join(", ")),
        }
    }
}

/// Qué valores admite una clave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    OneOf(&'static [&'static str]),
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expected::OneOf(v) => write!(f, "one of: {}", v.join(", ")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingError {
    #[error("unknown configuration key «{key}»; the keys are: {}", KEYS.join(", "))]
    UnknownKey { key: String },
    #[error("«{key}» is changed with `rotten exclude add|remove`, not with `rotten config set`")]
    ReadOnly { key: String },
    #[error("invalid value «{value}» for «{key}»: expected {expected}")]
    InvalidValue {
        key: String,
        value: String,
        expected: Expected,
    },
}

impl Config {
    fn exclusion_list(&self, key: &str) -> Option<&Vec<String>> {
        let l = &self.exclusions;
        Some(match key {
            "exclusions.paths" => &l.paths,
            "exclusions.globs" => &l.globs,
            "exclusions.rules" => &l.rules,
            "exclusions.categories" => &l.categories,
            "exclusions.bundle_ids" => &l.bundle_ids,
            _ => return None,
        })
    }

    /// El valor de `key`; si no está en el fichero, el que se aplica por
    /// defecto.
    pub fn get(&self, key: &str) -> Result<SettingValue, SettingError> {
        match key {
            "language" => Ok(SettingValue::Text(
                self.language.unwrap_or(Language::System).name().to_owned(),
            )),
            _ => self
                .exclusion_list(key)
                .map(|l| SettingValue::List(l.clone()))
                .ok_or_else(|| SettingError::UnknownKey {
                    key: key.to_owned(),
                }),
        }
    }

    /// Cambia `key` a `raw`, ya validado. Devuelve el valor tal como queda.
    pub fn set(&mut self, key: &str, raw: &str) -> Result<SettingValue, SettingError> {
        match key {
            "language" => {
                let lang = match raw.trim().to_ascii_lowercase().as_str() {
                    "system" => Language::System,
                    "es" => Language::Es,
                    "en" => Language::En,
                    _ => {
                        return Err(SettingError::InvalidValue {
                            key: key.to_owned(),
                            value: raw.to_owned(),
                            expected: Expected::OneOf(Language::NAMES),
                        });
                    }
                };
                self.language = Some(lang);
            }
            _ if self.exclusion_list(key).is_some() => {
                return Err(SettingError::ReadOnly {
                    key: key.to_owned(),
                });
            }
            _ => {
                return Err(SettingError::UnknownKey {
                    key: key.to_owned(),
                });
            }
        }
        self.get(key)
    }

    /// Las exclusiones compiladas contra `home`. `ConfigState::load` da por
    /// corrupto un fichero que no las cumple.
    pub fn validate(&self, home: &Path) -> Result<(), String> {
        Exclusions::compile(self, home).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::exclusions::ConfigState;

    const HOME: &str = "/Users/ana";

    #[test]
    fn every_key_can_be_read_with_its_default() {
        let c = Config::default();
        for key in KEYS {
            c.get(key).unwrap_or_else(|e| panic!("{key}: {e}"));
        }
        assert_eq!(c.get("language"), Ok(SettingValue::Text("system".into())));
        assert_eq!(c.get("exclusions.paths"), Ok(SettingValue::List(vec![])));
        for unknown in ["nope", "journal.retention_days"] {
            assert!(matches!(
                c.get(unknown),
                Err(SettingError::UnknownKey { .. })
            ));
        }
    }

    #[test]
    fn set_validates_keys_and_types() {
        let mut c = Config::default();
        assert_eq!(
            c.set("language", " ES "),
            Ok(SettingValue::Text("es".into()))
        );
        assert_eq!(c.language, Some(Language::Es));
        let before = c.clone();
        for bad in ["fr", "", "español"] {
            assert!(
                matches!(
                    c.set("language", bad),
                    Err(SettingError::InvalidValue { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(
            c.set("exclusions.paths", "~/x"),
            Err(SettingError::ReadOnly { .. })
        ));
        for unknown in ["schema_version", "journal.retention_days"] {
            assert!(matches!(
                c.set(unknown, "2"),
                Err(SettingError::UnknownKey { .. })
            ));
        }
        assert_eq!(c, before, "un valor rechazado no cambia nada");
    }

    /// Lo que escribe `set` se vuelve a cargar igual; un valor que `set`
    /// rechaza, escrito a mano, deja la configuración corrupta (fail-closed).
    #[test]
    fn what_is_written_is_what_is_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("config.toml");
        let mut c = Config::default();
        c.set("language", "en").expect("set");
        c.save(&p).expect("save");
        assert_eq!(
            ConfigState::load(&p, Path::new(HOME)),
            ConfigState::Loaded(c)
        );
        for bad in [
            "language = \"fr\"\n",
            "language = 1\n",
            "[exclusions]\nkeep = [\"~/x\"]\n",
        ] {
            fs::write(&p, bad).expect("write");
            assert!(
                matches!(
                    ConfigState::load(&p, Path::new(HOME)),
                    ConfigState::Corrupt { .. }
                ),
                "{bad:?}"
            );
        }
    }

    /// I3: claves desconocidas fuera de `[exclusions]` se toleran, se avisan
    /// y se conservan al reescribir.
    #[test]
    fn unknown_keys_outside_exclusions_are_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("config.toml");
        fs::write(
            &p,
            "theme = \"dark\"\n\n[journal]\nretention_days = 30\n\n[exclusions]\npaths = [\"~/keep\"]\n",
        )
        .expect("write");
        let ConfigState::Loaded(mut c) = ConfigState::load(&p, Path::new(HOME)) else {
            panic!("debería cargar");
        };
        assert_eq!(c.unknown_keys(), ["journal", "theme"]);
        c.set("language", "es").expect("set");
        c.save(&p).expect("save");
        let text = fs::read_to_string(&p).expect("read");
        assert!(text.contains("theme = \"dark\""), "{text}");
        assert!(text.contains("retention_days = 30"), "{text}");
        let ConfigState::Loaded(again) = ConfigState::load(&p, Path::new(HOME)) else {
            panic!("debería cargar: {text}");
        };
        assert_eq!(again, c);
        assert_eq!(again.exclusions.paths, ["~/keep"]);
    }

    #[test]
    fn save_is_atomic_and_leaves_no_temporaries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("config.toml");
        fs::write(&p, "viejo").expect("write");
        let mut c = Config::default();
        c.set("language", "es").expect("set");
        c.save(&p).expect("save");
        c.save(&p).expect("save otra vez");
        let names: Vec<String> = fs::read_dir(dir.path())
            .expect("ls")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["config.toml"]);
        assert_eq!(
            fs::read_to_string(&p).expect("read"),
            "schema_version = 1\nlanguage = \"es\"\n\n[exclusions]\n"
        );
    }

    /// B1: si la configuración canónica es un enlace (dotfiles), se escribe
    /// en su destino y el enlace se conserva; también con un destino
    /// relativo o con un enlace a otro enlace.
    #[test]
    fn save_writes_through_a_symlinked_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(dir.path()).expect("canon");
        let dotfiles = root.join("dotfiles");
        let state = root.join("state");
        fs::create_dir_all(&dotfiles).expect("mkdir");
        fs::create_dir_all(&state).expect("mkdir");
        let real = dotfiles.join("rotten.toml");
        fs::write(&real, "schema_version = 1\n").expect("write");
        let hop = root.join("hop.toml");
        std::os::unix::fs::symlink("dotfiles/rotten.toml", &hop).expect("ln");
        let canonical = state.join("config.toml");
        std::os::unix::fs::symlink(&hop, &canonical).expect("ln");

        let mut c = Config::default();
        c.set("language", "en").expect("set");
        c.save(&canonical).expect("save");

        assert!(
            fs::symlink_metadata(&canonical)
                .expect("lstat")
                .is_symlink()
        );
        assert_eq!(fs::read_link(&canonical).expect("readlink"), hop);
        assert_eq!(
            fs::read_link(&hop).expect("readlink"),
            Path::new("dotfiles/rotten.toml")
        );
        assert!(
            fs::read_to_string(&real)
                .expect("read")
                .contains("language = \"en\"")
        );
        let leftovers: Vec<_> = fs::read_dir(&dotfiles)
            .expect("ls")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "{leftovers:?}");
        assert_eq!(
            ConfigState::load(&canonical, Path::new(HOME)),
            ConfigState::Loaded(c)
        );
    }
}
