//! `rotten trash put <ruta…>` (T-10): selección explícita por el mismo
//! pipeline que `clean`.
//!
//! Cada prueba usa una carpeta personal temporal (`ROTTEN_HOME`) y una
//! Papelera temporal (`ROTTEN_TRASH_DIR`): nunca se toca el `~` real ni su
//! Papelera. Cada salida JSON se valida contra su esquema con
//! `common::validate` (T-01).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use assert_cmd::Command;
use ra_catalog::fixture::{Entry, materialize};
use serde_json::Value;

struct Fx {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    trash: PathBuf,
    state: PathBuf,
}

fn e(path: &str) -> Entry {
    Entry {
        path: path.into(),
        size: 700,
        age_days: 10,
        dir: false,
    }
}

/// Carpeta personal con Documentos, Escritorio y Descargas.
fn fixture() -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");
    let home = root.join("home");
    let trash = root.join("trash");
    fs::create_dir_all(&home).expect("mkdir");
    fs::create_dir_all(&trash).expect("mkdir");
    let entries = [
        "Downloads/setup.dmg",
        "Downloads/guardar/a.zip",
        "Desktop/carpeta/nota.txt",
        "Desktop/carpeta/sub/foto.jpg",
        "Documents/tesis.docx",
        "Library/Keychains/login.keychain-db",
        "Library/Logs/old.log",
    ]
    .map(e);
    materialize(&home, &entries, &[], SystemTime::now()).expect("materializar");
    let state = home.join("Library/Application Support/io.github.netsirius.RottenApple");
    fs::create_dir_all(&state).expect("mkdir");
    Fx {
        _tmp: tmp,
        home,
        trash,
        state,
    }
}

impl Fx {
    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("rotten").expect("binario rotten");
        c.env("ROTTEN_HOME", &self.home)
            .env("ROTTEN_TRASH_DIR", &self.trash)
            .env_remove("ROTTEN_STATE_DIR")
            .env("LANG", "en_US.UTF-8");
        c
    }

    fn run(&self, args: &[&str]) -> (i32, Value) {
        let out = self.cmd().args(args).output().expect("ejecutar");
        let code = out.status.code().expect("código");
        let json = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "salida no JSON de {args:?} ({e}):\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        if let Err(errors) = common::validate(&json) {
            panic!(
                "el JSON de {args:?} no valida contra su esquema:\n  {}\n{json:#}",
                errors.join("\n  ")
            );
        }
        (code, json)
    }

    fn p(&self, rel: &str) -> String {
        self.home.join(rel).to_string_lossy().into_owned()
    }
}

