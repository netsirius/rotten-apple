//! Reconciliación tras un cierre abrupto (05 §5.6, RF-HIS-02): se simula
//! que el proceso muere entre la intención y el resultado.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use ra_catalog::fixture::{Entry, materialize};
use ra_core::{Action, Core, Env, Selection, TrashBackend, UndoState};
use ra_history::{Event, HistoryPaths, Journal, OpKind, OpMode, StepAction, read_journal};

struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    core: Core,
}

fn world() -> World {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");
    let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
    for d in [&home, &trash, &state] {
        fs::create_dir_all(d).expect("mkdir");
    }
    let entries = ["Library/Logs/a.log", "Library/Logs/b.log"].map(|p| Entry {
        path: p.into(),
        size: 100,
        age_days: 30,
        dir: false,
    });
    materialize(&home, &entries, &[], SystemTime::now()).expect("materializar");
    let env = Env::new(home.clone(), state.clone(), TrashBackend::Dir(trash));
    let core = Core::with_apps(env, Box::new(BTreeSet::<String>::new())).expect("core");
    World {
        _tmp: tmp,
        home,
        state,
        core,
    }
}

fn clean(w: &World) -> String {
    let preview = w
        .core
        .plan(&Selection::default(), Action::Trash)
        .expect("plan");
    let hash = preview.plan.plan_hash().to_owned();
    let report = w
        .core
        .execute(preview.plan, &hash, "test")
        .expect("ejecución");
    assert_eq!(report.done, 2);
    report.op_id.expect("op")
}

fn state_of(w: &World, op: &str) -> UndoState {
    let (ops, _) = w.core.history().expect("historial");
    ops.into_iter()
        .find(|o| o.op_id == op)
        .expect("op")
        .undo_state
}

/// El deshacer muere tras mover el elemento 1 pero antes de anotarlo; el
/// elemento 2 ni se empezó. Al reconciliar, el 1 cuenta como restaurado.
#[test]
fn interrupted_undo_is_reconciled_as_restored() {
    let w = world();
    let op = clean(&w);
    let undo = w.core.prepare_undo(Some(&op), None).expect("deshacer");
    let first = undo.plan.ready[0].clone();

    let paths = HistoryPaths::new(&w.state);
    let mut j = Journal::open(&paths, "op-undo-crash", SystemTime::now()).expect("journal");
    j.write(
        Event::OpStarted {
            kind: OpKind::Undo,
            mode: OpMode::Restore,
            plan_hash: None,
            undo_of: Some(op.clone()),
            items: 2,
            bytes: 0,
            by: "test".into(),
            trash: "dir".into(),
        },
        true,
    )
    .expect("start");
    j.write(
        Event::ItemIntent {
            item: first.item_id,
            action: StepAction::Restore,
            path: first.from.clone(),
            identity: first.identity,
            target: Some(first.to.clone()),
            category: None,
            regenerable: first.regenerable,
            bytes: first.bytes,
            parent_mode: None,
            origin: None,
        },
        true,
    )
    .expect("intent");
    fs::rename(&first.from, &first.to).expect("mover");
    drop(j); // «Cierre abrupto»: sin resultado ni op_finished.

    assert_eq!(w.core.recover().expect("recuperar"), vec!["op-undo-crash"]);
    assert!(
        w.core.recover().expect("recuperar").is_empty(),
        "solo una vez"
    );

    // Uno restaurado y otro aún en la Papelera.
    assert_eq!(state_of(&w, &op), UndoState::Partial);
    let rest = w.core.prepare_undo(Some(&op), None).expect("deshacer");
    assert_eq!(rest.plan.ready.len(), 1);
    assert_ne!(rest.plan.ready[0].item_id, first.item_id);
    w.core.undo(&rest.plan, "test").expect("restaurar");
    assert_eq!(state_of(&w, &op), UndoState::Restored);
    assert!(w.home.join("Library/Logs/a.log").exists());
    assert!(w.home.join("Library/Logs/b.log").exists());
}

