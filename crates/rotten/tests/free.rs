//! `rotten free <op-id…>` («Liberar ahora», T-11, CU-08).
//!
//! Cada prueba usa una carpeta personal temporal (`ROTTEN_HOME`) y una
//! Papelera temporal (`ROTTEN_TRASH_DIR`): nunca se toca el `~` real ni su
//! Papelera. Cada salida JSON se valida contra su esquema (`Fx::run`).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use common::{Fx, fixture};
use serde_json::{Value, json};

/// `(nombre, inode)` de lo que hay en la Papelera de prueba.
fn trash_listing(fx: &Fx) -> BTreeMap<String, u64> {
    fs::read_dir(&fx.trash)
        .expect("Papelera")
        .flatten()
        .map(|e| {
            let m = fs::symlink_metadata(e.path()).expect("lstat");
            (e.file_name().to_string_lossy().into_owned(), m.ino())
        })
        .collect()
}

/// `clean --yes`: la operación y dónde quedó cada elemento en la Papelera.
fn clean(fx: &Fx) -> (String, Vec<PathBuf>) {
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let op = doc["report"]["op_id"].as_str().expect("op").to_owned();
    let trashed: Vec<PathBuf> = doc["report"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| PathBuf::from(i["result_path"].as_str().expect("result_path")))
        .collect();
    assert!(trashed.len() >= 2, "{doc:#}");
    (op, trashed)
}

