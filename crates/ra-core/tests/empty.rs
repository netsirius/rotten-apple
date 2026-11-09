//! `rotten trash` y `rotten trash empty` en el núcleo (T-12), siempre con
//! la Papelera de prueba (`TrashBackend::Dir`): nada sale de la raíz
//! temporal.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use ra_core::{
    Core, CoreError, Env, ErrorClass, FreeTotals, HistoryKind, Outcome, PlanError, TrashBackend,
    UndoState,
};

struct Fx {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    trash: PathBuf,
    core: Core,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");
    let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
    for d in [&home, &trash, &state] {
        fs::create_dir_all(d).expect("mkdir");
    }
    let core = Core::with_apps(
        Env::new(home.clone(), state, TrashBackend::Dir(trash.clone())),
        Box::new(BTreeSet::<String>::new()),
    )
    .expect("core");
    Fx {
        _tmp: tmp,
        home,
        trash,
        core,
    }
}

impl Fx {
    /// Crea `names` en la carpeta personal y los manda a la Papelera con
    /// `trash put`; devuelve la operación.
    fn trash_put(&self, names: &[&str]) -> String {
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|n| {
                let p = self.home.join(n);
                fs::write(&p, vec![b'x'; 5000]).expect("write");
                p
            })
            .collect();
        let preview = self.core.plan_selection(&paths).expect("plan");
        let hash = preview.plan.plan_hash().to_owned();
        let r = self
            .core
            .execute(preview.plan, &hash, "test")
            .expect("trash put");
        assert_eq!(r.done as usize, names.len());
        r.op_id.expect("op")
    }

    fn decoy(&self, name: &str) {
        fs::write(self.trash.join(name), b"de otra app").expect("write");
    }

    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.trash)
            .expect("ls")
            .map(|e| {
                e.expect("entrada")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        v.sort();
        v
    }
}

/// `st_blocks × 512` de todo lo que hay bajo `p`, enlaces duros una vez
/// (lo que cuenta `du -sk`).
fn du(p: &Path, seen: &mut BTreeSet<(u64, u64)>) -> u64 {
    let m = fs::symlink_metadata(p).expect("lstat");
    let mut n = if seen.insert((m.dev(), m.ino())) {
        m.blocks() * 512
    } else {
        0
    };
    if m.is_dir() {
        for e in fs::read_dir(p).expect("ls") {
            n += du(&e.expect("entrada").path(), seen);
        }
    }
    n
}

/// Criterio de aceptación de T-12: lo que aparece en la Papelera después
/// de la revisión no se borra, aunque se ejecute el mismo plan.
#[test]
fn an_entry_that_appears_after_the_preview_is_not_deleted() {
    let fx = fx();
    let op = fx.trash_put(&["a.log", "b.log"]);
    fx.decoy("ajeno.txt");
    let preview = fx.core.prepare_empty().expect("plan");
    assert_eq!(preview.plan.totals.items, 3);
    assert_eq!(preview.dry_run.would, 3);
    let hash = preview.plan.plan_hash.clone();

    // Llegan cosas nuevas entre la revisión y la ejecución.
    fx.decoy("llega-tarde.txt");
    fs::create_dir(fx.trash.join("carpeta nueva")).expect("mkdir");
    let r = fx
        .core
        .empty_trash(&preview.plan, &hash, "test")
        .expect("vaciar");
    assert_eq!((r.report.done, r.report.skipped), (3, 0));
    assert_eq!(fx.names(), ["carpeta nueva", "llega-tarde.txt"]);
    assert_eq!(r.space.estimated_bytes, preview.plan.totals.bytes);

    // La operación que mandó a.log y b.log pasa a irreversible, y el
    // vaciado queda en el historial.
    let (ops, _) = fx.core.history().expect("historial");
    let by_id = |id: &str| ops.iter().find(|o| o.op_id == id).expect("op").clone();
    assert_eq!(by_id(&op).undo_state, UndoState::Irreversible);
    let empty = r.report.op_id.as_deref().expect("op");
    assert_eq!(by_id(empty).kind, HistoryKind::TrashEmpty);
}

