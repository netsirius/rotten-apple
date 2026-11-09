//! Esquemas JSON del CLI (T-01: RNF-ARQ-22, RF-CLI-04, RF-CLI-21).
//!
//! - Cada salida de `cli.rs` se repite aquí, se valida contra
//!   `schemas/<comando>.v<N>.schema.json` (lo hace `Fx::run`) y se compara con
//!   su *snapshot* de `insta`, con rutas, fechas, IDs y hashes ocultos.
//! - Renombrar o quitar una clave sin subir `schema_version` hace fallar
//!   la validación (`renaming_any_key_breaks_validation`).
//!
//! Para regenerar los *snapshots* tras un cambio intencionado:
//! `INSTA_UPDATE=always cargo test -p rotten --test schemas` y revisar el
//! diff antes de hacer commit.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{Fx, fixture, redact, schema_files, validate};
use serde_json::{Map, Value, json};

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
    let mut commands: Vec<&str> = files.keys().map(|(c, _)| c.as_str()).collect();
    commands.dedup();
    assert_eq!(
        commands,
        [
            "clean", "config", "doctor", "exclude", "free", "history", "scan", "trash", "undo"
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
fn free() {
    let fx = fixture();
    check(
        &fx,
        "free_error_unknown_op",
        &["free", "no-such-op", "--yes", "--json"],
        1,
    );
    let (_, doc) = fx.run(&["clean", "--yes", "--json"]);
    let op = doc["report"]["op_id"].as_str().expect("op").to_owned();
    check(
        &fx,
        "free_dry_run",
        &["free", &op, "--dry-run", "--json"],
        0,
    );
    check(&fx, "free_error_no_yes", &["free", &op, "--json"], 2);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let unreadable = fx.run(&["free", &op, "--yes", "--json"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(unreadable.0, 3, "{:#}", unreadable.1);
    insta::assert_json_snapshot!("free_error_trash_unreadable", redact(&fx, &unreadable.1));
    // El primer elemento cambia dentro de la Papelera: se omite (4).
    let first = doc["report"]["items"][0]["result_path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    let first = std::path::Path::new(&first);
    if first.is_dir() {
        fs::write(first.join("nuevo.txt"), b"x").expect("write");
    } else {
        fs::write(first, b"cambiado").expect("write");
    }
    check(
        &fx,
        "free_executed_partial",
        &["free", &op, "--yes", "--json"],
        4,
    );
    check(&fx, "history_freed", &["history", "--json"], 0);
    let (code, undo) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(
        (code, &undo["error"]["code"]),
        (1, &json!("nothing_to_undo"))
    );
}

#[test]
fn free_everything() {
    let fx = fixture();
    let (_, doc) = fx.run(&["clean", "--yes", "--json"]);
    let op = doc["report"]["op_id"].as_str().expect("op").to_owned();
    let freed = check(&fx, "free_executed", &["free", &op, "--yes", "--json"], 0);
    let (code, undo) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(
        (code, &undo["error"]["code"]),
        (1, &json!("nothing_to_undo"))
    );
    // Una liberación no se libera. El mensaje lleva el id de la operación,
    // que cambia en cada ejecución.
    let free_op = freed["report"]["op_id"].as_str().expect("op");
    let (code, doc) = fx.run(&["free", free_op, "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    let text = serde_json::to_string(&redact(&fx, &doc)).expect("json");
    let doc: Value = serde_json::from_str(&text.replace(free_op, "[op_id]")).expect("json");
    insta::assert_json_snapshot!("free_error_not_freeable", doc);
}

/// `rotten trash` y `rotten trash empty` (T-12).
#[test]
fn trash_list_and_empty() {
    let fx = fixture();
    check(&fx, "trash_list_nothing", &["trash", "--json"], 0);
    check(
        &fx,
        "trash_empty_nothing",
        &["trash", "empty", "--yes", "--json"],
        0,
    );
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");
    check(&fx, "trash_list", &["trash", "--json"], 0);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let list = fx.run(&["trash", "--json"]);
    let empty = fx.run(&["trash", "empty", "--yes", "--json"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    assert_eq!(list.0, 0, "{:#}", list.1);
    insta::assert_json_snapshot!("trash_list_unreadable", redact(&fx, &list.1));
    assert_eq!(empty.0, 3, "{:#}", empty.1);
    insta::assert_json_snapshot!("trash_empty_error_unreadable", redact(&fx, &empty.1));
    check(
        &fx,
        "trash_empty_dry_run",
        &["trash", "empty", "--dry-run", "--json"],
        0,
    );
    check(
        &fx,
        "trash_empty_error_no_yes",
        &["trash", "empty", "--json"],
        2,
    );
    check(
        &fx,
        "trash_empty_executed",
        &["trash", "empty", "--yes", "--json"],
        0,
    );
    check(&fx, "history_trash_emptied", &["history", "--json"], 0);
}

#[test]
fn doctor() {
    let fx = fixture();
    check(&fx, "doctor", &["doctor", "--json"], 0);
    write_corrupt_config(&fx);
    check(&fx, "doctor_config_corrupt", &["doctor", "--json"], 5);
}

/// `rotten config get|set|path` (T-13) y el enlace en `~/.config/rotten/`.
#[test]
fn config() {
    let fx = fixture();
    check(&fx, "config_path_no_link", &["config", "path", "--json"], 0);
    check(
        &fx,
        "config_get_default",
        &["config", "get", "language", "--json"],
        0,
    );
    check(
        &fx,
        "config_get_list",
        &["config", "get", "exclusions.paths", "--json"],
        0,
    );
    check(
        &fx,
        "config_set_dry_run",
        &["config", "set", "language", "es", "--dry-run", "--json"],
        0,
    );
    check(
        &fx,
        "config_set_creates_link",
        &["config", "set", "language", "es", "--json"],
        0,
    );
    check(
        &fx,
        "config_set_unchanged",
        &["config", "set", "language", "es", "--json"],
        0,
    );
    check(
        &fx,
        "config_set_integer",
        &["config", "set", "journal.retention_days", "90", "--json"],
        0,
    );
    check(&fx, "config_path_linked", &["config", "path", "--json"], 0);
    check(
        &fx,
        "config_get_integer",
        &["config", "get", "journal.retention_days", "--json"],
        0,
    );
    check(
        &fx,
        "config_error_unknown_key",
        &["config", "get", "nope", "--json"],
        2,
    );
    check(
        &fx,
        "config_error_invalid_value",
        &["config", "set", "journal.retention_days", "0", "--json"],
        2,
    );
    check(
        &fx,
        "config_error_read_only",
        &["config", "set", "exclusions.paths", "~/x", "--json"],
        2,
    );

    // Otra cosa en la ruta del enlace.
    let link = fx.p(".config/rotten/config.toml");
    fs::remove_file(&link).expect("rm");
    fs::write(&link, "language = \"en\"\n").expect("write");
    check(
        &fx,
        "config_path_conflict",
        &["config", "path", "--json"],
        0,
    );
    check(
        &fx,
        "config_error_link_conflict",
        &["config", "set", "language", "en", "--json"],
        5,
    );
    check(&fx, "doctor_config_link_conflict", &["doctor", "--json"], 5);
    check(
        &fx,
        "clean_error_config_link_conflict",
        &["clean", "--dry-run", "--json"],
        5,
    );
    fs::remove_file(&link).expect("rm");

    write_corrupt_config(&fx);
    check(
        &fx,
        "config_error_config_corrupt",
        &["config", "set", "language", "en", "--json"],
        5,
    );
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
    let op = docs.last().expect("trash put")["report"]["op_id"]
        .as_str()
        .expect("op")
        .to_owned();
    docs.push(fx.run(&["free", &op, "--dry-run", "--json"]).1);
    docs.push(fx.run(&["free", &op, "--yes", "--json"]).1);
    docs.push(fx.run(&["free", "no-such-op", "--json"]).1);
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");
    docs.push(fx.run(&["trash", "--json"]).1);
    docs.push(fx.run(&["trash", "empty", "--dry-run", "--json"]).1);
    docs.push(fx.run(&["trash", "empty", "--yes", "--json"]).1);
    docs.push(fx.run(&["history", "--json"]).1);
    docs.push(fx.run(&["config", "get", "language", "--json"]).1);
    docs.push(fx.run(&["config", "set", "language", "es", "--json"]).1);
    docs.push(fx.run(&["config", "path", "--json"]).1);
    docs.push(fx.run(&["config", "get", "nope", "--json"]).1);
    let link = fx.p(".config/rotten/config.toml");
    fs::remove_file(&link).expect("rm");
    std::os::unix::fs::symlink(fx.trash.join("otra.toml"), &link).expect("ln");
    docs.push(fx.run(&["config", "path", "--json"]).1);
    docs.push(fx.run(&["doctor", "--json"]).1);
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
    doc["schema_version"] = json!(3);
    assert!(validate(&doc).is_err());
}