/// Todo lo que hay bajo `base` (sin seguir enlaces) salvo el estado de la app.
fn snapshot(base: &Path, skip: &Path) -> BTreeMap<PathBuf, (u64, bool, u64, i64, i64)> {
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
            out.insert(
                p.clone(),
                (m.ino(), m.is_dir(), m.size(), m.mtime(), m.mtime_nsec()),
            );
            if m.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

fn trash_is_empty(fx: &Fx) -> bool {
    fs::read_dir(&fx.trash).expect("trash").next().is_none()
}

#[test]
fn protected_paths_and_ancestors_exit_5_and_touch_nothing() {
    let fx = fixture();
    let before = snapshot(&fx.home, &fx.state);
    for bad in [
        fx.p("Library/Keychains/login.keychain-db"),
        fx.p("Library"),
        fx.home.to_string_lossy().into_owned(),
    ] {
        for dry in [true, false] {
            let mut args = vec!["trash", "put", "--json", "--yes"];
            if dry {
                args.push("--dry-run");
            }
            let ok = fx.p("Downloads/setup.dmg");
            args.push(&ok);
            args.push(&bad);
            let (code, doc) = fx.run(&args);
            assert_eq!(code, 5, "{bad} (dry-run: {dry}): {doc:#}");
            assert_eq!(doc["command"], "trash");
            assert_eq!(doc["error"]["exit_code"], 5);
        }
    }
    assert_eq!(snapshot(&fx.home, &fx.state), before);
    assert!(trash_is_empty(&fx));
}

#[test]
fn dry_run_shows_the_plan_with_its_origin_and_changes_nothing() {
    let fx = fixture();
    let before = snapshot(&fx.home, &fx.state);
    let (code, doc) = fx.run(&[
        "trash",
        "put",
        "--dry-run",
        "--json",
        &fx.p("Downloads/setup.dmg"),
        &fx.p("Desktop/carpeta"),
    ]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["command"], "trash");
    assert_eq!(doc["action"], "put");
    assert_eq!(doc["dry_run"], true);
    let plan = &doc["plan"];
    assert_eq!(plan["origin"], "explicit_selection");
    let items = plan["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    for i in items {
        assert_eq!(i["origin"], "explicit_selection");
        assert_eq!(i["preselected"], false);
        assert_eq!(i["action"], "trash");
    }
    assert_eq!(doc["preview"]["would"], 2);
    // El plan se puede volver a cargar con su hash.
    ra_core::Plan::from_json(&plan.to_string()).expect("plan válido");
    assert_eq!(snapshot(&fx.home, &fx.state), before);
    assert!(trash_is_empty(&fx));
}

#[test]
fn excluded_paths_do_not_enter_the_plan() {
    let fx = fixture();
    let (code, _) = fx.run(&["exclude", "add", "~/Downloads/guardar", "--json"]);
    assert_eq!(code, 0);
    let (code, doc) = fx.run(&[
        "trash",
        "put",
        "--yes",
        "--json",
        &fx.p("Downloads/setup.dmg"),
        &fx.p("Downloads/guardar/a.zip"),
    ]);
    // Una de las rutas pedidas se queda fuera: completado con omisiones.
    assert_eq!(code, 4, "{doc:#}");
    let items = doc["plan"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["path"], fx.p("Downloads/setup.dmg"));
    assert_eq!(
        doc["plan"]["skipped"][0]["path"],
        fx.p("Downloads/guardar/a.zip")
    );
    assert_eq!(doc["plan"]["skipped"][0]["reason"]["kind"], "excluded");
    assert_eq!(doc["report"]["done"], 1);
    assert!(Path::new(&fx.p("Downloads/guardar/a.zip")).exists());
}

#[test]
fn repeated_or_nested_paths_are_not_omissions() {
    let fx = fixture();
    let dir = fx.p("Desktop/carpeta");
    let inner = fx.p("Desktop/carpeta/nota.txt");
    let (code, doc) = fx.run(&["trash", "put", "--yes", "--json", &dir, &inner, &dir]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 1);
    assert!(!Path::new(&dir).exists());
}

#[test]
fn without_terminal_or_yes_is_a_usage_error() {
    let fx = fixture();
    let (code, doc) = fx.run(&["trash", "put", &fx.p("Downloads/setup.dmg")]);
    assert_eq!(code, 2, "{doc:#}");
    assert!(Path::new(&fx.p("Downloads/setup.dmg")).exists());
    // Sin rutas, clap lo rechaza también con 2.
    let out = fx.cmd().args(["trash", "put"]).output().expect("ejecutar");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn put_then_undo_leaves_the_tree_as_it_was() {
    let fx = fixture();
    let before = snapshot(&fx.home, &fx.state);
    let targets = [fx.p("Downloads/setup.dmg"), fx.p("Desktop/carpeta")];
    let (code, doc) = fx.run(&["trash", "put", "--yes", "--json", &targets[0], &targets[1]]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 2);
    let op = doc["report"]["op_id"].as_str().expect("op_id").to_owned();
    for t in &targets {
        assert!(!Path::new(t).exists(), "{t} debería estar en la Papelera");
    }
    assert!(Path::new(&fx.p("Documents/tesis.docx")).exists());

    // El journal anota el origen de cada elemento.
    let journal = fs::read_dir(fx.state.join("journal"))
        .expect("journal")
        .flatten()
        .map(|e| fs::read_to_string(e.path()).expect("leer"))
        .collect::<String>();
    let intents: Vec<Value> = journal
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["ev"] == "item_intent" && v["op"] == op.as_str())
        .collect();
    assert_eq!(intents.len(), 2);
    assert!(intents.iter().all(|v| v["origin"] == "explicit_selection"));

    let (code, hist) = fx.run(&["history", "--json"]);
    assert_eq!(code, 0);
    assert_eq!(hist["operations"][0]["undo_state"], "restorable");

    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 2);
    assert!(trash_is_empty(&fx));
    let after = snapshot(&fx.home, &fx.state);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>()
    );
    for (p, m0) in &before {
        let m1 = &after[p];
        assert_eq!(m0.0, m1.0, "{p:?} no ha vuelto (mismo inode)");
        // Mover cambia el mtime de las carpetas padre; los ficheros, intactos.
        if !m0.1 {
            assert_eq!(m0, m1, "{p:?}");
        }
    }
}
