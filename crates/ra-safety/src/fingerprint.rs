//! Huella del contenido de un elemento (RF-SEG-07).
//!
//! La identidad de una carpeta solo cambia cuando cambian sus hijos
//! directos. Para omitir carpetas con entradas modificadas a cualquier
//! profundidad se calcula una huella BLAKE3 sobre `(ruta relativa, tipo, ino,
//! tamaño, mtime, ctime)` de cada descendiente, en orden determinista.
//!
//! El recorrido es relativo a descriptor y nunca sigue enlaces simbólicos:
//! cada carpeta se abre con `O_NOFOLLOW` desde la anterior, y la carpeta
//! padre del elemento con `O_NOFOLLOW_ANY` (falla si cualquier componente de
//! la ruta es un enlace). No cruza puntos de montaje: si encuentra uno lo
//! marca en [`TreeFingerprint::crosses_device`] y no desciende.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

use rustix::fs::{AtFlags, Dir, Mode, OFlags};
use serde::{Deserialize, Serialize};

use crate::identity::{FileKind, Identity};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeFingerprint {
    /// Número de entradas, incluido el propio elemento.
    pub entries: u64,
    /// Bytes asignados en disco (`st_blocks × 512`), enlaces duros una vez.
    pub bytes_allocated: u64,
    /// Bytes lógicos (`st_size`), enlaces duros una vez.
    pub bytes_logical: u64,
    /// `mtime` más reciente de todo el árbol (segundos desde la época).
    pub newest_mtime: i64,
    /// Hay algún punto de montaje dentro.
    pub crosses_device: bool,
    /// `blake3:<hex>`.
    pub digest: String,
}

/// Abre la carpeta padre de `path` sin seguir ningún enlace simbólico.
pub fn open_parent_nofollow(path: &Path) -> io::Result<(OwnedFd, &OsStr)> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sin carpeta padre"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sin nombre"))?;
    let fd = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW_ANY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    Ok((fd, name))
}

/// `lstat` de `path` que falla si algún componente de la ruta es un enlace
/// simbólico (el último puede serlo: se describe el enlace, no su destino).
/// Devuelve la identidad y los flags (`st_flags`).
pub fn stat_nofollow_any(path: &Path) -> io::Result<(Identity, u32)> {
    let (parent, name) = open_parent_nofollow(path)?;
    let st = rustix::fs::statat(parent.as_fd(), name, AtFlags::SYMLINK_NOFOLLOW)?;
    Ok((Identity::from_stat(&st), st.st_flags))
}

/// Huella de `path` (fichero, enlace o carpeta).
pub fn fingerprint_path(path: &Path) -> io::Result<TreeFingerprint> {
    let (parent, name) = open_parent_nofollow(path)?;
    fingerprint_at(parent.as_fd(), name)
}

/// Huella de la entrada `name` dentro de la carpeta `parent`.
pub fn fingerprint_at(parent: BorrowedFd<'_>, name: &OsStr) -> io::Result<TreeFingerprint> {
    let st = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let root = Identity::from_stat(&st);
    let mut acc = Acc {
        hasher: blake3::Hasher::new(),
        seen: HashSet::new(),
        root_dev: root.dev,
        out: TreeFingerprint {
            entries: 0,
            bytes_allocated: 0,
            bytes_logical: 0,
            newest_mtime: i64::MIN,
            crosses_device: false,
            digest: String::new(),
        },
    };
    acc.add(b"", &root, st.st_blocks as u64);
    if root.kind == FileKind::Dir {
        let fd = open_dir_at(parent, name)?;
        acc.walk(fd.as_fd(), &mut Vec::new())?;
    }
    acc.out.digest = format!("blake3:{}", acc.hasher.finalize().to_hex());
    Ok(acc.out)
}

fn open_dir_at(parent: BorrowedFd<'_>, name: impl rustix::path::Arg) -> io::Result<OwnedFd> {
    Ok(rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?)
}

struct Acc {
    hasher: blake3::Hasher,
    seen: HashSet<(u64, u64)>,
    root_dev: u64,
    out: TreeFingerprint,
}

