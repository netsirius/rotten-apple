//! Errores de la fachada, ya clasificados para los códigos de salida.
//!
//! Cada error tiene un código estable ([`CoreError::code`]) y un mensaje en
//! inglés (su `Display`): el JSON del CLI lleva los dos y no depende del
//! idioma (RNF-25). Las variantes concretas llevan sus datos por separado
//! para que el CLI pueda traducir el mensaje; las genéricas (`Usage`,
//! `Safety`…) quedan para errores sin variante propia.

use std::path::PathBuf;

use ra_history::UndoError;
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
            E::Permission(_) => ErrorClass::Permission,
            E::Safety(_)
            | E::ConfigCorrupt { .. }
            | E::WrongHome(_)
            | E::Plan(_)
            | E::SelectionProtected { .. }
            | E::SelectionUserFolderLink { .. }
            | E::Unresolvable { .. }
            | E::Unclassifiable { .. } => ErrorClass::Safety,
            E::Locked => ErrorClass::Locked,
            E::NotFound(_) | E::NothingToUndo | E::Undo(_) => ErrorClass::NotFound,
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
        }
    }
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
