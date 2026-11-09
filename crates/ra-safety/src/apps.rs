//! Apps en ejecución, con semántica fail-closed (RF-SEG-08).
//!
//! Las reglas pueden exigir que una app esté cerrada (navegadores, la app
//! dueña de una caché o de un estado guardado). Si la lista de procesos no se
//! puede obtener, el estado es «no determinable» y cuenta como «en uso».

use std::collections::BTreeSet;
use std::io;

use serde::{Deserialize, Serialize};

use crate::paths::fold;

/// Fuente de bundle IDs en ejecución. Se inyecta para poder probar.
pub trait RunningApps: Send + Sync {
    fn running_bundle_ids(&self) -> io::Result<BTreeSet<String>>;
}

/// Implementación real, vía `libproc` (`ra-platform`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemApps;

impl RunningApps for SystemApps {
    fn running_bundle_ids(&self) -> io::Result<BTreeSet<String>> {
        ra_platform::running_app_bundle_ids()
    }
}

/// Lista fija, para pruebas.
impl RunningApps for BTreeSet<String> {
    fn running_bundle_ids(&self) -> io::Result<BTreeSet<String>> {
        Ok(self.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppBlock {
    #[error("the app {bundle_id} is open")]
    Running { bundle_id: String },
    #[error("cannot tell whether the app is open: {detail}")]
    Unknown { detail: String },
}

/// Instantánea de las apps abiertas, tomada una vez por escaneo o ejecución.
#[derive(Debug, Clone)]
pub struct AppProbe {
    state: Result<BTreeSet<String>, String>,
}

impl AppProbe {
    pub fn snapshot(source: &dyn RunningApps) -> Self {
        Self {
            state: source
                .running_bundle_ids()
                .map(|ids| ids.iter().map(|s| fold(s)).collect())
                .map_err(|e| e.to_string()),
        }
    }

    /// `Ok(())` si ninguna de las apps está abierta. Sin apps que comprobar,
    /// siempre `Ok(())`.
    pub fn check<S: AsRef<str>>(&self, bundle_ids: &[S]) -> Result<(), AppBlock> {
        if bundle_ids.is_empty() {
            return Ok(());
        }
        let running = self
            .state
            .as_ref()
            .map_err(|e| AppBlock::Unknown { detail: e.clone() })?;
        for id in bundle_ids {
            if running.contains(&fold(id.as_ref())) {
                return Err(AppBlock::Running {
                    bundle_id: id.as_ref().to_owned(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Broken;
    impl RunningApps for Broken {
        fn running_bundle_ids(&self) -> io::Result<BTreeSet<String>> {
            Err(io::Error::other("sin acceso"))
        }
    }

    #[test]
    fn unknown_state_blocks() {
        let p = AppProbe::snapshot(&Broken);
        assert!(matches!(p.check(&["com.a"]), Err(AppBlock::Unknown { .. })));
        assert!(p.check::<&str>(&[]).is_ok());
    }

    #[test]
    fn running_app_blocks_case_insensitively() {
        let set: BTreeSet<String> = ["com.Google.Chrome".to_owned()].into();
        let p = AppProbe::snapshot(&set);
        assert!(p.check(&["com.google.chrome"]).is_err());
        assert!(p.check(&["org.mozilla.firefox"]).is_ok());
    }
}
