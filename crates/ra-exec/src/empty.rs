//! Lo que hay en la Papelera (`rotten trash`) y el plan para vaciarla
//! (`rotten trash empty`, T-12, CU-08).
//!
//! - Las carpetas salen siempre del backend: con `ROTTEN_TRASH_DIR`, solo
//!   esa; con la del sistema, `~/.Trash` y cada
//!   `/Volumes/<X>/.Trashes/<uid>` que exista.
//! - El plan tiene la identidad completa de cada entrada de primer nivel,
//!   leída en la propia Papelera; al borrar se exige que siga siendo
//!   idéntica (`ctime` incluido). Lo que aparezca después no está en el plan.
//! - Una Papelera que no se puede leer no se adivina: es un error.
//! - `.DS_Store` del primer nivel no entra: es de Finder y guarda los datos
//!   de «Devolver» de lo que quede en la Papelera.

use std::ffi::OsStr;
use std::io;
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use ra_history::{FreeTotals, ItemRef, TrashedItem};
use ra_safety::Identity;
use rustix::fs::{AtFlags, Dir, Mode, OFlags};
use serde::{Deserialize, Serialize};

use crate::trash::TrashBackend;

/// Nombre que nunca se lista ni se borra en el primer nivel.
const FINDER_METADATA: &[u8] = b".DS_Store";

/// Carpetas de la Papelera que usa `backend`, en orden estable.
pub fn trash_roots(backend: &TrashBackend, home: &Path) -> Vec<PathBuf> {
    match backend {
        TrashBackend::Dir(d) => vec![d.clone()],
        TrashBackend::System => {
            let uid = rustix::process::getuid().as_raw().to_string();
            let mut out = vec![home.join(".Trash")];
            let mut vols: Vec<PathBuf> = std::fs::read_dir("/Volumes")
                .into_iter()
                .flatten()
                .flatten()
                // El volumen de arranque es un enlace a `/`: no se sigue.
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path().join(".Trashes").join(&uid))
                .filter(|p| match std::fs::symlink_metadata(p) {
                    Ok(_) => true,
                    Err(e) => e.kind() == io::ErrorKind::PermissionDenied,
                })
                .collect();
            vols.sort();
            out.extend(vols);
            out
        }
    }
}

/// Una entrada del primer nivel de la Papelera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEntry {
    pub path: PathBuf,
    pub identity: Identity,
    /// Bytes asignados (`st_blocks × 512`, enlaces duros una vez dentro de
    /// la entrada), como `du`.
    pub bytes: u64,
}

/// Entradas del primer nivel de `dir`, por nombre. Si `dir` no existe, no
/// hay nada; cualquier otro fallo al abrirla o leerla es un error (sin
/// Acceso total al disco, `EPERM`).
pub fn read_trash_dir(dir: &Path) -> io::Result<Vec<TrashEntry>> {
    let fd = match rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW_ANY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut names: Vec<Vec<u8>> = Vec::new();
    for e in Dir::read_from(&fd)? {
        let e = e?;
        let name = e.file_name().to_bytes();
        if name == b"." || name == b".." || name == FINDER_METADATA {
            continue;
        }
        names.push(name.to_vec());
    }
    names.sort();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let name = OsStr::from_bytes(&name);
        // Lo que desaparece mientras se lee ya no está.
        let Ok(st) = rustix::fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        let bytes = ra_safety::fingerprint::fingerprint_at(fd.as_fd(), name)
            .map(|f| f.bytes_allocated)
            .unwrap_or(st.st_blocks as u64 * 512);
        out.push(TrashEntry {
            path: dir.join(name),
            identity: Identity::from_stat(&st),
            bytes,
        });
    }
    Ok(out)
}

/// La operación y el elemento que mandaron `entry` a la Papelera, según
/// el journal: mismo objeto (volumen, inode y tipo), como al deshacer.
/// Si hay varios, el que además coincide en la ruta.
pub fn attribute<'a>(entry: &TrashEntry, known: &'a [TrashedItem]) -> Option<&'a TrashedItem> {
    let mut found = None;
    for k in known {
        if k.identity.same_object(&entry.identity) {
            if k.path == entry.path {
                return Some(k);
            }
            found.get_or_insert(k);
        }
    }
    found
}

/// Una entrada que `rotten trash empty` borraría.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyItem {
    /// Número del elemento en la operación de vaciar (1, 2…).
    pub item_id: u32,
    pub path: PathBuf,
    /// Identidad leída en la Papelera al preparar el plan.
    pub identity: Identity,
    pub bytes: u64,
    /// Operación y elemento de Rotten Apple que la mandaron allí, si consta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<ItemRef>,
}

/// Plan de `rotten trash empty`. Su hash cubre las carpetas, cada entrada
/// con su identidad completa y los totales.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyPlan {
    pub trash_dirs: Vec<PathBuf>,
    pub items: Vec<EmptyItem>,
    pub totals: FreeTotals,
    pub plan_hash: String,
}

impl EmptyPlan {
    /// Hash BLAKE3 del plan sin el campo `plan_hash`.
    pub fn compute_hash(&self) -> String {
        let body = (&self.trash_dirs, &self.items, &self.totals);
        let text = serde_json::to_string(&body).unwrap_or_default();
        format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex())
    }

    /// Cada entrada está en el primer nivel de una de `trash_dirs`, que son
    /// carpetas de la Papelera del backend actual.
    pub fn within(&self, roots: &[PathBuf]) -> bool {
        self.trash_dirs.iter().all(|d| roots.contains(d))
            && self.items.iter().all(|it| {
                let name_ok = it
                    .path
                    .file_name()
                    .is_some_and(|n| n != "." && n != ".." && n.as_bytes() != FINDER_METADATA);
                name_ok
                    && it
                        .path
                        .parent()
                        .is_some_and(|p| self.trash_dirs.iter().any(|d| d == p))
            })
    }
}

