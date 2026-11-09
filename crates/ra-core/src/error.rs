//! Errores de la fachada, ya clasificados para los códigos de salida.
//!
//! Cada error tiene un código estable ([`CoreError::code`]) y un mensaje en
//! inglés (su `Display`): el JSON del CLI lleva los dos y no depende del
//! idioma (RNF-25). Las variantes concretas llevan sus datos por separado
//! para que el CLI pueda traducir el mensaje; las genéricas (`Usage`,
//! `Safety`…) quedan para errores sin variante propia.

use std::path::PathBuf;

use ra_history::{FreeError, ItemRef, UndoError};
use ra_plan::PlanError;
use ra_safety::ProtectionReason;

/// Clase del error: decide el código de salida (07 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Uso incorrecto (`2`).
    Usage,
    /// Falta un permiso (`3`).
    Permission,
    /// Abortado por seguridad o plan cambiado (`5`).
    Safety,
    /// Otro ejecutor activo (`6`).
    Locked,
    /// No existe lo pedido (`1`).
    NotFound,
    /// Cualquier otro error (`1`).
    Other,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Permission(String),
    /// Abortado por seguridad o plan cambiado (código 5).
    #[error("{0}")]
    Safety(String),
    /// Otro ejecutor activo (código 6).
    #[error("another Rotten Apple process is running an operation")]
    Locked,
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),

    #[error("unknown category: {0}")]
    UnknownCategory(String),
    #[error("invalid exclusion: {0}")]
    InvalidExclusion(String),
    #[error("give at least one path")]
    NoPaths,
    #[error("the path must be absolute: {}", .0.display())]
    RelativePath(PathBuf),
    /// `config.toml` no se puede usar: no se actúa ni se reescribe.
    #[error("{} is corrupt ({reason}): nothing is done until it is fixed", path.display())]
    ConfigCorrupt { path: PathBuf, reason: String },
    #[error("the plan belongs to another home folder ({})", .0.display())]
    WrongHome(PathBuf),
    /// Hash distinto, revisión caducada o plan ilegible.
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// Selección explícita de una ruta protegida: no se mueve nada.
    #[error("{path}: {reason}. Nothing is moved.")]
    SelectionProtected {
        path: String,
        reason: ProtectionReason,
    },
    /// Selección explícita de un enlace a una carpeta del usuario.
    #[error("{path}: it is a link to {}, a user folder that cannot be chosen itself. Nothing is moved.", root.display())]
    SelectionUserFolderLink { path: String, root: PathBuf },
    #[error("cannot resolve {path}: {detail}. Nothing is moved.")]
    Unresolvable { path: String, detail: String },
    #[error("cannot classify {path}: {detail}. Nothing is moved.")]
    Unclassifiable { path: String, detail: String },
    #[error("there is no operation to undo")]
    NothingToUndo,
    #[error(transparent)]
    Undo(#[from] UndoError),
    #[error(transparent)]
    Free(#[from] FreeError),
    /// `rotten free`: el plan que se prepara al ejecutar no es el revisado.
    /// `left`: los elementos revisados que ya no están en él (p. ej. porque
    /// su volumen se desmontó).
    #[error(
        "the plan changed since the review{}. Nothing is deleted.",
        left_list(left)
    )]
    FreePlanChanged { left: Vec<ItemRef> },
    /// `rotten trash empty`: una carpeta de la Papelera revisada ya no
    /// existe (volumen desmontado).
    #[error("the Trash folder {} is no longer there (was its volume unmounted?). Nothing is deleted.", path.display())]
    TrashGone { path: PathBuf },
    /// La Papelera no se puede leer (sin Acceso total al disco): no se
    /// adivina ni se borra nada (CU-08 A1).
    #[error("cannot read the Trash ({}): grant Full Disk Access to your terminal and run `rotten doctor`. Nothing is deleted.", path.display())]
    TrashUnreadable { path: PathBuf },
    /// `rotten trash empty` sin poder leer alguna carpeta de la Papelera:
    /// no se vacía a medias; lo de Rotten Apple se puede liberar con
    /// `rotten free`, que no necesita listar la Papelera.
    #[error("cannot read the Trash ({}): grant Full Disk Access to your terminal and run `rotten doctor`. Nothing is deleted. To free only what Rotten Apple sent there, use `rotten free <op-id>`.", path.display())]
    EmptyUnreadable { path: PathBuf },
}

