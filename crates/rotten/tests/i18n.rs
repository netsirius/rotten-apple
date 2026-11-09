//! Idioma del CLI (T-03: RF-CLI-09, RNF-24, RNF-25).
//!
//! - La salida `--json` es idéntica con `LANG=es_ES.UTF-8` y con
//!   `LANG=en_US.UTF-8`, en todos los comandos y también en los errores.

mod common;

use std::fs;
use std::path::Path;

use common::{Fx, fixture, redact};
use serde_json::Value;

const ES: &str = "es_ES.UTF-8";
const EN: &str = "en_US.UTF-8";

/// Borra lo que haya en `p` (fichero o carpeta).
fn remove(p: &Path) {
    if p.is_dir() {
        fs::remove_dir_all(p).expect("rm -r");
    } else {
        fs::remove_file(p).expect("rm");
    }
}

/// Sustituye los identificadores de operación (`<11 hex>-<6 hex>`) dentro
/// de los textos: cambian en cada carpeta personal de prueba.
fn mask_op_ids(v: &Value) -> Value {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            let hex = |c: u8| c.is_ascii_hexdigit();
            let mut out = String::new();
            let mut i = 0;
            while i < b.len() {
                if i + 18 <= b.len()
                    && b[i..i + 11].iter().all(|c| hex(*c))
                    && b[i + 11] == b'-'
                    && b[i + 12..i + 18].iter().all(|c| hex(*c))
                {
                    out.push_str("[op]");
                    i += 18;
                } else {
                    let ch = s[i..].chars().next().expect("carácter");
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(mask_op_ids).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), mask_op_ids(x)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Todos los comandos, con éxito y con cada error que se puede provocar,
/// en una carpeta personal nueva. Devuelve (argumentos, código, JSON sin lo
/// volátil) de cada paso.
fn scenario(lang: &str) -> Vec<(String, i32, Value)> {
    let fx = fixture();
    let mut out = Vec::new();
    let mut step = |fx: &Fx, args: &[&str]| -> Value {
        let (code, doc) = fx.run_lang(lang, args);
        let shown = args
            .iter()
            .map(|a| a.replace(fx.home.to_str().expect("UTF-8"), "~"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push((shown, code, mask_op_ids(&redact(fx, &doc))));
        doc
    };
    let cache = fx.p("Library/Caches/com.example.editor");
    let cache = cache.to_str().expect("UTF-8");
    let keychain = fx.p("Library/Keychains/login.keychain-db");
    let keychain = keychain.to_str().expect("UTF-8");

    step(&fx, &["scan", "--json"]);
    step(&fx, &["scan", "--category", "CAT-03", "--json"]);
    step(&fx, &["doctor", "--json"]);
    step(&fx, &["history", "--json"]);
    step(&fx, &["exclude", "list", "--json"]);
    step(
        &fx,
        &["exclude", "add", "~/Library/Logs/x", "--dry-run", "--json"],
    );
    step(&fx, &["exclude", "add", "~/Library/Logs/x", "--json"]);
    step(&fx, &["exclude", "add", "~/Library/Logs/x", "--json"]);
    step(&fx, &["exclude", "list", "--json"]);
    step(&fx, &["exclude", "remove", "~/Library/Logs/x", "--json"]);
    step(&fx, &["exclude", "remove", "~/Library/Logs/x", "--json"]);
    step(&fx, &["exclude", "add", "relativa/ruta", "--json"]);
    step(&fx, &["clean", "--json"]);
    step(&fx, &["clean", "--permanent", "--json"]);
    step(&fx, &["clean", "--dry-run", "--category", "CAT-99", "--json"]);
    step(&fx, &["clean", "--dry-run", "--json"]);
    step(
        &fx,
        &["clean", "--dry-run", "--category", "CAT-01", "--json"],
    );
    step(&fx, &["undo", "no-such-op", "--json"]);
    step(&fx, &["undo", "--yes", "--json"]);
    step(&fx, &["trash", "put", cache, "--dry-run", "--json"]);
    step(&fx, &["trash", "put", cache, keychain, "--yes", "--json"]);
    step(&fx, &["trash", "put", cache, "--json"]);
    step(&fx, &["trash", "put", "/", "--yes", "--json"]);
    step(&fx, &["trash", "put", "/nonexistent/x", "--yes", "--json"]);

    // Con el cerrojo de ejecución tomado por otro proceso.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fx.state.join("exec.lock"))
        .expect("lock");
    lock.try_lock().expect("tomar el cerrojo");
    step(&fx, &["clean", "--yes", "--json"]);
    step(&fx, &["doctor", "--json"]);
    drop(lock);

    let clean = step(&fx, &["clean", "--yes", "--json"]);
    step(&fx, &["clean", "--yes", "--json"]);
    step(&fx, &["history", "--json"]);
    step(&fx, &["undo", "--dry-run", "--json"]);
    step(&fx, &["undo", "--items", "2", "--yes", "--json"]);
    step(&fx, &["undo", "--items", "9", "--yes", "--json"]);
    // «Vaciar la Papelera» para el elemento 1: queda no recuperable.
    let gone = clean["report"]["items"][0]["result_path"]
        .as_str()
        .expect("ruta")
        .to_owned();
    remove(Path::new(&gone));
    step(&fx, &["undo", "--dry-run", "--json"]);
    step(&fx, &["undo", "--yes", "--json"]);
    step(&fx, &["history", "--json"]);
    step(&fx, &["undo", "--yes", "--json"]);
    step(&fx, &["trash", "put", cache, "--yes", "--json"]);
    step(&fx, &["history", "--json"]);

    // Configuración corrupta.
    fs::write(fx.state.join("config.toml"), "[exclusions\npaths = [").expect("write");
    step(&fx, &["scan", "--json"]);
    step(&fx, &["doctor", "--json"]);
    step(&fx, &["clean", "--dry-run", "--json"]);
    step(&fx, &["exclude", "list", "--json"]);
    step(&fx, &["exclude", "add", "~/Library/Logs", "--json"]);
    step(&fx, &["trash", "put", cache, "--dry-run", "--json"]);
    out
}

/// RF-CLI-09 y RNF-25: el JSON no depende del idioma, en ningún comando.
#[test]
fn json_is_identical_in_spanish_and_english() {
    let es = scenario(ES);
    let en = scenario(EN);
    assert_eq!(es.len(), en.len());
    let mut commands = std::collections::BTreeSet::new();
    for ((args, code_es, doc_es), (_, code_en, doc_en)) in es.iter().zip(&en) {
        assert_eq!(code_es, code_en, "{args}: código distinto según el idioma");
        assert_eq!(
            doc_es, doc_en,
            "{args}: el JSON cambia con el idioma\nes: {doc_es:#}\nen: {doc_en:#}"
        );
        commands.insert(doc_es["command"].as_str().expect("command").to_owned());
    }
    // Todos los comandos con salida JSON han pasado por aquí.
    assert_eq!(
        commands.into_iter().collect::<Vec<_>>(),
        ["clean", "doctor", "exclude", "history", "scan", "trash", "undo"]
    );
}
