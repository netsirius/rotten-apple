//! Identidad de un fichero y comprobación de flags.
//!
//! La identidad que se captura en la revisión y se revalida justo antes de
//! actuar es `(dev, ino, tipo, tamaño, mtime, ctime)` (README §8, 05 §5.4).
//! Mover a la Papelera es un `rename`, que conserva `ino` pero actualiza
//! `ctime`; por eso, tras mover y al deshacer, se usa
//! [`Identity::same_after_move`], que no mira `ctime`.

use std::fs::Metadata;
use std::os::macos::fs::MetadataExt as _;
use std::os::unix::fs::MetadataExt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

impl FileKind {
    fn from_mode(mode: u32) -> Self {
        match mode & (libc_mode::S_IFMT) {
            libc_mode::S_IFREG => FileKind::File,
            libc_mode::S_IFDIR => FileKind::Dir,
            libc_mode::S_IFLNK => FileKind::Symlink,
            _ => FileKind::Other,
        }
    }
}

/// Constantes de `st_mode` (iguales en todos los Unix que nos importan).
mod libc_mode {
    pub const S_IFMT: u32 = 0o170000;
    pub const S_IFREG: u32 = 0o100000;
    pub const S_IFDIR: u32 = 0o040000;
    pub const S_IFLNK: u32 = 0o120000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identity {
    pub dev: u64,
    pub ino: u64,
    pub kind: FileKind,
    pub size: u64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
}

impl Identity {
    /// Desde metadatos obtenidos **sin** seguir enlaces (`symlink_metadata`).
    pub fn from_metadata(m: &Metadata) -> Self {
        let ft = m.file_type();
        let kind = if ft.is_symlink() {
            FileKind::Symlink
        } else if ft.is_dir() {
            FileKind::Dir
        } else if ft.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        Self {
            dev: m.dev(),
            ino: m.ino(),
            kind,
            size: m.size(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
            ctime: m.ctime(),
            ctime_nsec: m.ctime_nsec(),
        }
    }

    /// Desde un `stat` relativo a descriptor (`fstatat`).
    pub fn from_stat(st: &rustix::fs::Stat) -> Self {
        Self {
            dev: st.st_dev as u64,
            ino: st.st_ino,
            kind: FileKind::from_mode(u32::from(st.st_mode)),
            size: st.st_size as u64,
            mtime: st.st_mtime,
            mtime_nsec: st.st_mtime_nsec,
            ctime: st.st_ctime,
            ctime_nsec: st.st_ctime_nsec,
        }
    }

    /// Mismo objeto del sistema de ficheros (volumen, inode y tipo).
    pub fn same_object(&self, other: &Identity) -> bool {
        self.dev == other.dev && self.ino == other.ino && self.kind == other.kind
    }

    /// Mismo objeto y mismo contenido aparente, ignorando `ctime`, que cambia
    /// al renombrar (mover a la Papelera y restaurar).
    pub fn same_after_move(&self, other: &Identity) -> bool {
        self.same_object(other)
            && self.size == other.size
            && self.mtime == other.mtime
            && self.mtime_nsec == other.mtime_nsec
    }
}

// Flags de `st_flags` (sys/stat.h de macOS).
pub const UF_IMMUTABLE: u32 = 0x0000_0002;
pub const UF_APPEND: u32 = 0x0000_0004;
pub const SF_IMMUTABLE: u32 = 0x0002_0000;
pub const SF_APPEND: u32 = 0x0004_0000;
pub const SF_RESTRICTED: u32 = 0x0008_0000;
pub const SF_NOUNLINK: u32 = 0x0010_0000;
pub const SF_FIRMLINK: u32 = 0x0080_0000;
pub const SF_DATALESS: u32 = 0x4000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum FlagViolation {
    #[error("inmutable (uchg/schg)")]
    Immutable,
    #[error("solo anexar (uappnd/sappnd)")]
    AppendOnly,
    #[error("protegido por SIP (restricted)")]
    Restricted,
    #[error("no se puede desenlazar (sunlnk)")]
    NoUnlink,
    #[error("firmlink del sistema")]
    Firmlink,
    #[error("fichero sin descargar de iCloud (dataless)")]
    Dataless,
}

/// Rechaza los flags que el ejecutor nunca debe tocar (05 §1.2, RF-SEG-10).
pub fn check_flags(flags: u32) -> Result<(), FlagViolation> {
    if flags & SF_RESTRICTED != 0 {
        return Err(FlagViolation::Restricted);
    }
    if flags & (UF_IMMUTABLE | SF_IMMUTABLE) != 0 {
        return Err(FlagViolation::Immutable);
    }
    if flags & (UF_APPEND | SF_APPEND) != 0 {
        return Err(FlagViolation::AppendOnly);
    }
    if flags & SF_NOUNLINK != 0 {
        return Err(FlagViolation::NoUnlink);
    }
    if flags & SF_FIRMLINK != 0 {
        return Err(FlagViolation::Firmlink);
    }
    if flags & SF_DATALESS != 0 {
        return Err(FlagViolation::Dataless);
    }
    Ok(())
}

/// Flags de unos metadatos de `std`.
pub fn flags_of(m: &Metadata) -> u32 {
    m.st_flags()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_rename_but_ctime_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::write(&a, b"hola").expect("write");
        let before = Identity::from_metadata(&std::fs::symlink_metadata(&a).expect("stat"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::rename(&a, &b).expect("rename");
        let after = Identity::from_metadata(&std::fs::symlink_metadata(&b).expect("stat"));
        assert!(before.same_after_move(&after));
        assert_eq!(before.ino, after.ino);
    }

    #[test]
    fn std_and_rustix_agree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = dir.path().join("f");
        std::fs::write(&f, b"x").expect("write");
        let a = Identity::from_metadata(&std::fs::symlink_metadata(&f).expect("stat"));
        let st = rustix::fs::lstat(&f).expect("lstat");
        assert_eq!(a, Identity::from_stat(&st));
    }

    #[test]
    fn flags_are_checked() {
        assert!(check_flags(0).is_ok());
        assert_eq!(check_flags(UF_IMMUTABLE), Err(FlagViolation::Immutable));
        assert_eq!(
            check_flags(SF_RESTRICTED | UF_IMMUTABLE),
            Err(FlagViolation::Restricted)
        );
        assert_eq!(check_flags(SF_DATALESS), Err(FlagViolation::Dataless));
    }
}
