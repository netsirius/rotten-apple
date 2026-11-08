//! Esquemas JSON del CLI (T-01: RNF-ARQ-22, RF-CLI-04, RF-CLI-21).
//!
//! - Cada salida de `cli.rs` se repite aquí, se valida contra
//!   `schemas/<comando>.v1.schema.json` (lo hace `Fx::run`) y se compara con
//!   su *snapshot* de `insta`, con rutas, fechas, IDs y hashes ocultos.
//! - Renombrar o quitar una clave sin subir `schema_version` hace fallar
//!   la validación (`renaming_any_key_breaks_validation`).
//!
//! Para regenerar los *snapshots* tras un cambio intencionado:
//! `INSTA_UPDATE=always cargo test -p rotten --test schemas` y revisar el
//! diff antes de hacer commit.

mod common;

use std::fs;

use common::{Fx, fixture, schema_files, validate};
use serde_json::{Map, Value, json};

/// Claves cuyo valor cambia en cada ejecución o depende de la máquina.
const VOLATILE: &[&str] = &[
    "created_at",
    "started_at",
    "finished_at",
    "plan_id",
    "plan_hash",
    "op_id",
    "undo_of",
    "digest",
    "newest_mtime",
    "dev",
    "ino",
    "mtime",
    "mtime_nsec",
    "ctime",
    "ctime_nsec",
    "parent_mode",
    "macos_version",
    "arch",
    "running_apps_probe",
];

/// Oculta lo que no es estable: rutas del fixture, fechas, IDs, hashes,
/// identidades de fichero y datos de la máquina. El tamaño de las carpetas
/// en `identity.size` depende del sistema de ficheros y también se oculta.
fn redact(fx: &Fx, v: &Value) -> Value {
    redact_in(fx, v, None)
}

fn redact_in(fx: &Fx, v: &Value, key: Option<&str>) -> Value {
    if let Some(k) = key
        && VOLATILE.contains(&k)
        && !v.is_null()
    {
        return json!(format!("[{k}]"));
    }
    match v {
        Value::String(s) => {
            let home = fx.home.to_str().expect("ruta UTF-8");
            let trash = fx.trash.to_str().expect("ruta UTF-8");
            json!(s.replace(trash, "[TRASH]").replace(home, "[HOME]"))
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| redact_in(fx, x, None)).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                let mut r = redact_in(fx, x, Some(k));
                if k == "identity"
                    && let Value::Object(id) = &mut r
                {
                    id.insert("size".into(), json!("[size]"));
                }
                out.insert(k.clone(), r);
            }
            // La versión del binario en `doctor`.
            if key == Some("report") && out.contains_key("catalog_version") {
                out.insert("version".into(), json!("[version]"));
            }
            Value::Object(out)
        }
        _ => v.clone(),
    }
}

/// Ejecuta, comprueba el código de salida y guarda el *snapshot*. `Fx::run`
/// ya ha validado el documento contra su esquema.
fn check(fx: &Fx, name: &str, args: &[&str], code: i32) -> Value {
    let (got, doc) = fx.run(args);
    assert_eq!(got, code, "{args:?}: {doc:#}");
    insta::assert_json_snapshot!(name, redact(fx, &doc));
    doc
}

fn write_corrupt_config(fx: &Fx) {
    fs::write(fx.state.join("config.toml"), "[exclusions\npaths = [").expect("write");
}

#[test]
fn schemas_are_valid_and_match_their_file_name() {
    let files = schema_files();
    let commands: Vec<&str> = files.keys().map(|(c, _)| c.as_str()).collect();
    assert_eq!(
        commands,
        [
            "clean", "doctor", "exclude", "history", "scan", "trash", "undo"
        ]
    );
    for ((command, version), (path, schema)) in &files {
        if let Err(e) = jsonschema::meta::validate(schema) {
            panic!("{}: no es un JSON Schema válido: {e}", path.display());
        }
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema",
            "{}",
            path.display()
        );
        // Todas las variantes fijan el comando y la versión del fichero.
        let defs = schema["$defs"].as_object().expect("$defs");
        for variant in schema["oneOf"].as_array().expect("oneOf") {
            let r = variant["$ref"].as_str().expect("$ref");
            let name = r.strip_prefix("#/$defs/").expect("ref local");
            let props = &defs[name]["properties"];
            assert_eq!(props["command"]["const"], command.as_str(), "{r}");
            assert_eq!(props["schema_version"]["const"], *version, "{r}");
        }
    }
}

#[test]
fn scan() {
    let fx = fixture();
    check(&fx, "scan", &["scan", "--json"], 0);
    check(
        &fx,
        "scan_category",
        &["scan", "--category", "CAT-03", "--json"],
        0,
    );
    write_corrupt_config(&fx);
    check(&fx, "scan_config_corrupt", &["scan", "--json"], 0);
}

