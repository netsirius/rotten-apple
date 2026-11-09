//! Journal, manifiestos e historial (ADR-04 §5.6, §5.7; RF-HIS).
//!
//! El journal JSONL es la **fuente de verdad**: cada operación escribe la
//! intención de cada elemento (con `fsync`) antes de actuar y el resultado
//! después. El manifiesto de deshacer de una operación es una proyección del
//! journal (sus intenciones, resultados y los deshaceres posteriores); se
//! guarda en `manifests/<op>.json` como copia, pero siempre se puede
//! reconstruir.
//!
//! Este crate no mueve ficheros: prepara el deshacer ([`prepare_undo`]) y
//! `ra-exec` lo ejecuta. Solo escribe en el directorio de estado de la app.

#![forbid(unsafe_code)]

mod journal;
mod manifest;

pub use journal::{
    Event, Journal, JournalError, OpKind, OpMode, OpStatus, Record, StepAction, read_journal,
};
pub use manifest::{
    HistoryKind, ItemState, Manifest, ManifestItem, NotRecoverable, NotRecoverableReason,
    OpSummary, UndoError, UndoItem, UndoPlan, UndoState,
    list_ops, manifest_for, prepare_undo,
};

use std::path::{Path, PathBuf};

/// Rutas del estado persistente de historial (05 §4.7, README §8).
#[derive(Debug, Clone)]
pub struct HistoryPaths {
    pub journal_dir: PathBuf,
    pub manifests_dir: PathBuf,
}

impl HistoryPaths {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            journal_dir: state_dir.join("journal"),
            manifests_dir: state_dir.join("manifests"),
        }
    }
}