impl Acc {
    fn add(&mut self, rel: &[u8], id: &Identity, blocks: u64) {
        self.out.entries += 1;
        self.out.newest_mtime = self.out.newest_mtime.max(id.mtime);
        if self.seen.insert((id.dev, id.ino)) {
            self.out.bytes_allocated += blocks * 512;
            self.out.bytes_logical += id.size;
        }
        let h = &mut self.hasher;
        h.update(&(rel.len() as u64).to_le_bytes());
        h.update(rel);
        h.update(&[id.kind as u8]);
        for v in [id.ino, id.size] {
            h.update(&v.to_le_bytes());
        }
        for v in [id.mtime, id.mtime_nsec, id.ctime, id.ctime_nsec] {
            h.update(&v.to_le_bytes());
        }
    }

    fn walk(&mut self, dir: BorrowedFd<'_>, rel: &mut Vec<u8>) -> io::Result<()> {
        let mut names: Vec<Vec<u8>> = Vec::new();
        for entry in Dir::read_from(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            names.push(name.to_vec());
        }
        names.sort();
        for name in names {
            let cname = std::ffi::CString::new(name.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "NUL en nombre"))?;
            let st = rustix::fs::statat(dir, cname.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
            let id = Identity::from_stat(&st);
            let len = rel.len();
            if !rel.is_empty() {
                rel.push(b'/');
            }
            rel.extend_from_slice(&name);
            self.add(rel, &id, st.st_blocks as u64);
            if id.kind == FileKind::Dir {
                if id.dev != self.root_dev {
                    self.out.crosses_device = true;
                } else {
                    let sub = open_dir_at(dir, cname.as_c_str())?;
                    self.walk(sub.as_fd(), rel)?;
                }
            }
            rel.truncate(len);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("cache");
        fs::create_dir_all(root.join("a/b")).expect("mkdir");
        fs::write(root.join("a/b/deep.bin"), vec![0u8; 5000]).expect("write");
        fs::write(root.join("top.txt"), b"hola").expect("write");
        dir
    }

    fn canon(p: &Path) -> std::path::PathBuf {
        fs::canonicalize(p).expect("canonicalize")
    }

    #[test]
    fn stable_when_nothing_changes() {
        let t = tree();
        let p = canon(t.path()).join("cache");
        let a = fingerprint_path(&p).expect("fp");
        let b = fingerprint_path(&p).expect("fp");
        assert_eq!(a, b);
        assert_eq!(a.entries, 5);
        assert!(a.bytes_logical >= 5004);
    }

    #[test]
    fn deep_modification_changes_digest() {
        let t = tree();
        let p = canon(t.path()).join("cache");
        let a = fingerprint_path(&p).expect("fp");
        fs::write(p.join("a/b/deep.bin"), vec![1u8; 5001]).expect("write");
        let b = fingerprint_path(&p).expect("fp");
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn new_nested_file_changes_digest() {
        let t = tree();
        let p = canon(t.path()).join("cache");
        let a = fingerprint_path(&p).expect("fp");
        fs::write(p.join("a/new"), b"").expect("write");
        assert_ne!(a.digest, fingerprint_path(&p).expect("fp").digest);
    }

    #[test]
    fn symlinks_are_not_followed() {
        let t = tree();
        let base = canon(t.path());
        let outside = base.join("outside");
        fs::create_dir(&outside).expect("mkdir");
        fs::write(outside.join("secret"), b"x").expect("write");
        std::os::unix::fs::symlink(&outside, base.join("cache/link")).expect("symlink");
        let fp = fingerprint_path(&base.join("cache")).expect("fp");
        // El enlace cuenta como una entrada; su destino no se recorre.
        assert_eq!(fp.entries, 6);
    }

    #[test]
    fn symlinked_parent_is_rejected() {
        let t = tree();
        let base = canon(t.path());
        std::os::unix::fs::symlink(base.join("cache"), base.join("alias")).expect("symlink");
        assert!(fingerprint_path(&base.join("alias/top.txt")).is_err());
    }
}
