//! Motivos y errores del núcleo en el idioma del usuario. El JSON lleva
//! los mismos datos con códigos estables y mensajes en inglés; aquí se
//! traducen a partir de los datos tipados, no de esos mensajes.

use std::path::Path;

use ra_core::{
    AppBlock, Block, CoreError, EnvError, ExclusionMatch, FlagViolation, FreeError, InUseDetail,
    NotRecoverableReason, Outcome, PlanError, ProtectionReason, SkipCause, SkipReason, UndoError,
    tilde,
};

use crate::i18n::{I18n, tr};

fn path(p: &Path, home: &Path) -> String {
    tilde(p, home)
}

pub fn flag(i: &I18n, f: &FlagViolation) -> String {
    match f {
        FlagViolation::Immutable => tr!(i, "flag-immutable"),
        FlagViolation::AppendOnly => tr!(i, "flag-append-only"),
        FlagViolation::Restricted => tr!(i, "flag-restricted"),
        FlagViolation::NoUnlink => tr!(i, "flag-no-unlink"),
        FlagViolation::Firmlink => tr!(i, "flag-firmlink"),
        FlagViolation::Dataless => tr!(i, "flag-dataless"),
    }
}

pub fn protection(i: &I18n, r: &ProtectionReason, home: &Path) -> String {
    match r {
        ProtectionReason::InsideProtected { root } => {
            tr!(i, "protected-inside", root = path(root, home))
        }
        ProtectionReason::AncestorOfProtected { root } => {
            tr!(i, "protected-ancestor", root = path(root, home))
        }
        ProtectionReason::UserFolder { root } => {
            tr!(i, "protected-user-folder", root = path(root, home))
        }
        ProtectionReason::UserFolderRoot { root } => {
            tr!(i, "protected-user-folder-root", root = path(root, home))
        }
        ProtectionReason::Home => tr!(i, "protected-home"),
        ProtectionReason::VolumeRoot => tr!(i, "protected-volume-root"),
        ProtectionReason::AppState => tr!(i, "protected-app-state"),
        ProtectionReason::ProtectedBundle => tr!(i, "protected-bundle"),
        ProtectionReason::InvalidPath { detail } => {
            tr!(i, "protected-invalid-path", detail = detail.as_str())
        }
        ProtectionReason::OutsideHome => tr!(i, "protected-outside-home"),
    }
}

pub fn exclusion(i: &I18n, m: &ExclusionMatch) -> String {
    match m {
        ExclusionMatch::Path(v) => tr!(i, "excluded-path", value = v.as_str()),
        ExclusionMatch::ContainsExcludedPath(v) => {
            tr!(i, "excluded-contains-path", value = v.as_str())
        }
        ExclusionMatch::Glob(v) => tr!(i, "excluded-glob", value = v.as_str()),
        ExclusionMatch::ContainsGlobMatch(v) => {
            tr!(i, "excluded-contains-glob", value = v.as_str())
        }
        ExclusionMatch::Rule(v) => tr!(i, "excluded-rule", value = v.as_str()),
        ExclusionMatch::Category(v) => tr!(i, "excluded-category", value = v.as_str()),
        ExclusionMatch::BundleId(v) => tr!(i, "excluded-bundle", value = v.as_str()),
    }
}

pub fn app(i: &I18n, a: &AppBlock) -> String {
    match a {
        AppBlock::Running { bundle_id } => tr!(i, "app-running", bundle = bundle_id.as_str()),
        AppBlock::Unknown { detail } => tr!(i, "app-unknown", detail = detail.as_str()),
    }
}

pub fn block(i: &I18n, b: &Block) -> String {
    match b {
        Block::TooRecent {
            min_age_days,
            age_days,
        } => tr!(
            i,
            "block-too-recent",
            age = (*age_days).max(0),
            min = *min_age_days
        ),
        Block::App { detail } => app(i, detail),
        Block::Flags { flag: f } => flag(i, f),
        Block::ContainsNeverName { name } => tr!(i, "block-never-name", name = name.as_str()),
        Block::CrossesDevice => tr!(i, "crosses-device"),
        Block::Unreadable { detail } => tr!(i, "unreadable", detail = detail.as_str()),
    }
}

/// Por qué un candidato no entró en el plan.
pub fn skip_reason(i: &I18n, r: &SkipReason, home: &Path) -> String {
    match r {
        SkipReason::Blocked(b) => block(i, b),
        SkipReason::Protected(p) => protection(i, p, home),
        SkipReason::Excluded(m) => exclusion(i, m),
        SkipReason::Overlaps(p) => tr!(i, "skip-overlaps", path = path(p, home)),
    }
}

/// Por qué se omitió un elemento al ejecutar (o se omitiría).
pub fn skip_cause(i: &I18n, c: &SkipCause, home: &Path) -> String {
    match c {
        SkipCause::Missing => tr!(i, "skip-missing"),
        SkipCause::SymlinkInPath => tr!(i, "skip-symlink-in-path"),
        SkipCause::IdentityChanged => tr!(i, "skip-identity-changed"),
        SkipCause::ContentsChanged => tr!(i, "skip-contents-changed"),
        SkipCause::Flags { flag: f } => flag(i, f),
        SkipCause::CrossesDevice => tr!(i, "crosses-device"),
        SkipCause::Protected { detail } => protection(i, detail, home),
        SkipCause::Excluded { detail } => exclusion(i, detail),
        SkipCause::App { detail } => app(i, detail),
        SkipCause::Unreadable { detail } => tr!(i, "unreadable", detail = detail.as_str()),
        SkipCause::ConflictExists => tr!(i, "skip-conflict-exists"),
        SkipCause::InUse {
            detail: InUseDetail::Held { pid, process },
        } => tr!(
            i,
            "skip-in-use",
            process = process.as_str(),
            pid = pid.to_string()
        ),
        SkipCause::InUse {
            detail: InUseDetail::Unknown { detail },
        } => tr!(i, "skip-in-use-unknown", detail = detail.as_str()),
        SkipCause::OutsideTrash => tr!(i, "skip-outside-trash"),
    }
}

