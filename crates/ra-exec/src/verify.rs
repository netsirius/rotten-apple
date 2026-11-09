//! Capa 3 de validación: justo antes de actuar (ADR-04 §5.4, RF-SEG-07).

use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use ra_plan::PlanItem;
use ra_platform::files::OpenFiles;
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
    #[error("it no longer exists")]
    Missing,
    #[error("there is a symbolic link in the path")]
    SymlinkInPath,
    #[error("it changed since the review")]
    IdentityChanged,
    #[error("its contents changed since the review")]
    ContentsChanged,
    #[error("{flag}")]
    Flags { flag: FlagViolation },
    #[error("it is or contains a mount point")]
    CrossesDevice,
    #[error("{detail}")]
    Protected { detail: ProtectionReason },
    #[error("excluded by the user")]
    Excluded { detail: ExclusionMatch },
    #[error("{detail}")]
    App { detail: AppBlock },
    #[error("cannot be read: {detail}")]
    Unreadable { detail: String },
    #[error("something already exists at the original path")]
    ConflictExists,
    #[error("{detail}")]
    InUse { detail: InUseDetail },
    /// Al borrar de la Papelera: la ruta no está en el primer nivel de una
    /// carpeta de la Papelera que se usa ahora.
    #[error("it is not in the Trash in use")]
    OutsideTrash,
}

/// Detalle de [`SkipCause::InUse`] (RF-SEG-08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InUseDetail {
    #[error("in use by {process} (pid {pid})")]
    Held { pid: i32, process: String },
    /// La lista de ficheros abiertos no se pudo obtener: cuenta como «en uso».
    #[error("cannot tell whether it is in use: {detail}")]
    Unknown { detail: String },
}

impl SkipCause {
    /// Código estable para el journal y el JSON: el mismo que la etiqueta
    /// `reason` de la serialización.
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
            SkipCause::App { .. } => "app",
            SkipCause::Unreadable { .. } => "unreadable",
            SkipCause::ConflictExists => "conflict_exists",
            SkipCause::InUse { .. } => "in_use",
            SkipCause::OutsideTrash => "outside_trash",
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
    /// Instantánea de los ficheros abiertos, tomada una vez por pasada.
    pub open: Result<OpenFiles, String>,
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
    // Lo último: si el elemento cambió, el motivo es el cambio, no el uso.
    let open = g.open.as_ref().map_err(|e| SkipCause::InUse {
        detail: InUseDetail::Unknown { detail: e.clone() },
    })?;
    if let Some(h) = open.holder(path, now.dev, now.ino) {
        return Err(SkipCause::InUse {
            detail: InUseDetail::Held {
                pid: h.pid,
                process: h.process,
            },
        });
    }
    Ok(Verified { parent, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El código del journal y la etiqueta `reason` del JSON no pueden
    /// divergir (antes, `app_running` frente a `app`).
    #[test]
    fn code_matches_the_serialized_reason() {
        let all = [
            SkipCause::Missing,
            SkipCause::SymlinkInPath,
            SkipCause::IdentityChanged,
            SkipCause::ContentsChanged,
            SkipCause::Flags {
                flag: FlagViolation::Immutable,
            },
            SkipCause::CrossesDevice,
            SkipCause::Protected {
                detail: ProtectionReason::Home,
            },
            SkipCause::Excluded {
                detail: ExclusionMatch::Rule("r".into()),
            },
            SkipCause::App {
                detail: AppBlock::Running {
                    bundle_id: "com.example".into(),
                },
            },
            SkipCause::Unreadable { detail: "x".into() },
            SkipCause::ConflictExists,
            SkipCause::InUse {
                detail: InUseDetail::Held {
                    pid: 1,
                    process: "p".into(),
                },
            },
        ];
        for cause in all {
            let v = serde_json::to_value(&cause).expect("serializar");
            assert_eq!(v["reason"], cause.code(), "{cause:?}");
        }
    }
}
