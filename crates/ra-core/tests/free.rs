//! Propiedad de «Liberar ahora» (T-11): `rotten free <op>` nunca borra nada
//! que no esté registrado para esa operación.
//!
//! Cada caso crea `raíz/{home,trash,state}` con la Papelera de prueba
//! (`TrashBackend::Dir`); nada sale de la raíz temporal. Se generan:
//!
//! - ficheros y carpetas en la carpeta personal, repartidos entre dos
//!   operaciones (`trash put`) A y B y lo que se queda en su sitio;
//! - señuelos en la Papelera que no mandó Rotten Apple, algunos con el
//!   mismo nombre que los elementos de A (que acaban como `nombre 2`);
//! - cambios a los elementos de A **después de revisar** el plan y antes
//!   de ejecutarlo: modificar, sustituir por otro con el mismo nombre o
//!   quitar.
//!
//! B se manda a **otra** Papelera (otro `ROTTEN_TRASH_DIR`) y además hay
//! una operación forjada en el journal que dice haber dejado «en la
//! Papelera `dir:<carpeta personal>`» los elementos que se quedan en su
//! sitio, con su identidad real.
//!
//! Se pide liberar A, B y la forjada a la vez. Solo entran en el plan los
//! elementos de A (B y la forjada: `other_trash`), y tras `free` solo
//! desaparecen los de A que siguen siendo el objeto anotado; los señuelos,
//! la otra Papelera, lo cambiado y la carpeta personal siguen igual.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use std::time::SystemTime;

use ra_core::{
    Core, CoreError, Env, ErrorClass, NotRecoverableReason, Outcome, PlanError, TrashBackend,
};
use ra_history::{Event, HistoryPaths, Journal, OpKind, OpMode, OpStatus, StepAction};

/// `PROPTEST_CASES` sube los casos en la tanda nocturna (RNF-08).
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    A,
    B,
    Home,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    Modify,
    Replace,
    Remove,
}

#[derive(Debug, Clone)]
struct Item {
    dir: bool,
    size: usize,
    owner: Owner,
    change: Change,
}