#[test]
fn clean() {
    let fx = fixture();
    check(&fx, "clean_dry_run", &["clean", "--dry-run", "--json"], 0);
    check(
        &fx,
        "clean_dry_run_category",
        &["clean", "--dry-run", "--category", "CAT-01", "--json"],
        0,
    );
    check(
        &fx,
        "clean_error_unknown_category",
        &["clean", "--dry-run", "--category", "CAT-99", "--json"],
        2,
    );
    check(&fx, "clean_error_no_yes", &["clean", "--json"], 2);
    check(
        &fx,
        "clean_error_permanent_no_yes",
        &["clean", "--permanent", "--json"],
        2,
    );
    check(&fx, "clean_executed", &["clean", "--yes", "--json"], 0);
    check(
        &fx,
        "clean_executed_empty",
        &["clean", "--yes", "--json"],
        0,
    );
}

#[test]
fn clean_with_exclusion_lock_and_corrupt_config() {
    let fx = fixture();
    fx.run(&["exclude", "add", "~/Library/Logs/old-app", "--json"]);
    check(
        &fx,
        "clean_dry_run_excluded",
        &["clean", "--dry-run", "--json"],
        0,
    );

    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fx.state.join("exec.lock"))
        .expect("lock");
    lock.try_lock().expect("tomar el cerrojo");
    check(&fx, "clean_error_locked", &["clean", "--yes", "--json"], 6);
    drop(lock);

    write_corrupt_config(&fx);
    check(
        &fx,
        "clean_error_config_corrupt",
        &["clean", "--dry-run", "--json"],
        5,
    );
}

