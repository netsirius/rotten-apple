//! Reconciliación tras un cierre abrupto (05 §5.6, RF-HIS-02): se simula
//! que el proceso muere entre la intención y el resultado.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use ra_catalog::fixture::{Entry, materialize};
use ra_core::{Action, Core, Env, Selection, TrashBackend, UndoState};
use ra_history::{Event, HistoryPaths, Journal, OpKind, OpMode, StepAction};

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