/// Una carpeta de la Papelera que no se ha podido leer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub path: PathBuf,
    pub error: String,
}

/// Plan para vaciar `dirs`: cada entrada de primer nivel con su identidad.
/// Si alguna carpeta no se puede leer, no hay plan (no se vacía «lo que se
/// pueda»).
pub fn plan_empty(dirs: &[PathBuf], known: &[TrashedItem]) -> Result<EmptyPlan, Unreadable> {
    let mut plan = EmptyPlan {
        trash_dirs: dirs.to_vec(),
        items: vec![],
        totals: FreeTotals::default(),
        plan_hash: String::new(),
    };
    for d in dirs {
        let entries = read_trash_dir(d).map_err(|e| Unreadable {
            path: d.clone(),
            error: e.to_string(),
        })?;
        for e in entries {
            plan.totals.items += 1;
            plan.totals.bytes += e.bytes;
            let of = attribute(&e, known).map(|k| k.of.clone());
            plan.items.push(EmptyItem {
                item_id: plan.items.len() as u32 + 1,
                path: e.path,
                identity: e.identity,
                bytes: e.bytes,
                of,
            });
        }
    }
    plan.plan_hash = plan.compute_hash();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn known(path: &Path, op: &str, item: u32) -> TrashedItem {
        TrashedItem {
            of: ItemRef {
                op: op.into(),
                item,
            },
            path: path.to_path_buf(),
            original_path: PathBuf::from("/x"),
            identity: ra_safety::fingerprint::stat_nofollow_any(path)
                .expect("lstat")
                .0,
            bytes: 1,
            trash: "dir:/t".into(),
        }
    }

    /// Las entradas salen ordenadas, con su identidad y tamaño; `.DS_Store`
    /// no entra; una carpeta que no existe está vacía.
    #[test]
    fn reads_top_level_entries_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let t = fs::canonicalize(tmp.path()).expect("canon");
        fs::create_dir_all(t.join("b/sub")).expect("mkdir");
        fs::write(t.join("b/sub/x"), vec![0u8; 10_000]).expect("write");
        fs::write(t.join("a.txt"), b"a").expect("write");
        fs::write(t.join(".DS_Store"), b"finder").expect("write");
        let got = read_trash_dir(&t).expect("read");
        let names: Vec<_> = got.iter().map(|e| e.path.clone()).collect();
        assert_eq!(names, vec![t.join("a.txt"), t.join("b")]);
        assert!(got[1].bytes >= 10_000, "{:?}", got[1]);
        assert_eq!(read_trash_dir(&t.join("no-existe")).expect("vacía"), vec![]);
    }

    /// Sin permiso no hay plan: no se vacía a medias.
    #[test]
    fn unreadable_dir_gives_no_plan() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(tmp.path()).expect("canon");
        let (a, b) = (base.join("a"), base.join("b"));
        for d in [&a, &b] {
            fs::create_dir(d).expect("mkdir");
            fs::write(d.join("x"), b"x").expect("write");
        }
        fs::set_permissions(&b, fs::Permissions::from_mode(0o000)).expect("chmod");
        let got = plan_empty(&[a.clone(), b.clone()], &[]);
        fs::set_permissions(&b, fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(got.expect_err("sin permiso").path, b);
    }

    /// Lo que mandó Rotten Apple se reconoce por inode, no por nombre; el
    /// hash cambia con la identidad y `within` rechaza rutas fuera de las
    /// carpetas de la Papelera.
    #[test]
    fn attribution_hash_and_bounds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let t = fs::canonicalize(tmp.path()).expect("canon");
        fs::write(t.join("mio"), b"1").expect("write");
        fs::write(t.join("ajeno"), b"2").expect("write");
        let k = known(&t.join("mio"), "op-1", 3);
        // Otro elemento del journal con el mismo nombre pero otro inode.
        fs::write(t.join("viejo"), b"3").expect("write");
        let mut stale = known(&t.join("viejo"), "op-0", 1);
        stale.path = t.join("ajeno");
        let plan = plan_empty(std::slice::from_ref(&t), &[stale, k]).expect("plan");
        let of: Vec<_> = plan.items.iter().map(|i| i.of.clone()).collect();
        assert_eq!(
            of,
            vec![
                None,
                Some(ItemRef {
                    op: "op-1".into(),
                    item: 3
                }),
                Some(ItemRef {
                    op: "op-0".into(),
                    item: 1
                }),
            ]
        );
        assert_eq!(plan.plan_hash, plan.compute_hash());
        let mut changed = plan.clone();
        changed.items[0].identity.ctime_nsec += 1;
        assert_ne!(changed.compute_hash(), plan.plan_hash);

        assert!(plan.within(std::slice::from_ref(&t)));
        assert!(!plan.within(&[t.join("otra")]));
        let mut outside = plan.clone();
        outside.items[0].path = t.parent().expect("padre").join("ajeno");
        assert!(!outside.within(std::slice::from_ref(&t)));
        let mut dotdot = plan.clone();
        dotdot.items[0].path = t.join("..");
        assert!(!dotdot.within(std::slice::from_ref(&t)));
    }
}
