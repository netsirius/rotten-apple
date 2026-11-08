//! Pruebas de integración del CLI `rotten` (07 §2 RF-CLI-20/21, README §8).
//!
//! La carpeta personal de prueba está en `common`, que además valida cada
//! salida JSON contra `schemas/` (T-01).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use common::fixture;
use ra_core::Plan;

/// Todo lo que hay bajo `base` (sin seguir enlaces) salvo el estado de la app.
fn snapshot(base: &Path, skip: &Path) -> BTreeMap<PathBuf, (u64, u64, i64, i64)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.starts_with(skip) {
                continue;
            }
            let m = fs::symlink_metadata(&p).expect("lstat");
            out.insert(p.clone(), (m.ino(), m.size(), m.mtime(), m.mtime_nsec()));
            if m.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

fn ino(p: &Path) -> u64 {
    fs::symlink_metadata(p).expect("lstat").ino()
}

#[test]
fn clean_dry_run_json_produces_a_valid_plan_and_changes_nothing() {
    let fx = fixture();
    let before = snapshot(&fx.home, &fx.state);
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["command"], "clean");
    assert_eq!(doc["dry_run"], true);

    // El plan se vuelve a cargar y su hash se verifica.
    let plan = Plan::from_json(&doc["plan"].to_string()).expect("plan válido con hash correcto");
    assert_eq!(plan.home(), fx.home);
    let paths: Vec<PathBuf> = plan.items().iter().map(|i| i.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            fx.p("Library/Logs/install.log"),
            fx.p("Library/Logs/old-app"),
            fx.p("Library/Saved Application State/com.example.closed.savedState"),
        ]
    );
    for i in plan.items() {
        assert_eq!(i.action, ra_core::Action::Trash);
        assert!(i.reversible);
    }
    // Omitido con motivo: el log reciente.
    let skipped: Vec<&Path> = plan.skipped().iter().map(|s| s.path.as_path()).collect();
    assert!(skipped.contains(&fx.p("Library/Logs/current.log").as_path()));
    // CAT-01 queda sin marcar.
    assert_eq!(plan.not_selected()["CAT-01"].items, 1);
    // La simulación prevé mover cada elemento.
    let preview = doc["preview"]["items"].as_array().expect("items");
    assert_eq!(preview.len(), 3);
    assert!(preview.iter().all(|i| i["action"] == "would_trash"));

    assert_eq!(
        snapshot(&fx.home, &fx.state),
        before,
        "--dry-run no cambia nada"
    );
    assert!(fs::read_dir(&fx.trash).expect("trash").next().is_none());
}

#[test]
fn json_is_automatic_when_piped() {
    let fx = fixture();
    let (code, doc) = fx.run(&["scan"]);
    assert_eq!(code, 0);
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["scan_status"], "complete");
    let cats: Vec<&str> = doc["categories"]
        .as_array()
        .expect("categorías")
        .iter()
        .map(|c| c["id"].as_str().expect("id"))
        .collect();
    assert_eq!(cats, ["CAT-01", "CAT-03", "CAT-08", "CAT-10"]);
}

