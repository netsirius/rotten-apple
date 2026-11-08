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
use ra_safety::Identity;
use rustix::fs::{AtFlags, Dir, Mode, OFlags, RenameFlags};

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

/// Tras un cierre abrupto, busca en la Papelera que usó la operación
/// (`trash`, tal como lo escribió [`TrashBackend::describe`]) el elemento que
/// estaba en `original`. Compara por identidad (volumen, inode y tipo), nunca
/// por nombre, y mira solo el primer nivel de la Papelera.
///
/// `Err` con el motivo si no se encuentra y alguna de las Papeleras no se
/// pudo leer por permisos: sin Acceso total al disco, `~/.Trash` no se
/// puede listar y el elemento puede estar ahí.
pub(crate) fn find_trashed(
    trash: &str,
    original: &Path,
    identity: &Identity,
) -> Result<Option<PathBuf>, String> {
    // Solo hace falta con la Papelera del sistema; sin ella, `~/.Trash` no
    // se considera.
    let home = std::env::home_dir().unwrap_or_default();
    let uid = rustix::process::getuid().as_raw();
    let mut denied = None;
    for d in trash_dirs(trash, original, identity.dev, &home, uid, dev_of) {
        match find_in_dir(&d, identity) {
            Ok(Some(p)) => return Ok(Some(p)),
            Ok(None) => {}
            Err(NoPermission) => denied = Some(d),
        }
    }
    match denied {
        Some(d) => Err(format!(
            "sin permiso para leer la Papelera ({}); concede Acceso total al disco y ejecuta `rotten doctor`",
            d.display()
        )),
        None => Ok(None),
    }
}

/// La Papelera existe pero no se puede leer (`EPERM` o `EACCES`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NoPermission;

fn permission(e: rustix::io::Errno) -> Result<Option<PathBuf>, NoPermission> {
    if e == rustix::io::Errno::PERM || e == rustix::io::Errno::ACCESS {
        Err(NoPermission)
    } else {
        Ok(None)
    }
}

fn dev_of(p: &Path) -> Option<u64> {
    rustix::fs::lstat(p).ok().map(|st| st.st_dev as u64)
}

/// Carpetas donde pudo quedar un elemento del volumen `dev`:
///
/// - `dir:<ruta>`: esa carpeta.
/// - `system`: `~/.Trash` si el elemento es del volumen de la carpeta
///   personal; si no, `<raíz del volumen>/.Trashes/<uid>`, donde la raíz es
///   el antepasado más alto de `original` que sigue en el volumen `dev`
///   (`/` incluido).
fn trash_dirs(
    trash: &str,
    original: &Path,
    dev: u64,
    home: &Path,
    uid: u32,
    dev_of: impl Fn(&Path) -> Option<u64>,
) -> Vec<PathBuf> {
    if let Some(dir) = trash.strip_prefix("dir:") {
        return vec![PathBuf::from(dir)];
    }
    if trash != "system" {
        return vec![];
    }
    if dev_of(home) == Some(dev) {
        return vec![home.join(".Trash")];
    }
    let mut root = None;
    for a in original.ancestors().skip(1) {
        match dev_of(a) {
            Some(d) if d == dev => root = Some(a),
            Some(_) => break,
            None => continue,
        }
    }
    root.map(|r| r.join(".Trashes").join(uid.to_string()))
        .into_iter()
        .collect()
}

