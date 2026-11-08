//! Ejecutor (ADR-04): la única parte del núcleo que modifica el sistema de
//! ficheros del usuario.
//!
//! - Solo ejecuta un [`ApprovedPlan`] y solo con el cerrojo [`ExecLock`]
//!   tomado (un ejecutor a la vez, RNF-ARQ-20).
//! - Escritura anticipada: la intención de cada elemento llega al disco
//!   antes de actuar; el resultado se anota después.
//! - Capa 3 de validación justo antes de cada acción ([`verify`]): ruta sin
//!   enlaces simbólicos, identidad `(dev, ino, tipo, tamaño, mtime, ctime)`,
//!   huella del contenido, flags, puntos de montaje, apps abiertas, ficheros
//!   abiertos por otros procesos y, otra vez, lista protegida y exclusiones.
//!   Si algo no cuadra, se **omite**.
//! - Por defecto a la Papelera; el borrado permanente es relativo a
//!   descriptor (`unlinkat`), nunca `rm -rf` por ruta.
//! - Un `--dry-run` recorre exactamente el mismo camino y sustituye la acción
//!   final por un registro (05 §5.8, RF-SEG-16).

#![forbid(unsafe_code)]

mod lock;
mod run;
mod trash;
mod verify;

pub use lock::ExecLock;
pub use ra_plan::ApprovedPlan;
pub use run::{
    ExecContext, ExecError, ExecReport, ItemReport, Outcome, dry_run, dry_run_undo, execute,
    execute_undo, recover_incomplete,
};
pub use trash::{TrashBackend, unique_name};
pub use verify::{InUseDetail, SkipCause};
