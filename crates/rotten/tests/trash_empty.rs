//! `rotten trash` y `rotten trash empty` (T-12, CU-08, RF-PAN-07).
//!
//! Cada prueba usa una carpeta personal temporal (`ROTTEN_HOME`) y una
//! Papelera temporal (`ROTTEN_TRASH_DIR`): nunca se toca el `~` real ni su
//! Papelera. Cada salida JSON se valida contra su esquema (`Fx::run`). La
//! prueba con la Papelera real está en `ra-core/tests/empty.rs`, ignorada y
//! de solo lectura.
//!
//! Que lo que llega a la Papelera después de revisar no se borra se prueba
//! en el núcleo (`ra-core/tests/empty.rs`): aquí cada ejecución con `--yes`
//! prepara su propio plan.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{Fx, fixture};
use serde_json::{Value, json};

fn names(fx: &Fx) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(&fx.trash)
        .expect("Papelera")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// `clean --yes` y `trash put` de un documento: las dos operaciones.
fn two_ops(fx: &Fx) -> (String, u64, String) {
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let clean = doc["report"]["op_id"].as_str().expect("op").to_owned();
    let done = doc["report"]["done"].as_u64().expect("done");
    let f = fx.p("Documents/borrador.txt");
    fs::write(&f, b"borrador").expect("write");
    let (code, doc) = fx.run(&[
        "trash",
        "put",
        f.to_str().expect("UTF-8"),
        "--yes",
        "--json",
    ]);
    assert_eq!(code, 0, "{doc:#}");
    let put = doc["report"]["op_id"].as_str().expect("op").to_owned();
    (clean, done, put)
}

fn human(fx: &Fx, lang: &str, args: &[&str]) -> (i32, String) {
    let out = fx
        .cmd_lang(lang)
        .env("ROTTEN_HUMAN", "1")
        .args(args)
        .output()
        .expect("ejecutar");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().expect("código"), text)
}