/// Entrada del primer nivel de `dir` que es el mismo objeto que `identity`.
fn find_in_dir(dir: &Path, identity: &Identity) -> Result<Option<PathBuf>, NoPermission> {
    let fd = match rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW_ANY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(e) => return permission(e),
    };
    // Mover a la Papelera es un `rename`: si la Papelera está en otro
    // volumen, el elemento no puede estar en ella.
    match rustix::fs::fstat(&fd) {
        Ok(st) if st.st_dev as u64 == identity.dev => {}
        _ => return Ok(None),
    }
    let entries = match Dir::read_from(&fd) {
        Ok(d) => d,
        Err(e) => return permission(e),
    };
    for e in entries {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let name = OsStr::from_bytes(name);
        let Ok(st) = rustix::fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        if Identity::from_stat(&st).same_object(identity) {
            return Ok(Some(dir.join(name)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(p: &Path) -> Identity {
        ra_safety::fingerprint::stat_nofollow_any(p)
            .expect("lstat")
            .0
    }

    /// Varias entradas `a N.log` y un `a.log` con el mismo nombre que el
    /// original pero otro inode: solo vale la que es el mismo objeto.
    #[test]
    fn finds_trashed_item_by_inode_not_by_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let trash = base.join("trash");
        std::fs::create_dir(&trash).expect("mkdir");
        let original = base.join("a.log");
        std::fs::write(&original, b"elemento").expect("write");
        let id = identity(&original);
        for n in ["a.log", "a 2.log", "a 4.log"] {
            std::fs::write(trash.join(n), b"otro").expect("write");
        }
        std::fs::rename(&original, trash.join("a 3.log")).expect("mover");
        let desc = TrashBackend::Dir(trash.clone()).describe();
        assert_eq!(
            find_trashed(&desc, &original, &id),
            Ok(Some(trash.join("a 3.log")))
        );

        // Si ya no está (Papelera vaciada), no se confunde con `a.log`.
        std::fs::remove_file(trash.join("a 3.log")).expect("rm");
        assert_eq!(find_trashed(&desc, &original, &id), Ok(None));
    }

    /// I-2: una Papelera que no se puede leer no es «no está»: el motivo
    /// dice que falta permiso y cómo arreglarlo.
    #[test]
    fn unreadable_trash_is_reported_not_silent() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let trash = base.join("trash");
        std::fs::create_dir(&trash).expect("mkdir");
        let original = base.join("a.log");
        std::fs::write(&original, b"x").expect("write");
        let id = identity(&original);
        std::fs::rename(&original, trash.join("a.log")).expect("mover");
        std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let desc = TrashBackend::Dir(trash.clone()).describe();
        let got = find_trashed(&desc, &original, &id);
        let in_dir = find_in_dir(&trash, &id);
        std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let msg = got.expect_err("sin permiso");
        assert!(
            msg.starts_with("sin permiso para leer la Papelera"),
            "{msg}"
        );
        assert!(msg.contains("rotten doctor"), "{msg}");
        assert_eq!(in_dir, Err(NoPermission));
        // Que no exista no es falta de permiso.
        assert_eq!(find_in_dir(&base.join("no-existe"), &id), Ok(None));
    }

    /// Una Papelera en otro volumen no se recorre, y una descripción
    /// desconocida no da carpetas.
    #[test]
    fn foreign_volume_or_unknown_backend_finds_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let f = base.join("x");
        std::fs::write(&f, b"x").expect("write");
        let mut id = identity(&f);
        assert_eq!(find_in_dir(&base, &id), Ok(Some(f.clone())));
        id.dev ^= 1;
        assert_eq!(find_in_dir(&base, &id), Ok(None));
        assert!(trash_dirs("otra", &f, 1, &base, 501, |_| Some(1)).is_empty());
    }

    /// Papelera del sistema: `~/.Trash` en el volumen de la carpeta personal
    /// y `<volumen>/.Trashes/<uid>` en los demás (volúmenes simulados).
    #[test]
    fn system_trash_dirs_by_volume() {
        let home = Path::new("/Users/ana");
        let devs = |p: &Path| -> Option<u64> {
            let s = p.to_str()?;
            if s.starts_with("/Volumes/Ext") {
                Some(7)
            } else if s.starts_with("/Volumes") || s == "/" {
                Some(1)
            } else if s.starts_with("/Users") {
                Some(2)
            } else {
                None
            }
        };
        // Volumen de la carpeta personal.
        let item = Path::new("/Users/ana/Library/Logs/a.log");
        assert_eq!(
            trash_dirs("system", item, 2, home, 501, devs),
            vec![PathBuf::from("/Users/ana/.Trash")]
        );
        // Otro volumen montado en /Volumes/Ext; la carpeta del elemento ya
        // no existe.
        let item = Path::new("/Volumes/Ext/Proyecto/borrada/a.log");
        assert_eq!(
            trash_dirs("system", item, 7, home, 501, devs),
            vec![PathBuf::from("/Volumes/Ext/.Trashes/501")]
        );
        // Volumen de arranque distinto del de la carpeta personal.
        let item = Path::new("/Volumes/a.log");
        assert_eq!(
            trash_dirs("system", item, 1, home, 501, devs),
            vec![PathBuf::from("/.Trashes/501")]
        );
    }

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