/// Una limpieza muere tras escribir la intención y antes de mover: el
/// elemento sigue en su sitio y se marca como no movido.
#[test]
fn interrupted_clean_before_acting_is_not_moved() {
    let w = world();
    let preview = w
        .core
        .plan(&Selection::default(), Action::Trash)
        .expect("plan");
    let item = preview.plan.items()[0].clone();
    let paths = HistoryPaths::new(&w.state);
    let mut j = Journal::open(&paths, "op-clean-crash", SystemTime::now()).expect("journal");
    j.write(
        Event::OpStarted {
            kind: OpKind::Clean,
            mode: OpMode::Trash,
            plan_hash: Some(preview.plan.plan_hash().into()),
            undo_of: None,
            items: 1,
            bytes: 0,
            by: "test".into(),
            trash: "dir".into(),
        },
        true,
    )
    .expect("start");
    j.write(
        Event::ItemIntent {
            item: item.item_id,
            action: StepAction::Trash,
            path: item.path.clone(),
            identity: item.identity,
            target: None,
            category: Some(item.category.clone()),
            regenerable: item.regenerable,
            bytes: item.bytes_allocated,
            parent_mode: None,
            origin: None,
        },
        true,
    )
    .expect("intent");
    drop(j);

    assert_eq!(w.core.recover().expect("recuperar"), vec!["op-clean-crash"]);
    assert!(item.path.exists());
    assert_eq!(state_of(&w, "op-clean-crash"), UndoState::NotApplicable);
    let (ops, _) = w.core.history().expect("historial");
    let op = ops
        .iter()
        .find(|o| o.op_id == "op-clean-crash")
        .expect("op");
    assert_eq!(op.items_skipped, 1);
    assert_eq!(op.items_done, 0);
}

/// Escribe el inicio y la intención de una operación de un elemento y la
/// deja sin terminar, como tras un cierre abrupto.
fn interrupted(w: &World, op: &str, mode: OpMode, action: StepAction) -> PathBuf {
    let preview = w
        .core
        .plan(&Selection::default(), Action::Trash)
        .expect("plan");
    let item = preview.plan.items()[0].clone();
    let mut j =
        Journal::open(&HistoryPaths::new(&w.state), op, SystemTime::now()).expect("journal");
    j.write(
        Event::OpStarted {
            kind: OpKind::Clean,
            mode,
            plan_hash: Some(preview.plan.plan_hash().into()),
            undo_of: None,
            items: 1,
            bytes: 0,
            by: "test".into(),
            trash: "dir".into(),
        },
        true,
    )
    .expect("start");
    j.write(
        Event::ItemIntent {
            item: item.item_id,
            action,
            path: item.path.clone(),
            identity: item.identity,
            target: None,
            category: Some(item.category.clone()),
            regenerable: item.regenerable,
            bytes: item.bytes_allocated,
            parent_mode: None,
            origin: None,
        },
        true,
    )
    .expect("intent");
    item.path
}

/// Contadores del `op_finished` que escribió la reconciliación.
fn finished(w: &World, op: &str) -> (u64, u64, u64) {
    let (records, _) = read_journal(&HistoryPaths::new(&w.state)).expect("journal");
    records
        .iter()
        .filter(|r| r.op == op)
        .find_map(|r| match r.event {
            Event::OpFinished {
                done,
                skipped,
                unknown,
                ..
            } => Some((done, skipped, unknown)),
            _ => None,
        })
        .expect("op_finished")
}

/// M-3: un borrado permanente interrumpido cuyo original ya no está queda
/// como borrado (hecho e irreversible), no como desconocido.
#[test]
fn interrupted_delete_whose_original_is_gone_is_deleted() {
    let w = world();
    let path = interrupted(&w, "op-delete-crash", OpMode::Permanent, StepAction::Delete);
    fs::remove_file(&path).expect("borrar");

    assert_eq!(
        w.core.recover().expect("recuperar"),
        vec!["op-delete-crash"]
    );
    assert_eq!(finished(&w, "op-delete-crash"), (1, 0, 0));
    let (ops, _) = w.core.history().expect("historial");
    let op = ops
        .iter()
        .find(|o| o.op_id == "op-delete-crash")
        .expect("op");
    assert_eq!(op.items_done, 1);
    assert_eq!(op.undo_state, UndoState::Irreversible);
}

/// M-3: un elemento que no está en su sitio ni en la Papelera queda
/// desconocido y no cuenta como hecho.
#[test]
fn interrupted_trash_not_found_anywhere_is_unknown_not_done() {
    let w = world();
    let path = interrupted(&w, "op-lost-crash", OpMode::Trash, StepAction::Trash);
    fs::remove_file(&path).expect("borrar");

    assert_eq!(w.core.recover().expect("recuperar"), vec!["op-lost-crash"]);
    assert_eq!(finished(&w, "op-lost-crash"), (0, 0, 1));
    let (ops, _) = w.core.history().expect("historial");
    let op = ops.iter().find(|o| o.op_id == "op-lost-crash").expect("op");
    assert_eq!(op.items_done, 0);
    assert_eq!(op.undo_state, UndoState::Irreversible);
}