#[test]
fn clean_yes_then_undo_puts_everything_back() {
    let fx = fixture();
    let targets = [
        fx.p("Library/Logs/install.log"),
        fx.p("Library/Logs/old-app"),
        fx.p("Library/Saved Application State/com.example.closed.savedState"),
    ];
    let inodes: Vec<u64> = targets.iter().map(|p| ino(p)).collect();
    let before = snapshot(&fx.home, &fx.state);

    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let report = &doc["report"];
    assert_eq!(report["done"], 3);
    let op = report["op_id"].as_str().expect("op_id").to_owned();
    for t in &targets {
        assert!(!t.exists(), "{t:?} debería estar en la Papelera");
    }
    for item in report["items"].as_array().expect("items") {
        let to = PathBuf::from(item["result_path"].as_str().expect("result_path"));
        assert!(
            to.starts_with(&fx.trash),
            "{to:?} fuera de la Papelera de prueba"
        );
        assert!(fs::symlink_metadata(&to).is_ok());
    }
    // Lo demás, intacto.
    assert!(fx.p("Documents/tesis.docx").exists());
    assert!(fx.p("Library/Caches/com.example.editor/data.bin").exists());
    assert!(fx.p("Library/Logs/current.log").exists());

    let (code, hist) = fx.run(&["history", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(hist["operations"][0]["op_id"], op.as_str());
    assert_eq!(hist["operations"][0]["undo_state"], "restorable");

    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 3);
    for (t, i) in targets.iter().zip(&inodes) {
        assert_eq!(ino(t), *i, "{t:?} no ha vuelto (mismo inode)");
    }
    assert!(fs::read_dir(&fx.trash).expect("trash").next().is_none());
    let after = snapshot(&fx.home, &fx.state);
    let files = |s: &BTreeMap<PathBuf, (u64, u64, i64, i64)>| -> Vec<(PathBuf, u64)> {
        s.iter().map(|(p, m)| (p.clone(), m.0)).collect()
    };
    assert_eq!(files(&after), files(&before));

    let (_, hist) = fx.run(&["history", "--json"]);
    let clean = hist["operations"]
        .as_array()
        .expect("ops")
        .iter()
        .find(|o| o["op_id"] == op.as_str())
        .expect("op");
    assert_eq!(clean["undo_state"], "restored");
}

#[test]
fn undo_by_items_and_when_trash_was_emptied() {
    let fx = fixture();
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0);
    let items = doc["report"]["items"].as_array().expect("items").clone();
    // «Vaciar la Papelera» para el elemento 1.
    let gone = PathBuf::from(items[0]["result_path"].as_str().expect("ruta"));
    if gone.is_dir() {
        fs::remove_dir_all(&gone).expect("rm");
    } else {
        fs::remove_file(&gone).expect("rm");
    }
    // Deshacer solo el elemento 2.
    let (code, doc) = fx.run(&["undo", "--items", "2", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 1);
    assert!(PathBuf::from(items[1]["path"].as_str().expect("ruta")).exists());
    // El resto: uno restaurable, otro ya no → código 4.
    let (code, doc) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(code, 4, "{doc:#}");
    assert_eq!(doc["report"]["done"], 1);
    assert_eq!(doc["not_recoverable"].as_array().expect("lista").len(), 2);
    // Dos restaurados y uno ya no está: «restaurada en parte».
    let (_, hist) = fx.run(&["history", "--json"]);
    assert_eq!(hist["operations"][2]["undo_state"], "partly_restored");
}

#[test]
fn emptying_the_trash_makes_an_operation_irreversible() {
    let fx = fixture();
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0);
    for item in doc["report"]["items"].as_array().expect("items") {
        let p = PathBuf::from(item["result_path"].as_str().expect("ruta"));
        if p.is_dir() {
            fs::remove_dir_all(&p).expect("rm");
        } else {
            fs::remove_file(&p).expect("rm");
        }
    }
    let (_, hist) = fx.run(&["history", "--json"]);
    assert_eq!(hist["operations"][0]["undo_state"], "irreversible");
    let (code, doc) = fx.run(&["undo", "--json", "--yes"]);
    assert_eq!(code, 1, "no queda nada que deshacer: {doc:#}");
}

#[test]
fn no_terminal_without_yes_is_a_usage_error() {
    let fx = fixture();
    let before = snapshot(&fx.home, &fx.state);
    let (code, doc) = fx.run(&["clean"]);
    assert_eq!(code, 2);
    assert_eq!(doc["error"]["exit_code"], 2);
    let (code, _) = fx.run(&["clean", "--permanent"]);
    assert_eq!(code, 2, "--permanent exige --yes");
    assert_eq!(snapshot(&fx.home, &fx.state), before);
}