/// Lo que cambia entre la revisión y la ejecución se omite, y un plan
/// alterado no se ejecuta.
#[test]
fn changed_entries_are_skipped_and_tampered_plans_refused() {
    let fx = fx();
    fx.trash_put(&["a.log"]);
    fx.decoy("ajeno.txt");
    let preview = fx.core.prepare_empty().expect("plan");
    let hash = preview.plan.plan_hash.clone();

    // Ruta fuera de la Papelera con el hash original: el plan no es el
    // revisado.
    let mut forged = preview.plan.clone();
    forged.items[0].path = fx.home.join("tesis.docx");
    fs::write(fx.home.join("tesis.docx"), b"importante").expect("write");
    let err = fx
        .core
        .empty_trash(&forged, &hash, "test")
        .expect_err("alterado");
    assert!(
        matches!(err, CoreError::Plan(PlanError::HashMismatch { .. })),
        "{err:?}"
    );
    // Aunque el hash cuadre, nada fuera del primer nivel de la Papelera.
    forged.plan_hash = forged.compute_hash();
    let err = fx
        .core
        .empty_trash(&forged, &forged.plan_hash.clone(), "test")
        .expect_err("fuera");
    assert!(
        matches!(err, CoreError::Plan(PlanError::Invalid(_))),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Safety);
    assert!(fx.home.join("tesis.docx").exists());

    // ajeno.txt cambia (otro tamaño) y a.log solo de permisos (ctime).
    fs::write(fx.trash.join("ajeno.txt"), b"cambiado despues").expect("write");
    fs::set_permissions(fx.trash.join("a.log"), fs::Permissions::from_mode(0o600)).expect("chmod");
    let r = fx
        .core
        .empty_trash(&preview.plan, &hash, "test")
        .expect("vaciar");
    assert_eq!((r.report.done, r.report.skipped), (0, 2));
    assert!(
        r.report
            .items
            .iter()
            .all(|i| matches!(&i.outcome, Outcome::Skipped { .. }))
    );
    assert_eq!(fx.names(), ["a.log", "ajeno.txt"]);
}

/// I1: un cambio dos o más niveles dentro de una carpeta de la Papelera
/// (que no toca la carpeta de primer nivel) también la deja fuera; un
/// `.DS_Store` de Finder dentro, no.
#[test]
fn deep_changes_inside_a_folder_are_skipped() {
    let fx = fx();
    let deep = fx.trash.join("proyecto/a/b");
    fs::create_dir_all(&deep).expect("mkdir");
    fs::write(deep.join("foto.jpg"), b"original").expect("write");
    fs::create_dir_all(fx.trash.join("otra/x")).expect("mkdir");
    fs::write(fx.trash.join("otra/x/y.txt"), b"y").expect("write");
    let preview = fx.core.prepare_empty().expect("plan");
    assert_eq!(preview.dry_run.would, 2);
    assert!(preview.plan.items.iter().all(|i| i.digest.is_some()));
    let hash = preview.plan.plan_hash.clone();

    let top = fs::symlink_metadata(fx.trash.join("proyecto")).expect("lstat");
    std::thread::sleep(std::time::Duration::from_millis(5));
    fs::write(deep.join("foto.jpg"), b"cambiada").expect("write");
    fs::write(fx.trash.join("otra/x/.DS_Store"), b"finder").expect("write");
    let after = fs::symlink_metadata(fx.trash.join("proyecto")).expect("lstat");
    assert_eq!(
        (top.mtime(), top.mtime_nsec(), top.ctime(), top.ctime_nsec()),
        (
            after.mtime(),
            after.mtime_nsec(),
            after.ctime(),
            after.ctime_nsec()
        ),
        "el cambio no debe tocar la carpeta de primer nivel"
    );

    let r = fx
        .core
        .empty_trash(&preview.plan, &hash, "test")
        .expect("vaciar");
    assert_eq!((r.report.done, r.report.skipped), (1, 1));
    let skipped = r
        .report
        .items
        .iter()
        .find(|i| matches!(i.outcome, Outcome::Skipped { .. }))
        .expect("omitido");
    assert!(skipped.path.ends_with("proyecto"));
    assert!(
        matches!(
            &skipped.outcome,
            Outcome::Skipped {
                cause: ra_core::SkipCause::ContentsChanged,
                ..
            }
        ),
        "{:?}",
        skipped.outcome
    );
    assert_eq!(fx.names(), ["proyecto"]);
    assert_eq!(fs::read(deep.join("foto.jpg")).expect("read"), b"cambiada");
}

