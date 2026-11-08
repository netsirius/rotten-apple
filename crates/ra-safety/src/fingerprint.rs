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
//!
//! El mismo recorrido mide lo que se liberaría al borrar
//! ([`TreeFingerprint::size`]): `ATTR_CMNEXT_PRIVATESIZE` de cada fichero
//! (05 §1.5.2), enlaces duros una vez, y los inodes que otro elemento puede
//! compartir (enlaces duros y clones completos) para que `scan` los cuente
//! una sola vez entre elementos (RF-SEG-12, RNF-ARQ-28).

use std::collections::HashSet;
use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use ra_platform::attrs::{FileAttrs, file_attrs_at};
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
    /// Tamaño liberable. No se serializa: no forma parte del plan ni de su
    /// hash, y cambia sin que cambie el contenido (al clonar o borrar otra
    /// copia); solo sirve para la estimación de `scan`.
    #[serde(skip)]
    pub size: SizeDetail,
}

/// Lo que se liberaría al borrar un elemento (RF-SEG-12, RNF-ARQ-28).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SizeDetail {
    /// Suma, una vez por inode, de los bytes que se liberarían de inmediato:
    /// `ATTR_CMNEXT_PRIVATESIZE` de los ficheros (0 para un clon completo;
    /// `scan` suma la familia entera cuando todos sus miembros están en la
    /// selección) y `st_blocks × 512` de lo demás o donde el volumen no da
    /// el atributo.
    pub bytes_private: u64,
    /// Ficheros que pueden compartir bloques con un clon APFS.
    pub cloned_entries: u64,
    /// Ficheros con más de un enlace duro (`st_nlink > 1`).
    pub hardlinked_entries: u64,
    /// Inodes que pueden aparecer también en otro elemento: enlaces duros y
    /// miembros de una familia de clones completos.
    pub shared: Vec<SharedInode>,
}

/// Inode que `scan` cuenta una sola vez entre elementos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedInode {
    pub dev: u64,
    pub ino: u64,
    /// Lo que aportó a [`TreeFingerprint::bytes_allocated`].
    pub allocated: u64,
    /// Lo que aportó a [`SizeDetail::bytes_private`].
    pub private: u64,
    /// Familia de clones completos a la que pertenece, si la hay.
    pub clone: Option<CloneFamily>,
}

/// Familia de clones completos (`ATTR_CMNEXT_CLONEID`): `members` es
/// `ATTR_CMNEXT_CLONE_REFCNT`, que cuenta los inodes que comparten todos sus
/// bloques, incluido el propio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CloneFamily {
    pub id: u64,
    pub members: u32,
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
            size: SizeDetail::default(),
        },
    };
    let attrs = CString::new(name.as_bytes())
        .map(|c| file_attrs(parent, &c, root.kind))
        .unwrap_or_default();
    acc.add(b"", &root, &st, attrs);
    if root.kind == FileKind::Dir {
        let fd = open_dir_at(parent, name)?;
        acc.walk(fd.as_fd(), &mut Vec::new())?;
    }
    acc.out.digest = format!("blake3:{}", acc.hasher.finalize().to_hex());
    Ok(acc.out)
}

/// Atributos de tamaño de un fichero regular. Si no se pueden leer, se mide
/// con `st_blocks × 512`: la estimación no debe bloquear el elemento.
fn file_attrs(dir: BorrowedFd<'_>, name: &CStr, kind: FileKind) -> FileAttrs {
    if kind != FileKind::File {
        return FileAttrs::default();
    }
    file_attrs_at(dir, name).unwrap_or_default()
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
    fn add(&mut self, rel: &[u8], id: &Identity, st: &rustix::fs::Stat, attrs: FileAttrs) {
        self.out.entries += 1;
        self.out.newest_mtime = self.out.newest_mtime.max(id.mtime);
        if self.seen.insert((id.dev, id.ino)) {
            let allocated = st.st_blocks as u64 * 512;
            let private = attrs.private_size.unwrap_or(allocated);
            self.out.bytes_allocated += allocated;
            self.out.bytes_logical += id.size;
            let size = &mut self.out.size;
            size.bytes_private += private;
            let hardlinked = id.kind == FileKind::File && st.st_nlink > 1;
            size.hardlinked_entries += u64::from(hardlinked);
            size.cloned_entries += u64::from(attrs.may_share_blocks());
            // Solo los clones completos con otro miembro forman familia.
            let clone = match (attrs.clone_id, attrs.clone_refcnt) {
                (Some(id), Some(members)) if members > 1 && attrs.may_share_blocks() => {
                    Some(CloneFamily { id, members })
                }
                _ => None,
            };
            if hardlinked || clone.is_some() {
                size.shared.push(SharedInode {
                    dev: id.dev,
                    ino: id.ino,
                    allocated,
                    private,
                    clone,
                });
            }
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
            let cname = CString::new(name.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "NUL en nombre"))?;
            let st = rustix::fs::statat(dir, cname.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
            let id = Identity::from_stat(&st);
            let len = rel.len();
            if !rel.is_empty() {
                rel.push(b'/');
            }
            rel.extend_from_slice(&name);
            let attrs = file_attrs(dir, cname.as_c_str(), id.kind);
            self.add(rel, &id, &st, attrs);
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
    fn plain_files_free_what_they_occupy() {
        let t = tree();
        let fp = fingerprint_path(&canon(t.path()).join("cache")).expect("fp");
        assert_eq!(fp.size.bytes_private, fp.bytes_allocated);
        assert_eq!(fp.size.cloned_entries, 0);
        assert!(fp.size.shared.is_empty());
    }

    #[test]
    fn full_clones_free_nothing_on_their_own() {
        let t = tree();
        let p = canon(t.path()).join("cache");
        let st = std::process::Command::new("cp")
            .arg("-c")
            .arg(p.join("a/b/deep.bin"))
            .arg(p.join("clone.bin"))
            .status()
            .expect("cp -c");
        assert!(st.success());
        let fp = fingerprint_path(&p).expect("fp");
        assert_eq!(fp.size.cloned_entries, 2);
        // Los dos miembros de la familia aportan 0: `scan` suma la familia
        // una vez si están todos en la selección.
        assert_eq!(fp.size.shared.len(), 2);
        let fam = fp.size.shared[0].clone.expect("familia");
        assert_eq!(fam.members, 2);
        assert!(
            fp.size
                .shared
                .iter()
                .all(|s| s.clone == Some(fam) && s.private == 0)
        );
        let deep = fp.size.shared[0].allocated;
        assert_eq!(fp.size.bytes_private + 2 * deep, fp.bytes_allocated);
    }

    #[test]
    fn hard_links_count_once_and_are_shared() {
        let t = tree();
        let p = canon(t.path()).join("cache");
        fs::hard_link(p.join("a/b/deep.bin"), p.join("link.bin")).expect("link");
        let fp = fingerprint_path(&p).expect("fp");
        assert_eq!(fp.size.hardlinked_entries, 1);
        assert_eq!(fp.size.shared.len(), 1);
        assert_eq!(fp.size.bytes_private, fp.bytes_allocated);
    }

    #[test]
    fn symlinked_parent_is_rejected() {
        let t = tree();
        let base = canon(t.path());
        std::os::unix::fs::symlink(base.join("cache"), base.join("alias")).expect("symlink");
        assert!(fingerprint_path(&base.join("alias/top.txt")).is_err());
    }
}