/// El problema de un elemento omitido o fallido; `None` si no lo hubo.
pub fn problem(i: &I18n, o: &Outcome, home: &Path) -> Option<String> {
    match o {
        Outcome::Skipped { cause, .. } => Some(skip_cause(i, cause, home)),
        // El error de la operación (E/S) viene del sistema, tal cual.
        Outcome::Failed { error } => Some(tr!(i, "failed", error = error.as_str())),
        _ => None,
    }
}

pub fn not_recoverable(i: &I18n, r: &NotRecoverableReason) -> String {
    match r {
        NotRecoverableReason::NotInTrash => tr!(i, "not-recoverable-not-in-trash"),
        NotRecoverableReason::Deleted => tr!(i, "not-recoverable-deleted"),
        NotRecoverableReason::AlreadyRestored => tr!(i, "not-recoverable-already-restored"),
        NotRecoverableReason::UnknownWhereabouts { detail: None } => {
            tr!(i, "not-recoverable-unknown")
        }
        NotRecoverableReason::UnknownWhereabouts { detail: Some(d) } => {
            tr!(i, "not-recoverable-unknown-detail", detail = d.as_str())
        }
        NotRecoverableReason::NeverMoved => tr!(i, "not-recoverable-never-moved"),
        NotRecoverableReason::OtherTrash => tr!(i, "not-recoverable-other-trash"),
        NotRecoverableReason::VolumeNotMounted => tr!(i, "not-recoverable-volume-not-mounted"),
    }
}

/// Un error del núcleo para personas. Las variantes genéricas llevan ya su
/// texto (en inglés); las demás se traducen por sus datos.
pub fn core_error(i: &I18n, e: &CoreError, home: &Path) -> String {
    use CoreError as E;
    match e {
        E::Usage(m) | E::Permission(m) | E::Safety(m) | E::NotFound(m) | E::Other(m) => m.clone(),
        E::Locked => tr!(i, "error-locked"),
        E::UnknownCategory(c) => tr!(i, "error-unknown-category", category = c.as_str()),
        E::InvalidExclusion(d) => tr!(i, "error-invalid-exclusion", detail = d.as_str()),
        E::NoPaths => tr!(i, "error-no-paths"),
        E::RelativePath(p) => tr!(i, "error-relative-path", path = p.display().to_string()),
        E::ConfigCorrupt { path: p, reason } => tr!(
            i,
            "error-config-corrupt",
            path = path(p, home),
            reason = reason.as_str()
        ),
        E::WrongHome(p) => tr!(i, "error-wrong-home", path = p.display().to_string()),
        E::Plan(PlanError::HashMismatch { .. }) => tr!(i, "error-plan-changed"),
        E::Plan(PlanError::Expired { age_secs }) => {
            tr!(i, "error-plan-expired", minutes = age_secs / 60)
        }
        E::Plan(PlanError::Invalid(d)) => tr!(i, "error-plan-invalid", detail = d.as_str()),
        E::SelectionProtected { path: p, reason } => tr!(
            i,
            "error-selection-protected",
            path = p.as_str(),
            reason = protection(i, reason, home)
        ),
        E::SelectionUserFolderLink { path: p, root } => tr!(
            i,
            "error-selection-user-folder-link",
            path = p.as_str(),
            root = path(root, home)
        ),
        E::Unresolvable { path: p, detail } => tr!(
            i,
            "error-unresolvable",
            path = p.as_str(),
            detail = detail.as_str()
        ),
        E::Unclassifiable { path: p, detail } => tr!(
            i,
            "error-unclassifiable",
            path = p.as_str(),
            detail = detail.as_str()
        ),
        E::NothingToUndo => tr!(i, "error-nothing-to-undo"),
        E::Undo(UndoError::UnknownOperation { op }) => {
            tr!(i, "error-unknown-operation", op = op.as_str())
        }
        E::Undo(UndoError::NotUndoable { op }) => tr!(i, "error-not-undoable", op = op.as_str()),
        E::Undo(UndoError::UnknownItem { op, item }) => tr!(
            i,
            "error-unknown-item",
            op = op.as_str(),
            item = item.to_string()
        ),
        E::Free(FreeError::NoOperations) => tr!(i, "error-no-operations"),
        E::Free(FreeError::UnknownOperation { op }) => {
            tr!(i, "error-unknown-operation", op = op.as_str())
        }
        E::Free(FreeError::NotFreeable { op }) => tr!(i, "error-not-freeable", op = op.as_str()),
        E::TrashUnreadable { path: p } => tr!(i, "error-trash-unreadable", path = path(p, home)),
        E::EmptyUnreadable { path: p } => tr!(i, "error-empty-unreadable", path = path(p, home)),
    }
}

pub fn env_error(i: &I18n, e: &EnvError) -> String {
    match e {
        EnvError::NoHome => tr!(i, "error-no-home"),
        EnvError::BadPath { var, path, detail } => tr!(
            i,
            "error-env-path",
            var = *var,
            path = path.as_str(),
            detail = detail.as_str()
        ),
    }
}