fn item() -> impl Strategy<Value = Item> {
    (
        any::<bool>(),
        1usize..3000,
        prop_oneof![
            Just(Owner::A),
            Just(Owner::A),
            Just(Owner::B),
            Just(Owner::Home)
        ],
        prop_oneof![
            3 => Just(Change::None),
            1 => Just(Change::Modify),
            1 => Just(Change::Replace),
            1 => Just(Change::Remove),
        ],
    )
        .prop_map(|(dir, size, owner, change)| Item {
            dir,
            size,
            owner,
            change,
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Meta {
    ino: u64,
    dir: bool,
    size: u64,
    mtime: (i64, i64),
}

/// Todo lo que hay bajo `base` (sin seguir enlaces), por ruta relativa.
fn snapshot(base: &Path) -> BTreeMap<PathBuf, Meta> {
    let mut out = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let m = fs::symlink_metadata(&p).expect("lstat");
            out.insert(
                p.strip_prefix(base).expect("rel").to_path_buf(),
                Meta {
                    ino: m.ino(),
                    dir: m.is_dir(),
                    size: m.size(),
                    mtime: (m.mtime(), m.mtime_nsec()),
                },
            );
            if m.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

fn make(p: &Path, dir: bool, size: usize) {
    if dir {
        fs::create_dir_all(p.join("sub")).expect("mkdir");
        fs::write(p.join("a.txt"), vec![b'a'; size]).expect("write");
        fs::write(p.join("sub/b.txt"), b"b").expect("write");
    } else {
        fs::write(p, vec![b'x'; size]).expect("write");
    }
}

fn remove(p: &Path) {
    if fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(p).expect("rm -r");
    } else {
        fs::remove_file(p).expect("rm");
    }
}

/// `trash put` de `paths` por el pipeline completo; devuelve la operación.
fn trash_put(core: &Core, paths: &[PathBuf]) -> String {
    let preview = core.plan_selection(paths).expect("plan");
    assert_eq!(
        preview.plan.items().len(),
        paths.len(),
        "{:?}",
        preview.plan.skipped()
    );
    let hash = preview.plan.plan_hash().to_owned();
    let r = core
        .execute(preview.plan, &hash, "test")
        .expect("trash put");
    assert_eq!(r.done as usize, paths.len());
    r.op_id.expect("op")
}

/// Si el journal cambia entre la revisión y la ejecución, el plan ya no es
/// el revisado: no se borra nada y el error es de seguridad (5).
#[test]
fn a_plan_whose_journal_changed_is_refused() {
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
    let files: Vec<PathBuf> = ["a.log", "b.log"]
        .iter()
        .map(|n| {
            let p = home.join(n);
            make(&p, false, 100);
            p
        })
        .collect();
    let op = trash_put(&core, &files[..1]);
    let first = core.prepare_free(std::slice::from_ref(&op)).expect("plan");
    let second = core.prepare_free(std::slice::from_ref(&op)).expect("plan");
    core.free(&second.plan, &second.plan.plan_hash, "test")
        .expect("liberar");
    // Algo más en la Papelera, que no debe tocarse.
    trash_put(&core, &files[1..]);
    let before = snapshot(&trash);
    let err = core
        .free(&first.plan, &first.plan.plan_hash, "test")
        .expect_err("plan cambiado");
    assert!(
        matches!(err, CoreError::Plan(PlanError::HashMismatch { .. })),
        "{err:?}"
    );
    assert_eq!(err.class(), ErrorClass::Safety);
    assert_eq!(err.code(), "plan_changed");
    assert_eq!(before, snapshot(&trash));
}

/// Operación `forged` en el journal de `state`: dice que cada ruta de
/// `paths` quedó «en la Papelera» `trash`, en su propio sitio y con su
/// identidad real.
fn forge(state: &Path, trash: &str, paths: &[PathBuf]) {
    let mut j =
        Journal::open(&HistoryPaths::new(state), "forged", SystemTime::now()).expect("journal");
    let start = Event::OpStarted {
        kind: OpKind::Clean,
        mode: OpMode::Trash,
        plan_hash: None,
        undo_of: None,
        items: paths.len() as u64,
        bytes: 1,
        by: "test".into(),
        trash: trash.into(),
    };
    j.write(start, true).expect("start");
    for (i, p) in paths.iter().enumerate() {
        let item = i as u32 + 1;
        let identity = ra_safety::Identity::from_metadata(&fs::symlink_metadata(p).expect("lstat"));
        let intent = Event::ItemIntent {
            item,
            action: StepAction::Trash,
            path: p.clone(),
            identity,
            target: None,
            category: None,
            regenerable: false,
            bytes: 1,
            parent_mode: None,
            origin: None,
            of: None,
        };
        j.write(intent, true).expect("intent");
        let done = Event::ItemDone {
            item,
            result_path: Some(p.clone()),
            post_check: None,
        };
        j.write(done, true).expect("done");
    }
    let end = Event::OpFinished {
        status: OpStatus::Completed,
        done: paths.len() as u64,
        skipped: 0,
        failed: 0,
        unknown: 0,
    };
    j.write(end, true).expect("end");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn free_never_deletes_anything_not_registered_for_that_operation(
        items in prop::collection::vec(item(), 1..8),
        decoys_before in prop::collection::vec(any::<prop::sample::Index>(), 0..4),
        decoys_after in 0usize..3,
    ) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canon");
        let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
        let trash_b = root.join("trash-b");
        for d in [&home, &trash, &state, &trash_b] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let work = home.join("work");
        fs::create_dir_all(&work).expect("mkdir");
        let paths: Vec<PathBuf> = items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let p = work.join(format!("e-{i}"));
                make(&p, it.dir, it.size);
                p
            })
            .collect();
        let core = Core::with_apps(
            Env::new(home.clone(), state.clone(), TrashBackend::Dir(trash.clone())),
            Box::new(BTreeSet::<String>::new()),
        )
        .expect("core");
        // Mismo historial, otra Papelera.
        let core_b = Core::with_apps(
            Env::new(home.clone(), state.clone(), TrashBackend::Dir(trash_b.clone())),
            Box::new(BTreeSet::<String>::new()),
        )
        .expect("core");

        // Señuelos con el nombre de elementos de A, antes de moverlos: A
        // acaba en la Papelera como `e-N 2`.
        for ix in &decoys_before {
            let i = ix.index(items.len());
            let p = trash.join(format!("e-{i}"));
            if fs::symlink_metadata(&p).is_err() {
                fs::write(&p, b"de otra app").expect("write");
            }
        }
        let of = |o: Owner| -> Vec<PathBuf> {
            items.iter().zip(&paths).filter(|(it, _)| it.owner == o).map(|(_, p)| p.clone()).collect()
        };
        let (in_a, in_b, stay) = (of(Owner::A), of(Owner::B), of(Owner::Home));
        prop_assume!(!in_a.is_empty());
        let op_a = trash_put(&core, &in_a);
        let mut ops = vec![op_a.clone()];
        if !in_b.is_empty() {
            ops.push(trash_put(&core_b, &in_b));
        }
        if !stay.is_empty() {
            forge(&state, &format!("dir:{}", work.display()), &stay);
            ops.push("forged".into());
        }
        for n in 0..decoys_after {
            fs::write(trash.join(format!("ajeno-{n}.txt")), b"ajeno").expect("write");
        }

        // Revisión: un elemento cuya Papelera no es la que se usa nunca
        // entra en el plan.
        let preview = core.prepare_free(&ops).expect("plan");
        prop_assert_eq!(preview.plan.ready.len(), in_a.len());
        prop_assert!(preview.plan.ready.iter().all(|it| it.of.op == op_a));
        prop_assert_eq!(preview.plan.not_freeable.len(), in_b.len() + stay.len());
        prop_assert!(
            preview
                .plan
                .not_freeable
                .iter()
                .all(|n| n.reason == NotRecoverableReason::OtherTrash),
            "{:?}",
            preview.plan.not_freeable
        );
        let hash = preview.plan.plan_hash.clone();

        // Cambios dentro de la Papelera entre la revisión y la ejecución.
        let a_items: Vec<&Item> = items.iter().filter(|it| it.owner == Owner::A).collect();
        let mut expect_gone = BTreeSet::new();
        for (it, f) in a_items.iter().zip(&preview.plan.ready) {
            prop_assert_eq!(&f.original_path, &in_a[f.of.item as usize - 1]);
            match it.change {
                Change::None => {
                    expect_gone.insert(f.path.clone());
                }
                Change::Modify => {
                    if it.dir {
                        fs::write(f.path.join("nuevo.txt"), b"nuevo").expect("write");
                    } else {
                        fs::write(&f.path, vec![b'y'; it.size + 1]).expect("write");
                    }
                }
                Change::Replace => {
                    remove(&f.path);
                    make(&f.path, it.dir, it.size);
                }
                Change::Remove => remove(&f.path),
            }
        }
        let home_before = snapshot(&home);
        let trash_before = snapshot(&trash);
        let trash_b_before = snapshot(&trash_b);

        let result = core.free(&preview.plan, &hash, "test").expect("free");

        // Solo desaparece lo de A que seguía siendo el objeto anotado, y
        // con todo su contenido; nada más cambia.
        let trash_after = snapshot(&trash);
        for (rel, m0) in &trash_before {
            let abs = trash.join(rel);
            if expect_gone.iter().any(|g| abs.starts_with(g)) {
                prop_assert!(!trash_after.contains_key(rel), "{:?} debía borrarse", rel);
                continue;
            }
            prop_assert_eq!(Some(m0), trash_after.get(rel), "{:?} cambió sin estar registrado", rel);
        }
        prop_assert!(trash_after.keys().all(|k| trash_before.contains_key(k)));
        prop_assert_eq!(&home_before, &snapshot(&home), "la carpeta personal cambió");
        prop_assert_eq!(&trash_b_before, &snapshot(&trash_b), "la otra Papelera cambió");

        let done: BTreeSet<PathBuf> = result
            .report
            .items
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Done { .. }))
            .map(|r| r.path.clone())
            .collect();
        prop_assert_eq!(&done, &expect_gone);
        prop_assert_eq!(
            result.report.skipped as usize,
            a_items.iter().filter(|it| it.change != Change::None).count()
        );
        prop_assert_eq!(result.space.estimated_bytes, result.report.bytes_done);
    }
}
