//! Envío a la Papelera.
//!
//! - [`TrashBackend::System`]: `-[NSFileManager trashItemAtURL:resultingItemURL:error:]`
//!   vía `objc2`, igual que Finder. Se captura `resultingItemURL` para poder
//!   deshacer con el manifiesto propio (05 §1.5.1).
//! - [`TrashBackend::Dir`]: una carpeta indicada (variable
//!   `ROTTEN_TRASH_DIR`), para pruebas y entornos aislados. Mueve con
//!   `renameatx_np(RENAME_EXCL)` relativo a descriptor, nunca sobrescribe y
//!   se niega a cruzar volúmenes: sigue siendo un movimiento reversible.
//!
//! Ninguno de los dos borra nada ni degrada a borrado permanente: si no se
//! puede mover, el elemento falla y queda donde estaba.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSFileManager, NSURL};
use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashBackend {
    System,
    Dir(PathBuf),
}

impl TrashBackend {
    /// Texto para el journal: `system` o `dir:<ruta>`.
    pub fn describe(&self) -> String {
        match self {
            TrashBackend::System => "system".into(),
            TrashBackend::Dir(d) => format!("dir:{}", d.display()),
        }
    }

    /// Mueve `name` (dentro de la carpeta abierta `parent`, cuya ruta es
    /// `path.parent()`) a la Papelera y devuelve dónde ha quedado.
    pub fn trash(&self, path: &Path, parent: BorrowedFd<'_>, name: &OsStr) -> io::Result<PathBuf> {
        match self {
            TrashBackend::System => system_trash(path),
            TrashBackend::Dir(dir) => dir_trash(dir, parent, name),
        }
    }
}

fn system_trash(path: &Path) -> io::Result<PathBuf> {
    autoreleasepool(|_| {
        let url = NSURL::from_file_path(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ruta no representable"))?;
        let fm = NSFileManager::defaultManager();
        let mut resulting = None;
        fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
            .map_err(|e| io::Error::other(e.localizedDescription().to_string()))?;
        resulting
            .and_then(|u| u.to_file_path())
            .ok_or_else(|| io::Error::other("la Papelera no devolvió la ruta resultante"))
    })
}

fn dir_trash(dir: &Path, parent: BorrowedFd<'_>, name: &OsStr) -> io::Result<PathBuf> {
    let trash = rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW_ANY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let item = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let tdev = rustix::fs::fstat(&trash)?.st_dev;
    if item.st_dev != tdev {
        return Err(io::Error::new(
            io::ErrorKind::CrossesDevices,
            "la Papelera de prueba está en otro volumen",
        ));
    }
    for n in 1..1000u32 {
        let candidate = unique_name(name, n);
        match rustix::fs::renameat_with(
            parent,
            name,
            trash.as_fd(),
            candidate.as_os_str(),
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => return Ok(dir.join(candidate)),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(io::Error::other("no hay nombre libre en la Papelera"))
}

/// `nombre`, `nombre 2`, `nombre 3`… (la extensión se conserva).
pub fn unique_name(name: &OsStr, n: u32) -> OsString {
    if n <= 1 {
        return name.to_owned();
    }
    let bytes = name.as_bytes();
    let (stem, ext) = match bytes.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 => (&bytes[..i], &bytes[i..]),
        _ => (bytes, &b""[..]),
    };
    let mut out = stem.to_vec();
    out.extend_from_slice(format!(" {n}").as_bytes());
    out.extend_from_slice(ext);
    OsString::from(OsStr::from_bytes(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_names_keep_extension() {
        assert_eq!(unique_name(OsStr::new("a.log"), 1), "a.log");
        assert_eq!(unique_name(OsStr::new("a.log"), 2), "a 2.log");
        assert_eq!(unique_name(OsStr::new("dir"), 3), "dir 3");
        assert_eq!(unique_name(OsStr::new(".hidden"), 2), ".hidden 2");
    }

    #[test]
    fn dir_trash_never_overwrites() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let trash = base.join("trash");
        std::fs::create_dir(&trash).expect("mkdir");
        std::fs::write(trash.join("a.log"), b"old").expect("write");
        std::fs::write(base.join("a.log"), b"new").expect("write");
        let parent = rustix::fs::open(&base, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
            .expect("open");
        let backend = TrashBackend::Dir(trash.clone());
        let to = backend
            .trash(&base.join("a.log"), parent.as_fd(), OsStr::new("a.log"))
            .expect("trash");
        assert_eq!(to, trash.join("a 2.log"));
        assert_eq!(std::fs::read(trash.join("a.log")).expect("read"), b"old");
        assert_eq!(std::fs::read(&to).expect("read"), b"new");
    }

    /// Usa la Papelera real del usuario: mueve un fichero temporal y lo
    /// devuelve a su sitio. No se ejecuta por defecto; para probar la
    /// integración con `NSFileManager` fuera del sandbox:
    /// `cargo test -p ra-exec -- --ignored system_trash_roundtrip`.
    #[test]
    #[ignore = "toca la Papelera real del usuario"]
    fn system_trash_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let f = base.join("rotten-apple-system-trash-test.txt");
        std::fs::write(&f, b"x").expect("write");
        let before = rustix::fs::lstat(&f).expect("lstat");
        let parent = rustix::fs::open(&base, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
            .expect("open");
        let to = TrashBackend::System
            .trash(&f, parent.as_fd(), f.file_name().expect("nombre"))
            .expect("trash");
        let after = rustix::fs::lstat(&to).expect("lstat");
        assert_eq!(
            before.st_ino, after.st_ino,
            "mover a la Papelera conserva el inode"
        );
        std::fs::rename(&to, &f).expect("devolver");
        assert!(f.exists());
    }
}