/// M3: una carpeta de la Papelera que no se puede recorrer entera queda
/// sin huella: `rotten trash` dice que su tamaño se queda corto y
/// `trash empty` no la borra.
#[test]
fn a_folder_that_cannot_be_walked_is_never_emptied() {
    let fx = fx();
    let locked = fx.trash.join("bloqueada/secreto");
    fs::create_dir_all(&locked).expect("mkdir");
    fs::write(locked.join("x.txt"), vec![b'x'; 9000]).expect("write");
    fx.decoy("ajeno.txt");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod");

    let view = fx.core.trash_view().expect("vista");
    let preview = fx.core.prepare_empty().expect("plan");
    let hash = preview.plan.plan_hash.clone();
    let r = fx.core.empty_trash(&preview.plan, &hash, "test");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("chmod");
    let r = r.expect("vaciar");

    assert!(view.volumes[0].partial);
    let item = |p: &[ra_core::EmptyItem]| {
        p.iter()
            .find(|i| i.path.ends_with("bloqueada"))
            .expect("bloqueada")
            .clone()
    };
    assert_eq!(item(&preview.plan.items).digest, None);
    let dry = preview
        .dry_run
        .items
        .iter()
        .find(|i| i.path.ends_with("bloqueada"))
        .expect("simulación");
    assert!(
        matches!(
            dry.outcome,
            Outcome::Skipped {
                cause: ra_core::SkipCause::NotVerifiable,
                ..
            }
        ),
        "{:?}",
        dry.outcome
    );
    assert_eq!((r.report.done, r.report.skipped), (1, 1));
    assert!(locked.join("x.txt").exists());
    assert_eq!(fx.names(), ["bloqueada"]);
}

/// Sin poder leer la Papelera, `trash empty` no prepara nada ni borra nada
/// (3), y `trash` dice que no puede leerla y muestra solo lo del journal.
#[test]
fn unreadable_trash_empties_nothing_and_view_says_so() {
    let fx = fx();
    let op = fx.trash_put(&["a.log", "b.log"]);
    fx.decoy("ajeno.txt");
    let preview = fx.core.prepare_empty().expect("plan");
    let hash = preview.plan.plan_hash.clone();

    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let prepared = fx.core.prepare_empty();
    let executed = fx.core.empty_trash(&preview.plan, &hash, "test");
    let view = fx.core.trash_view();
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");

    for err in [
        prepared.expect_err("sin permiso"),
        executed.expect_err("sin permiso"),
    ] {
        assert!(matches!(err, CoreError::EmptyUnreadable { .. }), "{err:?}");
        assert_eq!(err.class(), ErrorClass::Permission);
        assert_eq!(err.code(), "trash_unreadable");
        assert!(err.to_string().contains("rotten free"), "{err}");
    }
    assert_eq!(fx.names(), ["a.log", "ajeno.txt", "b.log"]);

    let view = view.expect("vista");
    assert!(!view.readable);
    let v = &view.volumes[0];
    assert_eq!((v.readable, v.total, v.others), (false, None, None));
    assert_eq!(v.by_operation.len(), 1);
    assert_eq!(
        (v.by_operation[0].op_id.as_str(), v.by_operation[0].items),
        (op.as_str(), 2)
    );
}