fn op_in_history(fx: &Fx, op: &str) -> Value {
    let (_, doc) = fx.run(&["history", "--json"]);
    doc["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .find(|o| o["op_id"] == op)
        .unwrap_or_else(|| panic!("{op} no está en el historial: {doc:#}"))
        .clone()
}

/// `rotten trash` separa, por volumen, lo de cada operación de lo demás.
#[test]
fn trash_lists_each_operation_apart_from_the_rest() {
    let fx = fixture();
    let (clean, done, put) = two_ops(&fx);
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");

    let (code, doc) = fx.run(&["trash", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["action"], "list");
    assert_eq!(doc["readable"], true);
    let vols = doc["volumes"].as_array().expect("volumes");
    assert_eq!(vols.len(), 1, "solo ROTTEN_TRASH_DIR: {doc:#}");
    let v = &vols[0];
    assert_eq!(v["path"], fx.trash.to_str().expect("UTF-8"));
    let ops: Vec<(Value, Value)> = v["by_operation"]
        .as_array()
        .expect("by_operation")
        .iter()
        .map(|o| (o["op_id"].clone(), o["items"].clone()))
        .collect();
    assert_eq!(ops, [(json!(clean), json!(done)), (json!(put), json!(1))]);
    assert_eq!(v["others"]["items"], 1);
    assert_eq!(v["total"]["items"], done + 2);

    let (code, text) = human(&fx, "es_ES.UTF-8", &["trash"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains(&format!("rotten free {clean}")), "{text}");
    assert!(text.contains("Otros: 1 elemento"), "{text}");
    assert!(text.contains("rotten trash empty"), "{text}");
}

/// Sin Acceso total al disco no se adivina: se dice, y solo se muestra lo
/// que anotó el journal.
#[test]
fn trash_without_access_shows_only_the_journal() {
    let fx = fixture();
    let (clean, done, _) = two_ops(&fx);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let json = fx.run(&["trash", "--json"]);
    let text = human(&fx, "es_ES.UTF-8", &["trash"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");

    let (code, doc) = json;
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["readable"], false);
    let v = &doc["volumes"][0];
    assert_eq!(
        (&v["readable"], &v["total"], &v["others"]),
        (&json!(false), &Value::Null, &Value::Null)
    );
    assert_eq!(v["by_operation"][0]["op_id"], clean);
    assert_eq!(v["by_operation"][0]["items"], done);
    let (code, text) = text;
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Acceso total al disco"), "{text}");
    assert!(text.contains("historial"), "{text}");
}

/// `--dry-run` enseña el plan y no toca nada; sin `--yes` y sin terminal,
/// tampoco (2).
#[test]
fn empty_dry_run_and_without_yes_touch_nothing() {
    let fx = fixture();
    two_ops(&fx);
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");
    let before = names(&fx);

    let (code, doc) = fx.run(&["trash", "empty", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["dry_run"], true);
    let items = doc["plan"]["items"].as_array().expect("items");
    assert_eq!(items.len(), before.len());
    assert_eq!(doc["report"]["would"], before.len());
    // Lo que mandó Rotten Apple lleva su operación; lo ajeno, no.
    let ajeno = items
        .iter()
        .find(|i| {
            i["path"]
                .as_str()
                .is_some_and(|p| p.ends_with("de-otra-app.txt"))
        })
        .expect("ajeno");
    assert!(ajeno.get("of").is_none(), "{ajeno:#}");
    assert_eq!(
        items.iter().filter(|i| i.get("of").is_some()).count(),
        before.len() - 1
    );

    let (code, doc) = fx.run(&["trash", "empty", "--json"]);
    assert_eq!(code, 2, "{doc:#}");
    assert_eq!(names(&fx), before);
}

/// Vaciar borra todo lo revisado; las operaciones afectadas pasan a
/// irreversibles y el vaciado queda en el historial.
#[test]
fn empty_deletes_what_was_reviewed_and_ops_become_irreversible() {
    let fx = fixture();
    let (clean, done, put) = two_ops(&fx);
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");

    let (code, doc) = fx.run(&["trash", "empty", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], done + 2);
    assert_eq!(
        doc["space"]["estimated_bytes"],
        doc["plan"]["totals"]["bytes"]
    );
    assert!(names(&fx).is_empty(), "{:?}", names(&fx));
    let empty_op = doc["report"]["op_id"].as_str().expect("op");
    assert_eq!(op_in_history(&fx, empty_op)["kind"], "trash_empty");
    for op in [&clean, &put] {
        assert_eq!(op_in_history(&fx, op)["undo_state"], "irreversible");
    }
    // Ya no hay nada que deshacer ni que liberar.
    let (code, doc) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(
        (code, &doc["error"]["code"]),
        (1, &json!("nothing_to_undo"))
    );
    let (code, doc) = fx.run(&["free", &clean, "--yes", "--json"]);
    assert_eq!(code, 0, "nada pendiente: {doc:#}");
    assert!(
        doc["plan"]["not_freeable"]
            .as_array()
            .expect("not_freeable")
            .iter()
            .all(|n| n["reason"] == "deleted"),
        "{doc:#}"
    );
    // Vacía: nada que hacer.
    let (code, doc) = fx.run(&["trash", "empty", "--yes", "--json"]);
    assert_eq!((code, &doc["report"]["done"]), (0, &json!(0)));
    let (code, text) = human(&fx, "es_ES.UTF-8", &["trash", "empty", "--yes"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("La Papelera ya está vacía"), "{text}");
}

/// Sin poder leer la Papelera, `trash empty` termina con 3, no borra nada y
/// sugiere `rotten free`.
#[test]
fn empty_unreadable_exits_3_and_suggests_free() {
    let fx = fixture();
    let (clean, _, _) = two_ops(&fx);
    let before = names(&fx);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let dry = fx.run(&["trash", "empty", "--dry-run", "--json"]);
    let real = fx.run(&["trash", "empty", "--yes", "--json"]);
    let text = human(&fx, "es_ES.UTF-8", &["trash", "empty", "--yes"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    for (code, doc) in [dry, real] {
        assert_eq!(code, 3, "{doc:#}");
        assert_eq!(doc["error"]["code"], "trash_unreadable");
        let msg = doc["error"]["message"].as_str().expect("message");
        assert!(msg.contains("rotten free"), "{msg}");
        assert!(msg.contains("Full Disk Access"), "{msg}");
    }
    let (code, text) = text;
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("rotten free"), "{text}");
    assert!(text.contains("Acceso total al disco"), "{text}");
    assert_eq!(names(&fx), before);
    // Con `free` sí se puede liberar lo de Rotten Apple, que no necesita
    // listar la Papelera (aquí, legible de nuevo).
    let (code, doc) = fx.run(&["free", &clean, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
}

/// I5 (PoC D del revisor): enlaces simbólicos en la Papelera, uno en el
/// primer nivel que apunta a Documentos y otros dentro de una carpeta (a
/// Documentos y a un fichero suyo). Se borran los enlaces, nunca lo que
/// hay al otro lado.
#[test]
fn symlinks_in_the_trash_are_deleted_not_followed() {
    let fx = fixture();
    let docs = fx.p("Documents");
    fs::create_dir_all(&docs).expect("mkdir");
    let secret = docs.join("secreto.txt");
    fs::write(&secret, b"secreto").expect("write");
    let before: Vec<_> = {
        let mut v: Vec<_> = fs::read_dir(&docs)
            .expect("Documentos")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        v.sort();
        v
    };
    std::os::unix::fs::symlink(&docs, fx.trash.join("link-a-documentos")).expect("ln");
    let folder = fx.trash.join("carpeta");
    fs::create_dir(&folder).expect("mkdir");
    std::os::unix::fs::symlink(&docs, folder.join("link2")).expect("ln");
    std::os::unix::fs::symlink(&secret, folder.join("link-fichero")).expect("ln");

    let (code, doc) = fx.run(&["trash", "empty", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 2, "{doc:#}");
    assert!(names(&fx).is_empty(), "{:?}", names(&fx));
    assert_eq!(fs::read(&secret).expect("secreto"), b"secreto");
    let after: Vec<_> = {
        let mut v: Vec<_> = fs::read_dir(&docs)
            .expect("Documentos")
            .flatten()
            .map(|e| e.file_name())
            .collect();
        v.sort();
        v
    };
    assert_eq!(before, after, "Documentos cambió");
}
