//! Capa de seguridad del núcleo (ADR-04 §5.4).
//!
//! - [`protected`]: lista fail-closed incrustada en el binario (RF-SEG-04). No
//!   se puede anular desde configuración, catálogo ni plan.
//! - [`exclusions`]: exclusiones del usuario leídas de `config.toml`. Si el
//!   fichero existe pero no se puede leer o validar, el estado es «corrupto» y
//!   ninguna acción destructiva puede ejecutarse (RF-SEG-05).
//! - [`settings`]: claves de `config.toml` que cambia `rotten config set`,
//!   validadas al escribir y al leer.
//! - [`config_link`]: enlace `~/.config/rotten/config.toml`; si en su lugar
//!   hay otra cosa, no se actúa (RF-SEG-05 c).
//! - [`identity`]: identidad `(dev, ino, tipo, tamaño, mtime, ctime)` y
//!   comprobación de flags (RF-SEG-07, 05 §1.2).
//! - [`fingerprint`]: huella del contenido de una carpeta, para omitir
//!   carpetas con entradas cambiadas desde la revisión.
//! - [`apps`]: comprobación de apps en ejecución con semántica fail-closed
//!   (RF-SEG-08: «no determinable» cuenta como «en uso»).

#![forbid(unsafe_code)]

pub mod apps;
pub mod config_link;
pub mod exclusions;
pub mod fingerprint;
pub mod identity;
pub mod paths;
pub mod protected;
pub mod settings;

pub use apps::{AppProbe, RunningApps, SystemApps};
pub use config_link::{Found as LinkFound, LinkState};
pub use exclusions::{Config, ConfigState, Exclusion, ExclusionMatch, Exclusions};
pub use fingerprint::TreeFingerprint;
pub use identity::{FileKind, FlagViolation, Identity};
pub use protected::{Origin, ProtectedList, ProtectionReason};
pub use settings::{Expected, Language, SettingError, SettingValue};