/// `rotten trash` separa lo de cada operación de lo demás, y el total es
/// lo que mide `du` (RF-PAN-07).
#[test]
fn view_splits_operations_from_the_rest() {
    let fx = fx();
    let a = fx.trash_put(&["a.log", "b.log"]);
    let b = fx.trash_put(&["c.log"]);
    fx.decoy("ajeno.txt");
    fs::create_dir_all(fx.trash.join("carpeta/sub")).expect("mkdir");
    fs::write(fx.trash.join("carpeta/sub/x"), vec![0u8; 70_000]).expect("write");
    // Finder: no cuenta ni se borra.
    fs::write(fx.trash.join(".DS_Store"), b"finder").expect("write");

    let view = fx.core.trash_view().expect("vista");
    assert!(view.readable);
    assert_eq!(view.volumes.len(), 1);
    let v = &view.volumes[0];
    assert_eq!(v.path, fx.trash);
    let ops: Vec<(&str, u64)> = v
        .by_operation
        .iter()
        .map(|o| (o.op_id.as_str(), o.items))
        .collect();
    assert_eq!(ops, [(a.as_str(), 2), (b.as_str(), 1)]);
    let others = v.others.expect("otros");
    assert_eq!(others.items, 2);
    let total = v.total.expect("total");
    assert_eq!(total.items, 5);
    let mine: u64 = v.by_operation.iter().map(|o| o.bytes).sum();
    assert_eq!(total.bytes, mine + others.bytes);

    let mut seen = BTreeSet::new();
    let expected: u64 = fs::read_dir(&fx.trash)
        .expect("ls")
        .map(|e| e.expect("entrada").path())
        .filter(|p| p.file_name().is_some_and(|n| n != ".DS_Store"))
        .map(|p| du(&p, &mut seen))
        .sum();
    assert_eq!(total.bytes, expected);
    assert_eq!(
        FreeTotals {
            items: total.items,
            bytes: total.bytes
        },
        fx.core.prepare_empty().expect("plan").plan.totals
    );

    // Vaciar no toca .DS_Store.
    let p = fx.core.prepare_empty().expect("plan");
    let hash = p.plan.plan_hash.clone();
    fx.core.empty_trash(&p.plan, &hash, "test").expect("vaciar");
    assert_eq!(fx.names(), [".DS_Store"]);
    let v = fx.core.trash_view().expect("vista").volumes.remove(0);
    assert_eq!(v.total, Some(FreeTotals::default()));
    assert!(v.by_operation.is_empty());
}

/// Papelera **real** del usuario (`TrashBackend::System`): `~/.Trash` y las
/// de `/Volumes/*/.Trashes/<uid>`.
///
/// Ignorada por defecto porque lee la Papelera de quien la ejecuta y su
/// resultado depende de la máquina y de si la terminal tiene Acceso total
/// al disco. **Solo lee**: nunca prepara ni ejecuta `trash empty`, y el
/// estado (journal) va a una carpeta temporal. Para correrla a mano:
///
/// ```sh
/// cargo test -p ra-core --test empty -- --ignored real_trash
/// ```
///
/// Con Acceso total al disco comprueba RF-PAN-07: el tamaño de cada
/// volumen coincide (± 1 %) con lo que mide `du` de su contenido.
#[test]
#[ignore = "lee la Papelera real del usuario; solo lectura, se ejecuta a mano"]
fn real_trash_is_read_only_and_matches_du() {
    let home = std::env::home_dir().expect("HOME");
    let state = tempfile::tempdir().expect("tempdir");
    let core = Core::new(Env::new(
        home.clone(),
        state.path().to_path_buf(),
        TrashBackend::System,
    ))
    .expect("core");
    let view = core.trash_view().expect("vista");
    assert_eq!(view.volumes[0].path, home.join(".Trash"));
    for v in &view.volumes {
        eprintln!("{}: legible={} {:?}", v.path.display(), v.readable, v.total);
        let Some(total) = v.total else {
            assert!(!v.readable);
            continue;
        };
        let mut seen = BTreeSet::new();
        let measured: u64 = fs::read_dir(&v.path)
            .into_iter()
            .flatten()
            .map(|e| e.expect("entrada").path())
            .filter(|p| p.file_name().is_some_and(|n| n != ".DS_Store"))
            .map(|p| du(&p, &mut seen))
            .sum();
        let diff = total.bytes.abs_diff(measured);
        assert!(
            diff <= measured / 100 + 4096,
            "{}: {} frente a {measured}",
            v.path.display(),
            total.bytes
        );
    }
}