impl CoreError {
    pub fn class(&self) -> ErrorClass {
        use CoreError as E;
        match self {
            E::Usage(_)
            | E::UnknownCategory(_)
            | E::InvalidExclusion(_)
            | E::NoPaths
            | E::RelativePath(_) => ErrorClass::Usage,
            E::Permission(_) | E::TrashUnreadable { .. } | E::EmptyUnreadable { .. } => {
                ErrorClass::Permission
            }
            E::Free(FreeError::NoOperations) => ErrorClass::Usage,
            E::Safety(_)
            | E::ConfigCorrupt { .. }
            | E::WrongHome(_)
            | E::Plan(_)
            | E::SelectionProtected { .. }
            | E::SelectionUserFolderLink { .. }
            | E::Unresolvable { .. }
            | E::Unclassifiable { .. }
            | E::FreePlanChanged { .. }
            | E::TrashGone { .. } => ErrorClass::Safety,
            E::Locked => ErrorClass::Locked,
            E::NotFound(_) | E::NothingToUndo | E::Undo(_) | E::Free(_) => ErrorClass::NotFound,
            E::Other(_) => ErrorClass::Other,
        }
    }

    /// Código estable para el JSON (`error.code`).
    pub fn code(&self) -> &'static str {
        use CoreError as E;
        match self {
            E::Usage(_) => "usage",
            E::Permission(_) => "permission",
            E::Safety(_) => "safety",
            E::Locked => "locked",
            E::NotFound(_) => "not_found",
            E::Other(_) => "error",
            E::UnknownCategory(_) => "unknown_category",
            E::InvalidExclusion(_) => "invalid_exclusion",
            E::NoPaths => "no_paths",
            E::RelativePath(_) => "relative_path",
            E::ConfigCorrupt { .. } => "config_corrupt",
            E::WrongHome(_) => "wrong_home",
            E::Plan(PlanError::HashMismatch { .. }) => "plan_changed",
            E::Plan(PlanError::Expired { .. }) => "plan_expired",
            E::Plan(PlanError::Invalid(_)) => "plan_invalid",
            E::SelectionProtected { .. } => "selection_protected",
            E::SelectionUserFolderLink { .. } => "selection_user_folder_link",
            E::Unresolvable { .. } => "unresolvable_path",
            E::Unclassifiable { .. } => "unclassifiable_path",
            E::NothingToUndo => "nothing_to_undo",
            E::Undo(UndoError::UnknownOperation { .. }) => "unknown_operation",
            E::Undo(UndoError::NotUndoable { .. }) => "not_undoable",
            E::Undo(UndoError::UnknownItem { .. }) => "unknown_item",
            E::Free(FreeError::NoOperations) => "no_operations",
            E::Free(FreeError::UnknownOperation { .. }) => "unknown_operation",
            E::Free(FreeError::NotFreeable { .. }) => "not_freeable",
            E::TrashUnreadable { .. } | E::EmptyUnreadable { .. } => "trash_unreadable",
            E::FreePlanChanged { .. } => "plan_changed",
            E::TrashGone { .. } => "trash_gone",
        }
    }
}

/// `; no longer in it: op#1, op#2` (vacío si no salió nada).
fn left_list(left: &[ItemRef]) -> String {
    if left.is_empty() {
        return String::new();
    }
    let ids: Vec<String> = left
        .iter()
        .map(|r| format!("{}#{}", r.op, r.item))
        .collect();
    format!("; no longer in it: {}", ids.join(", "))
}

impl From<ra_exec::ExecError> for CoreError {
    fn from(e: ra_exec::ExecError) -> Self {
        CoreError::Other(e.to_string())
    }
}

impl From<ra_history::JournalError> for CoreError {
    fn from(e: ra_history::JournalError) -> Self {
        CoreError::Other(e.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Other(e.to_string())
    }
}
