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

/// `[journal]` en `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalSettings {
    /// Días que se guardan las operaciones del journal (RF-HIS-08).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u32>,
}

impl JournalSettings {
    pub fn is_unset(&self) -> bool {
        self.retention_days.is_none()
    }
}

/// Retención del journal por defecto (RF-HIS-08) y límites admitidos.
pub const DEFAULT_RETENTION_DAYS: u32 = 180;
pub const MIN_RETENTION_DAYS: u32 = 1;
pub const MAX_RETENTION_DAYS: u32 = 3650;

/// Todas las claves, en el orden en que se documentan.
pub const KEYS: &[&str] = &[
    "language",
    "journal.retention_days",
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
    Integer(u64),
    List(Vec<String>),
}

impl fmt::Display for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::Text(s) => f.write_str(s),
            SettingValue::Integer(n) => write!(f, "{n}"),
            SettingValue::List(l) => f.write_str(&l.join(", ")),
        }
    }
}

/// Qué valores admite una clave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    OneOf(&'static [&'static str]),
    Range { min: u64, max: u64 },
}

impl fmt::Display for Expected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expected::OneOf(v) => write!(f, "one of: {}", v.join(", ")),
            Expected::Range { min, max } => write!(f, "a whole number from {min} to {max}"),
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

const RETENTION: Expected = Expected::Range {
    min: MIN_RETENTION_DAYS as u64,
    max: MAX_RETENTION_DAYS as u64,
};

fn retention_ok(days: u64) -> bool {
    (u64::from(MIN_RETENTION_DAYS)..=u64::from(MAX_RETENTION_DAYS)).contains(&days)
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
            "journal.retention_days" => Ok(SettingValue::Integer(u64::from(
                self.journal
                    .retention_days
                    .unwrap_or(DEFAULT_RETENTION_DAYS),
            ))),
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
        let invalid = |expected| SettingError::InvalidValue {
            key: key.to_owned(),
            value: raw.to_owned(),
            expected,
        };
        let v = raw.trim();
        match key {
            "language" => {
                let lang = match v.to_ascii_lowercase().as_str() {
                    "system" => Language::System,
                    "es" => Language::Es,
                    "en" => Language::En,
                    _ => return Err(invalid(Expected::OneOf(Language::NAMES))),
                };
                self.language = Some(lang);
            }
            "journal.retention_days" => {
                let days = v
                    .parse::<u64>()
                    .ok()
                    .filter(|d| retention_ok(*d))
                    .and_then(|d| u32::try_from(d).ok())
                    .ok_or_else(|| invalid(RETENTION))?;
                self.journal.retention_days = Some(days);
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

    /// Las reglas de cada clave y las exclusiones compiladas contra `home`.
    /// `ConfigState::load` da por corrupto un fichero que no las cumple.
    pub fn validate(&self, home: &Path) -> Result<(), String> {
        if let Some(d) = self.journal.retention_days
            && !retention_ok(u64::from(d))
        {
            return Err(format!(
                "journal.retention_days = {d}: expected {RETENTION}"
            ));
        }
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
        assert_eq!(
            c.get("journal.retention_days"),
            Ok(SettingValue::Integer(180))
        );
        assert_eq!(c.get("exclusions.paths"), Ok(SettingValue::List(vec![])));
        assert!(matches!(
            c.get("nope"),
            Err(SettingError::UnknownKey { .. })
        ));
    }

    #[test]
    fn set_validates_keys_and_types() {
        let mut c = Config::default();
        assert_eq!(
            c.set("language", " ES "),
            Ok(SettingValue::Text("es".into()))
        );
        assert_eq!(c.language, Some(Language::Es));
        assert_eq!(
            c.set("journal.retention_days", "30"),
            Ok(SettingValue::Integer(30))
        );
        let before = c.clone();
        for (key, bad) in [
            ("language", "fr"),
            ("language", ""),
            ("journal.retention_days", "0"),
            ("journal.retention_days", "3651"),
            ("journal.retention_days", "-5"),
            ("journal.retention_days", "30 días"),
            ("journal.retention_days", "99999999999999999999"),
        ] {
            assert!(
                matches!(c.set(key, bad), Err(SettingError::InvalidValue { .. })),
                "{key} = {bad:?}"
            );
        }
        assert!(matches!(
            c.set("exclusions.paths", "~/x"),
            Err(SettingError::ReadOnly { .. })
        ));
        assert!(matches!(
            c.set("schema_version", "2"),
            Err(SettingError::UnknownKey { .. })
        ));
        assert_eq!(c, before, "un valor rechazado no cambia nada");
    }

    /// Lo que escribe `set` se vuelve a cargar igual; lo que `set` rechaza,
    /// escrito a mano, deja la configuración corrupta (fail-closed).
    #[test]
    fn what_is_written_is_what_is_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("config.toml");
        let mut c = Config::default();
        c.set("language", "en").expect("set");
        c.set("journal.retention_days", "90").expect("set");
        c.save(&p).expect("save");
        assert_eq!(
            ConfigState::load(&p, Path::new(HOME)),
            ConfigState::Loaded(c)
        );
        for bad in [
            "language = \"fr\"\n",
            "language = 1\n",
            "[journal]\nretention_days = 0\n",
            "[journal]\nretention_days = \"90\"\n",
            "[journal]\nkeep = 1\n",
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
}
