//! Capa 3 de validación: justo antes de actuar (ADR-04 §5.4, RF-SEG-07).

use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use ra_plan::PlanItem;
use ra_safety::apps::AppBlock;
use ra_safety::fingerprint::{fingerprint_at, open_parent_nofollow};
use ra_safety::identity::check_flags;
use ra_safety::{
    AppProbe, ExclusionMatch, Exclusions, FlagViolation, Identity, Origin, ProtectedList,
    ProtectionReason,
};
use rustix::fs::AtFlags;
use serde::{Deserialize, Serialize};

/// Por qué se omitió un elemento al ejecutar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipCause {
    #[error("ya no existe")]
    Missing,
    #[error("hay un enlace simbólico en la ruta")]
    SymlinkInPath,
    #[error("cambió desde la revisión")]
    IdentityChanged,
    #[error("su contenido cambió desde la revisión")]
    ContentsChanged,
    #[error("{flag}")]
    Flags { flag: FlagViolation },
    #[error("es o contiene un punto de montaje")]
    CrossesDevice,
    #[error("{detail}")]
    Protected { detail: ProtectionReason },
    #[error("excluido por el usuario")]
    Excluded { detail: ExclusionMatch },
    #[error("{detail}")]
    App { detail: AppBlock },
    #[error("no se puede leer: {detail}")]
    Unreadable { detail: String },
    #[error("ya existe algo en la ruta original")]
    ConflictExists,
}

impl SkipCause {
    /// Código estable para el journal y el JSON.
    pub fn code(&self) -> &'static str {
        match self {
            SkipCause::Missing => "missing",
            SkipCause::SymlinkInPath => "symlink_in_path",
            SkipCause::IdentityChanged => "identity_changed",
            SkipCause::ContentsChanged => "contents_changed",
            SkipCause::Flags { .. } => "flags",
            SkipCause::CrossesDevice => "crosses_device",
            SkipCause::Protected { .. } => "protected",
            SkipCause::Excluded { .. } => "excluded",
            SkipCause::App { .. } => "app_running",
            SkipCause::Unreadable { .. } => "unreadable",
            SkipCause::ConflictExists => "conflict_exists",
        }
    }
}

/// Elemento verificado: carpeta padre abierta sin seguir enlaces y nombre.
pub(crate) struct Verified<'a> {
    pub parent: OwnedFd,
    pub name: &'a OsStr,
}

pub(crate) fn open_error(e: io::Error) -> SkipCause {
    match e.raw_os_error() {
        Some(libc_errno::ELOOP) => SkipCause::SymlinkInPath,
        Some(libc_errno::ENOENT) | Some(libc_errno::ENOTDIR) => SkipCause::Missing,
        _ => SkipCause::Unreadable {
            detail: e.to_string(),
        },
    }
}

/// Números de error de macOS que interesan aquí.
mod libc_errno {
    pub const ENOENT: i32 = 2;
    pub const ENOTDIR: i32 = 20;
    pub const ELOOP: i32 = 62;
}

pub(crate) struct Guards<'a> {
    pub protected: &'a ProtectedList,
    pub exclusions: &'a Exclusions,
    pub apps: &'a AppProbe,
}

pub(crate) fn verify<'p>(item: &'p PlanItem, g: &Guards<'_>) -> Result<Verified<'p>, SkipCause> {
    let path: &Path = &item.path;
    g.protected
        .check(path, Origin::Rules)
        .map_err(|detail| SkipCause::Protected { detail })?;
    if let Some(detail) = g.exclusions.check(path, &item.rule.id, &item.category) {
        return Err(SkipCause::Excluded { detail });
    }

    let (parent, name) = open_parent_nofollow(path).map_err(open_error)?;
    let st = rustix::fs::statat(parent.as_fd(), name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|e| open_error(e.into()))?;
    let now = Identity::from_stat(&st);
    if now != item.identity {
        return Err(SkipCause::IdentityChanged);
    }
    check_flags(st.st_flags).map_err(|flag| SkipCause::Flags { flag })?;
    let pst = rustix::fs::fstat(&parent).map_err(|e| SkipCause::Unreadable {
        detail: e.to_string(),
    })?;
    if pst.st_dev != st.st_dev {
        return Err(SkipCause::CrossesDevice);
    }
    let fp = fingerprint_at(parent.as_fd(), name).map_err(|e| SkipCause::Unreadable {
        detail: e.to_string(),
    })?;
    if fp.crosses_device {
        return Err(SkipCause::CrossesDevice);
    }
    if fp.digest != item.fingerprint.digest {
        return Err(SkipCause::ContentsChanged);
    }
    g.apps
        .check(&item.apps)
        .map_err(|detail| SkipCause::App { detail })?;
    Ok(Verified { parent, name })
}