fn trash_put(fx: &Fx, path: &Path) -> String {
    let (code, doc) = fx.run(&["trash", "put", path.to_str().expect("UTF-8"), "--yes"]);
    assert_eq!(code, 0, "{doc:#}");
    doc["report"]["op_id"].as_str().expect("op").to_owned()
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

/// Aceptación: `clean --yes` y `free <op> --yes` dejan la Papelera de
/// prueba vacía de esos elementos (y solo de esos); `history` muestra
/// «irreversible» y `undo` devuelve `1`.
#[test]
fn clean_then_free_empties_those_items_only() {
    let fx = fixture();
    let (op, trashed) = clean(&fx);
    // Algo que no mandó Rotten Apple.
    fs::write(fx.trash.join("ajeno.txt"), b"de otra app").expect("write");
    let before = trash_listing(&fx);

    let (code, doc) = fx.run(&["free", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], trashed.len());
    for p in &trashed {
        assert!(
            fs::symlink_metadata(p).is_err(),
            "{p:?} sigue en la Papelera"
        );
    }
    let after = trash_listing(&fx);
    assert_eq!(
        after,
        BTreeMap::from([("ajeno.txt".to_owned(), before["ajeno.txt"])])
    );
    // Lo estimado frente a lo medido (RF-SEG-12 e).
    assert_eq!(doc["space"]["estimated_bytes"], doc["report"]["bytes_done"]);
    assert!(
        doc["space"]["volumes"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    );

    let original = op_in_history(&fx, &op);
    assert_eq!(original["undo_state"], "irreversible", "{original:#}");
    let free_op = doc["report"]["op_id"].as_str().expect("op");
    let freed = op_in_history(&fx, free_op);
    assert_eq!(freed["kind"], "free");
    assert_eq!(freed["mode"], "permanent");
    assert_eq!(freed["items_done"], trashed.len());

    let (code, doc) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    assert_eq!(doc["error"]["code"], "nothing_to_undo");
    // I5: deshacer esa operación, nombrándola, tampoco: es irreversible (1).
    for args in [
        ["undo", &op, "--yes", "--json"],
        ["undo", &op, "--dry-run", "--json"],
    ] {
        let (code, doc) = fx.run(&args);
        assert_eq!(code, 1, "{args:?}: {doc:#}");
        assert_eq!(doc["error"]["code"], "not_undoable", "{args:?}");
    }
    // Ya no queda nada que liberar: se dice, no se borra nada más y no es
    // un resultado parcial (0).
    let (code, doc) = fx.run(&["free", &op, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let (code, doc) = fx.run(&["free", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert!(doc["space"]["measured_bytes"].is_null(), "M6: {doc:#}");
    assert!(doc["plan"]["ready"].as_array().expect("ready").is_empty());
    assert!(
        doc["plan"]["not_freeable"]
            .as_array()
            .expect("not_freeable")
            .iter()
            .all(|n| n["reason"] == "deleted")
    );
    assert_eq!(trash_listing(&fx), after);
}

/// Aceptación: un elemento modificado o sustituido dentro de la Papelera se
/// omite y el resultado es `4`.
#[test]
fn modified_or_replaced_inside_the_trash_is_skipped() {
    let fx = fixture();
    let (op, trashed) = clean(&fx);
    let (modified, replaced) = (&trashed[0], &trashed[1]);
    // Modificado: mismo inode, otro contenido (en una carpeta, una entrada
    // nueva cambia su `mtime`).
    let marker = if modified.is_dir() {
        let m = modified.join("nuevo.txt");
        fs::write(&m, b"modificado dentro de la Papelera").expect("write");
        m
    } else {
        fs::write(modified, b"modificado dentro de la Papelera").expect("write");
        modified.clone()
    };
    // Sustituido: otro objeto con el mismo nombre.
    if replaced.is_dir() {
        fs::remove_dir_all(replaced).expect("rm");
    } else {
        fs::remove_file(replaced).expect("rm");
    }
    fs::write(replaced, b"otro fichero").expect("write");
    let files = [modified, replaced];

    let (code, doc) = fx.run(&["free", &op, "--dry-run", "--json"]);
    assert_eq!(code, 4, "la simulación prevé las omisiones: {doc:#}");
    assert_eq!(doc["report"]["skipped"], 2);

    let (code, doc) = fx.run(&["free", &op, "--yes", "--json"]);
    assert_eq!(code, 4, "{doc:#}");
    assert_eq!(doc["report"]["skipped"], 2);
    assert_eq!(doc["report"]["done"], trashed.len() - 2);
    let skipped: Vec<&Value> = doc["report"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|i| i["outcome"] == "skipped")
        .collect();
    assert!(skipped.iter().all(|i| i["reason"] == "identity_changed"));
    assert_eq!(
        fs::read(&marker).expect("sigue"),
        b"modificado dentro de la Papelera"
    );
    assert_eq!(fs::read(replaced).expect("sigue"), b"otro fichero");
    for p in trashed.iter().filter(|p| !files.contains(p)) {
        assert!(fs::symlink_metadata(p).is_err(), "{p:?}");
    }
}

/// Aceptación: `--dry-run --json` muestra el plan sin tocar nada.
#[test]
fn dry_run_shows_the_plan_and_touches_nothing() {
    let fx = fixture();
    let (op, trashed) = clean(&fx);
    let before = trash_listing(&fx);
    let journal_before = fs::read_dir(fx.state.join("journal"))
        .expect("journal")
        .flatten()
        .map(|e| fs::read(e.path()).expect("leer"))
        .collect::<Vec<_>>();

    let (code, doc) = fx.run(&["free", &op, "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["dry_run"], true);
    assert_eq!(doc["plan"]["ops"], json!([op]));
    let ready = doc["plan"]["ready"].as_array().expect("ready");
    assert_eq!(ready.len(), trashed.len());
    for (it, p) in ready.iter().zip(&trashed) {
        assert_eq!(it["path"], p.to_str().expect("UTF-8"));
        assert_eq!(it["of"]["op"], op.as_str());
    }
    assert_eq!(doc["report"]["would"], trashed.len());
    assert!(
        doc["report"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|i| i["action"] == "would_delete")
    );
    assert_eq!(trash_listing(&fx), before);
    let journal_after = fs::read_dir(fx.state.join("journal"))
        .expect("journal")
        .flatten()
        .map(|e| fs::read(e.path()).expect("leer"))
        .collect::<Vec<_>>();
    assert_eq!(journal_before, journal_after, "la simulación no escribe");
    assert_eq!(op_in_history(&fx, &op)["undo_state"], "restorable");
}

/// Sin terminal hace falta `--yes` (07 §1.4); no se borra nada.
#[test]
fn needs_yes_without_a_terminal() {
    let fx = fixture();
    let (op, _) = clean(&fx);
    let before = trash_listing(&fx);
    let (code, doc) = fx.run(&["free", &op, "--json"]);
    assert_eq!(code, 2, "{doc:#}");
    assert_eq!(doc["error"]["code"], "needs_yes");
    assert_eq!(trash_listing(&fx), before);
}

/// Varias operaciones a la vez (sin repetir) en una sola liberación.
#[test]
fn several_operations_at_once() {
    let fx = fixture();
    let a = fx.p("Library/Caches/com.example.editor");
    let b = fx.p("Library/Logs/install.log");
    let op1 = trash_put(&fx, &a);
    let op2 = trash_put(&fx, &b);
    assert_eq!(trash_listing(&fx).len(), 2);
    let (code, doc) = fx.run(&["free", &op1, &op2, &op1, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["plan"]["ops"], json!([op1, op2]));
    assert_eq!(doc["report"]["done"], 2);
    assert!(trash_listing(&fx).is_empty());
    for op in [&op1, &op2] {
        assert_eq!(op_in_history(&fx, op)["undo_state"], "irreversible");
    }
}

/// Una operación que no existe es `1`; un deshacer no se libera (no mandó
/// nada a la Papelera).
#[test]
fn unknown_or_not_freeable_operations() {
    let fx = fixture();
    let (code, doc) = fx.run(&["free", "no-such-op", "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    assert_eq!(doc["error"]["code"], "unknown_operation");
    clean(&fx);
    let (code, doc) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let undo_op = doc["report"]["op_id"].as_str().expect("op");
    let (code, doc) = fx.run(&["free", undo_op, "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    assert_eq!(doc["error"]["code"], "not_freeable");
}

/// Restaurar algo deja la operación «restaurada en parte» al liberar el
/// resto, y lo restaurado no se toca.
#[test]
fn partly_restored_then_freed() {
    let fx = fixture();
    let (op, trashed) = clean(&fx);
    let (code, doc) = fx.run(&["undo", &op, "--items", "1", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let restored = PathBuf::from(
        doc["report"]["items"][0]["result_path"]
            .as_str()
            .expect("ruta"),
    );
    // Lo restaurado no queda pendiente: todo lo demás se libera (0).
    let (code, doc) = fx.run(&["free", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], trashed.len() - 1);
    assert_eq!(doc["plan"]["not_freeable"][0]["reason"], "already_restored");
    assert!(restored.exists());
    assert_eq!(op_in_history(&fx, &op)["undo_state"], "partly_restored");
    // Restaurada en parte y el resto borrado: nada que deshacer y algo que
    // ya no vuelve (4).
    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 4, "{doc:#}");
}

/// Códigos de `undo <op>`: restaurada entera, 0 (no queda nada pendiente);
/// `clean --permanent`, irreversible: 1 con `not_undoable`.
#[test]
fn undo_exit_codes_of_restored_and_permanent_operations() {
    let fx = fixture();
    let (op, _) = clean(&fx);
    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(op_in_history(&fx, &op)["undo_state"], "restored");
    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert!(doc["report"]["done"] == 0, "{doc:#}");

    let (code, doc) = fx.run(&["clean", "--permanent", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let permanent = doc["report"]["op_id"].as_str().expect("op").to_owned();
    let (code, doc) = fx.run(&["undo", &permanent, "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    assert_eq!(doc["error"]["code"], "not_undoable");
}

/// Sin permiso para leer la Papelera (sin Acceso total al disco) no se
/// adivina: se dice, se sale con `3` y no se borra nada.
#[test]
fn unreadable_trash_exits_3_and_deletes_nothing() {
    let fx = fixture();
    let (op, trashed) = clean(&fx);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    let dry = fx.run(&["free", &op, "--dry-run", "--json"]);
    let real = fx.run(&["free", &op, "--yes", "--json"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    for (code, doc) in [dry, real] {
        assert_eq!(code, 3, "{doc:#}");
        assert_eq!(doc["error"]["code"], "trash_unreadable");
        assert!(
            doc["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("Full Disk Access")),
            "{doc:#}"
        );
    }
    for p in &trashed {
        assert!(fs::symlink_metadata(p).is_ok(), "{p:?} se borró");
    }
}

/// Escribe una operación `forged` que dice haber mandado `path` «a la
/// Papelera» `trash` (descriptor de `op_started`), con su identidad real.
fn forge(fx: &Fx, trash: &str, path: &Path) {
    let m = fs::symlink_metadata(path).expect("lstat");
    let identity = json!({
        "dev": m.dev(), "ino": m.ino(), "kind": "file", "size": m.size(),
        "mtime": m.mtime(), "mtime_nsec": m.mtime_nsec(),
        "ctime": m.ctime(), "ctime_nsec": m.ctime_nsec(),
    });
    let ts = "2026-11-01T10:00:00.000Z";
    let lines = [
        json!({"schema_version":1,"ts":ts,"op":"forged","ev":"op_started","kind":"clean","mode":"trash","items":1,"bytes":1,"by":"cli","trash":trash}),
        json!({"schema_version":1,"ts":ts,"op":"forged","ev":"item_intent","item":1,"action":"trash","path":path,"identity":identity,"bytes":1}),
        json!({"schema_version":1,"ts":ts,"op":"forged","ev":"item_done","item":1,"result_path":path}),
        json!({"schema_version":1,"ts":ts,"op":"forged","ev":"op_finished","status":"completed","done":1,"skipped":0,"failed":0}),
    ];
    let dir = fx.state.join("journal");
    fs::create_dir_all(&dir).expect("mkdir");
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(dir.join("2026-11.jsonl"), text).expect("write");
}

/// Una línea del journal que dice que un documento está «en la Papelera»
/// no basta (B1 de la revisión):
///
/// - con el descriptor de la Papelera en uso, solo se borra lo que está en
///   su primer nivel (`not_in_trash`);
/// - con un descriptor que apunta a otra carpeta (`dir:~/Documents`, donde
///   el documento sí está «en el primer nivel») o a la Papelera del
///   sistema cuando se usa `ROTTEN_TRASH_DIR`, la operación no se puede
///   liberar (`other_trash`).
///
/// En los tres casos sale con 4 y el documento sigue ahí.
#[test]
fn a_journal_path_outside_the_trash_is_never_deleted() {
    for (desc, reason) in [
        ("trash", "not_in_trash"),
        ("documents", "other_trash"),
        ("system", "other_trash"),
    ] {
        let fx = fixture();
        let doc_path = fx.p("Documents/tesis.docx");
        let trash = match desc {
            "trash" => format!("dir:{}", fx.trash.display()),
            "documents" => format!("dir:{}", fx.p("Documents").display()),
            _ => "system".to_owned(),
        };
        forge(&fx, &trash, &doc_path);
        let (code, dry) = fx.run(&["free", "forged", "--dry-run", "--json"]);
        assert_eq!(code, 4, "{desc}: {dry:#}");
        let (code, doc) = fx.run(&["free", "forged", "--yes", "--json"]);
        assert_eq!(code, 4, "{desc}: {doc:#}");
        assert!(
            doc["plan"]["ready"].as_array().expect("ready").is_empty(),
            "{desc}: {doc:#}"
        );
        assert_eq!(
            doc["plan"]["not_freeable"][0]["reason"], reason,
            "{desc}: {doc:#}"
        );
        assert!(
            doc_path.exists(),
            "{desc}: nunca se borra lo que no está en la Papelera en uso"
        );
    }
}

/// Máximo común divisor.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// A qué paso redondea `statfs` el espacio libre, a juzgar por `v`: la
/// mayor potencia 2^a·5^b que lo divide. Con bloques de 4 KiB sale del
/// orden de KiB; con un espacio libre redondeado a cientos de MB, de
/// cientos de MB.
fn roundness(v: u64) -> u64 {
    gcd(v, (1u64 << 30) * 5u64.pow(12))
}

/// M-5: lo liberado medido con `statfs` no se queda por debajo de lo
/// estimado más allá de máx(5 %, 200 MB), que es lo que el producto
/// comprueba. Con un fichero de 32 MiB.
///
/// Si el volumen resuelve el tamaño (el espacio libre no viene redondeado
/// a pasos mayores que lo borrado), además: lo medido es al menos la mitad
/// de lo estimado y no lo supera en más de la tolerancia. Hay entornos que
/// redondean `statfs` a cientos de MB; ahí esas dos no pueden distinguir
/// nada y se omiten (se dice en la salida de la prueba).
#[test]
fn measured_space_matches_the_estimate_within_m5() {
    let fx = fixture();
    let big = fx.p("Downloads/grande.bin");
    fs::create_dir_all(big.parent().expect("padre")).expect("mkdir");
    // Bloques reales, no un fichero disperso.
    fs::write(&big, vec![0x5au8; 32 << 20]).expect("write");
    let op = trash_put(&fx, &big);
    let (code, doc) = fx.run(&["free", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let estimated = doc["space"]["estimated_bytes"].as_i64().expect("estimado");
    assert!(estimated >= 32 << 20, "{doc:#}");
    let measured = doc["space"]["measured_bytes"]
        .as_i64()
        .expect("medido con statfs");
    let tolerance = (estimated / 20).max(200_000_000);
    assert!(
        measured >= estimated - tolerance,
        "estimado {estimated}, medido {measured}: {doc:#}"
    );
    let volumes = doc["space"]["volumes"].as_array().expect("volúmenes");
    let step = volumes.iter().fold(0, |g, v| {
        let before = v["free_before"].as_u64().expect("antes");
        let after = v["free_after"].as_u64().expect("después");
        g.max(roundness(before).min(roundness(after)))
    });
    if step > (estimated as u64) / 4 {
        eprintln!("statfs redondea a pasos de {step} B: M-5 solo se comprueba por abajo");
        return;
    }
    assert!(
        measured >= estimated / 2,
        "estimado {estimated}, medido {measured}: {doc:#}"
    );
    assert!(
        measured <= estimated + tolerance,
        "estimado {estimated}, medido {measured}: {doc:#}"
    );
}

/// Tras `clean` y `trash put`, el informe sugiere `rotten free <op-id>`.
#[test]
fn clean_report_suggests_free() {
    let fx = fixture();
    let out = fx
        .cmd()
        .env("ROTTEN_HUMAN", "1")
        .args(["clean", "--yes"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    let (_, doc) = fx.run(&["history", "--json"]);
    let op = doc["operations"][0]["op_id"].as_str().expect("op");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("rotten free {op}")), "{text}");

    let out = fx
        .cmd()
        .env("ROTTEN_HUMAN", "1")
        .args(["trash", "put", "--yes"])
        .arg(fx.p("Library/Caches/com.example.editor"))
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    let (_, doc) = fx.run(&["history", "--json"]);
    let op = doc["operations"][0]["op_id"].as_str().expect("op");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("rotten free {op}")), "{text}");
}