#[test]
fn corrupt_exclusions_abort_with_code_5() {
    let fx = fixture();
    fs::create_dir_all(&fx.state).expect("mkdir");
    let config = fx.state.join("config.toml");
    fs::write(&config, "[exclusions\npaths = [").expect("write");
    let before = snapshot(&fx.home, &fx.state);

    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 5, "{doc:#}");
    let (code, _) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 5);
    let (code, _) = fx.run(&["exclude", "add", "~/Library/Logs", "--json"]);
    assert_eq!(code, 5, "no se sobrescribe un fichero corrupto");
    assert_eq!(
        fs::read_to_string(&config).expect("config"),
        "[exclusions\npaths = ["
    );
    // El escaneo sigue, con aviso.
    let (code, doc) = fx.run(&["scan", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(doc["config"], "corrupt");
    assert_eq!(doc["scan_status"], "complete");
    let (code, _) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 5);

    assert_eq!(snapshot(&fx.home, &fx.state), before);
    assert!(fs::read_dir(&fx.trash).expect("trash").next().is_none());
}

#[test]
fn exclusions_are_honoured() {
    let fx = fixture();
    let (code, doc) = fx.run(&["exclude", "add", "~/Library/Logs/old-app", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["changed"], true);
    let (_, doc) = fx.run(&["exclude", "list", "--json"]);
    assert_eq!(doc["exclusions"][0], "~/Library/Logs/old-app");

    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0);
    let plan = Plan::from_json(&doc["plan"].to_string()).expect("plan");
    assert!(
        plan.items()
            .iter()
            .all(|i| i.path != fx.p("Library/Logs/old-app"))
    );
    assert!(
        plan.skipped()
            .iter()
            .any(|s| s.path == fx.p("Library/Logs/old-app"))
    );

    let (code, doc) = fx.run(&["exclude", "remove", "~/Library/Logs/old-app", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(doc["changed"], true);
    let (code, _) = fx.run(&["exclude", "add", "relativa/ruta", "--json"]);
    assert_eq!(code, 2);
}

#[test]
fn category_selection_includes_unmarked_safe_items() {
    let fx = fixture();
    let (code, doc) = fx.run(&["clean", "--dry-run", "--category", "CAT-01", "--json"]);
    assert_eq!(code, 0);
    let plan = Plan::from_json(&doc["plan"].to_string()).expect("plan");
    let paths: Vec<PathBuf> = plan.items().iter().map(|i| i.path.clone()).collect();
    assert_eq!(paths, vec![fx.p("Library/Caches/com.example.editor")]);
    let (code, _) = fx.run(&["clean", "--dry-run", "--category", "CAT-99", "--json"]);
    assert_eq!(code, 2);
}

#[test]
fn another_executor_running_exits_6() {
    let fx = fixture();
    fs::create_dir_all(&fx.state).expect("mkdir");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fx.state.join("exec.lock"))
        .expect("lock");
    lock.try_lock().expect("tomar el cerrojo");
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 6, "{doc:#}");
    assert!(fx.p("Library/Logs/install.log").exists());
    drop(lock);
}

#[test]
fn doctor_and_history_report_json() {
    let fx = fixture();
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let r = &doc["report"];
    assert_eq!(r["home"], fx.home.to_str().expect("utf-8"));
    assert_eq!(r["home_source"], "ROTTEN_HOME");
    assert_eq!(r["config"], "missing");
    assert_eq!(r["full_disk_access"], "undetermined");
    assert!(r["trash"].as_str().expect("trash").starts_with("dir:"));
    let (code, doc) = fx.run(&["history", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(doc["operations"].as_array().expect("ops").len(), 0);
}

#[test]
fn usage_errors_exit_2() {
    let fx = fixture();
    let out = fx.cmd().output().expect("ejecutar");
    assert_eq!(out.status.code(), Some(2));
    let out = fx
        .cmd()
        .args(["clean", "--no-such-flag"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(2));
    let (code, _) = fx.run(&["undo", "no-such-op", "--json"]);
    assert_eq!(code, 1);
}