#[test]
fn history_and_undo() {
    let fx = fixture();
    check(&fx, "history_empty", &["history", "--json"], 0);
    check(
        &fx,
        "undo_error_unknown_op",
        &["undo", "no-such-op", "--json"],
        1,
    );
    let (_, doc) = fx.run(&["clean", "--yes", "--json"]);
    check(&fx, "history_restorable", &["history", "--json"], 0);
    check(&fx, "undo_dry_run", &["undo", "--dry-run", "--json"], 0);
    check(
        &fx,
        "undo_items_executed",
        &["undo", "--items", "2", "--yes", "--json"],
        0,
    );
    check(&fx, "history_partial", &["history", "--json"], 0);

    // «Vaciar la Papelera» para el elemento 1: queda no recuperable.
    let gone = doc["report"]["items"][0]["result_path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let gone = std::path::Path::new(&gone);
    if gone.is_dir() {
        fs::remove_dir_all(gone).expect("rm");
    } else {
        fs::remove_file(gone).expect("rm");
    }
    check(
        &fx,
        "undo_dry_run_not_recoverable",
        &["undo", "--dry-run", "--json"],
        0,
    );
    check(
        &fx,
        "undo_executed_partial",
        &["undo", "--yes", "--json"],
        4,
    );
    check(&fx, "history_partly_restored", &["history", "--json"], 0);
    check(&fx, "undo_error_nothing", &["undo", "--yes", "--json"], 1);
}

#[test]
fn history_irreversible_and_undo_nothing_left() {
    let fx = fixture();
    let (_, doc) = fx.run(&["clean", "--yes", "--json"]);
    for item in doc["report"]["items"].as_array().expect("items") {
        let p = std::path::PathBuf::from(item["result_path"].as_str().expect("ruta"));
        if p.is_dir() {
            fs::remove_dir_all(&p).expect("rm");
        } else {
            fs::remove_file(&p).expect("rm");
        }
    }
    check(&fx, "history_irreversible", &["history", "--json"], 0);
    check(
        &fx,
        "undo_error_irreversible",
        &["undo", "--yes", "--json"],
        1,
    );
}

#[test]
fn undo_everything() {
    let fx = fixture();
    fx.run(&["clean", "--yes", "--json"]);
    check(&fx, "undo_executed", &["undo", "--yes", "--json"], 0);
    check(&fx, "history_restored", &["history", "--json"], 0);
}

#[test]
fn trash_put() {
    let fx = fixture();
    let cache = fx.p("Library/Caches/com.example.editor");
    let cache = cache.to_str().expect("ruta UTF-8");
    let keychain = fx.p("Library/Keychains/login.keychain-db");
    let keychain = keychain.to_str().expect("ruta UTF-8");
    check(
        &fx,
        "trash_put_dry_run",
        &["trash", "put", cache, "--dry-run", "--json"],
        0,
    );
    check(
        &fx,
        "trash_put_error_protected",
        &["trash", "put", cache, keychain, "--yes", "--json"],
        5,
    );
    check(
        &fx,
        "trash_put_error_no_yes",
        &["trash", "put", cache, "--json"],
        2,
    );
    check(
        &fx,
        "trash_put_executed",
        &["trash", "put", cache, "--yes", "--json"],
        0,
    );
}

#[test]
fn doctor() {
    let fx = fixture();
    check(&fx, "doctor", &["doctor", "--json"], 0);
    write_corrupt_config(&fx);
    check(&fx, "doctor_config_corrupt", &["doctor", "--json"], 5);
}

#[test]
fn exclude() {
    let fx = fixture();
    check(&fx, "exclude_list_empty", &["exclude", "list", "--json"], 0);
    check(
        &fx,
        "exclude_add_dry_run",
        &[
            "exclude",
            "add",
            "~/Library/Logs/old-app",
            "--dry-run",
            "--json",
        ],
        0,
    );
    check(
        &fx,
        "exclude_add",
        &["exclude", "add", "~/Library/Logs/old-app", "--json"],
        0,
    );
    check(
        &fx,
        "exclude_add_again",
        &["exclude", "add", "~/Library/Logs/old-app", "--json"],
        0,
    );
    check(&fx, "exclude_list", &["exclude", "list", "--json"], 0);
    check(
        &fx,
        "exclude_remove",
        &["exclude", "remove", "~/Library/Logs/old-app", "--json"],
        0,
    );
    check(
        &fx,
        "exclude_error_relative",
        &["exclude", "add", "relativa/ruta", "--json"],
        2,
    );
    write_corrupt_config(&fx);
    check(
        &fx,
        "exclude_error_config_corrupt",
        &["exclude", "add", "~/Library/Logs", "--json"],
        5,
    );
}

/// Un documento de cada forma: éxito y error de cada comando.
fn sample_documents() -> Vec<Value> {
    let fx = fixture();
    let mut docs = vec![
        fx.run(&["scan", "--json"]).1,
        fx.run(&["doctor", "--json"]).1,
        fx.run(&["exclude", "list", "--json"]).1,
        fx.run(&["exclude", "add", "~/Library/Caches", "--dry-run", "--json"])
            .1,
        fx.run(&["clean", "--json"]).1,
        fx.run(&["clean", "--dry-run", "--json"]).1,
        fx.run(&["clean", "--yes", "--json"]).1,
        fx.run(&["history", "--json"]).1,
        fx.run(&["undo", "--dry-run", "--json"]).1,
        fx.run(&["undo", "--yes", "--json"]).1,
    ];
    docs.push(fx.run(&["undo", "no-such-op", "--json"]).1);
    let cache = fx.p("Library/Caches/com.example.editor");
    let cache = cache.to_str().expect("ruta UTF-8");
    docs.push(fx.run(&["trash", "put", cache, "--dry-run", "--json"]).1);
    docs.push(fx.run(&["trash", "put", cache, "--yes", "--json"]).1);
    docs
}

/// Todas las rutas de claves de objeto hasta `depth` niveles, entrando en
/// el primer elemento de cada lista.
fn key_paths(v: &Value, prefix: &mut Vec<String>, depth: usize, out: &mut Vec<Vec<String>>) {
    if depth == 0 {
        return;
    }
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                prefix.push(k.clone());
                out.push(prefix.clone());
                key_paths(x, prefix, depth - 1, out);
                prefix.pop();
            }
        }
        Value::Array(a) => {
            if let Some(first) = a.first() {
                prefix.push("0".into());
                key_paths(first, prefix, depth, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

fn parent_mut<'a>(v: &'a mut Value, path: &[String]) -> &'a mut Map<String, Value> {
    let mut cur = v;
    for seg in path {
        cur = match cur {
            Value::Object(m) => m.get_mut(seg).expect("clave"),
            Value::Array(a) => a
                .get_mut(seg.parse::<usize>().expect("índice"))
                .expect("elemento"),
            _ => panic!("ruta no válida"),
        };
    }
    cur.as_object_mut().expect("objeto")
}

/// RNF-ARQ-22: renombrar o quitar cualquier clave (hasta tres niveles, que
/// cubren el plan, sus elementos y los informes) hace fallar la validación.
/// Cambiar el contrato exige subir `schema_version` y publicar el esquema.
#[test]
fn renaming_any_key_breaks_validation() {
    let mut checked = 0;
    for doc in sample_documents() {
        let mut paths = Vec::new();
        key_paths(&doc, &mut Vec::new(), 4, &mut paths);
        for path in paths {
            let (key, parent) = path.split_last().expect("ruta");
            let renamed = format!("{key}_renamed");

            let mut mutated = doc.clone();
            let obj = parent_mut(&mut mutated, parent);
            let value = obj.remove(key).expect("clave");
            obj.insert(renamed, value);
            assert!(
                validate(&mutated).is_err(),
                "renombrar {path:?} en `{}` sigue validando",
                doc["command"]
            );
            checked += 1;
        }
    }
    assert!(checked > 200, "solo se han probado {checked} claves");

    // Subir la versión sin esquema nuevo tampoco valida.
    let mut doc = sample_documents().swap_remove(0);
    doc["schema_version"] = json!(2);
    assert!(validate(&doc).is_err());
}
