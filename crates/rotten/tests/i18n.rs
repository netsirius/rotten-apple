//! Idioma del CLI (T-03: RF-CLI-09, RNF-24, RNF-25).
//!
//! - La salida `--json` es idéntica con `LANG=es_ES.UTF-8` y con
//!   `LANG=en_US.UTF-8`, en todos los comandos y también en los errores.
//! - La salida para personas sale en el idioma de `LANG`.
//! - Pseudo-localización: con `ROTTEN_PSEUDO_LOCALE=1`, toda línea que
//!   imprime el CLI viene entera del catálogo de mensajes (sin textos
//!   sueltos ni concatenados).
//! - La ayuda de cada comando existe en los dos idiomas.
//! - Los scripts de `rotten completion` (T-14) se generan en los dos
//!   idiomas y con la pseudo-localización, con las descripciones en ese
//!   idioma y sintaxis válida.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use common::{Fx, fixture, redact, syntax_ok};
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
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, x)| (k.clone(), mask_op_ids(x))).collect())
        }
        other => other.clone(),
    }
}

/// La operación más reciente del historial (sin pasar por los pasos).
fn last_op(fx: &Fx) -> String {
    let (_, doc) = fx.run(&["history", "--json"]);
    doc["operations"][0]["op_id"]
        .as_str()
        .expect("op")
        .to_owned()
}

/// Todos los comandos, con éxito y con cada error que se puede provocar,
/// en una carpeta personal nueva. `run` ejecuta un paso (sin `--json`) y
/// devuelve su resultado.
fn scenario<T>(mut run: impl FnMut(&Fx, &[&str]) -> T) -> Vec<(String, T)> {
    let fx = fixture();
    let mut out = Vec::new();
    let mut step = |fx: &Fx, args: &[&str]| {
        let shown = args
            .iter()
            .map(|a| a.replace(fx.home.to_str().expect("UTF-8"), "~"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push((shown, run(fx, args)));
    };
    let cache = fx.p("Library/Caches/com.example.editor");
    let cache = cache.to_str().expect("UTF-8");
    let keychain = fx.p("Library/Keychains/login.keychain-db");
    let keychain = keychain.to_str().expect("UTF-8");

    step(&fx, &["scan"]);
    step(&fx, &["scan", "--category", "CAT-03"]);
    step(&fx, &["doctor"]);
    step(&fx, &["history"]);
    step(&fx, &["exclude", "list"]);
    step(&fx, &["exclude", "add", "~/Library/Logs/x", "--dry-run"]);
    step(&fx, &["exclude", "add", "~/Library/Logs/x"]);
    step(&fx, &["exclude", "add", "~/Library/Logs/x"]);
    step(&fx, &["exclude", "list"]);
    step(&fx, &["exclude", "remove", "~/Library/Logs/x"]);
    step(&fx, &["exclude", "remove", "~/Library/Logs/x"]);
    step(&fx, &["exclude", "add", "relativa/ruta"]);
    step(&fx, &["clean"]);
    step(&fx, &["clean", "--permanent"]);
    step(&fx, &["clean", "--dry-run", "--category", "CAT-99"]);
    step(&fx, &["clean", "--dry-run"]);
    step(&fx, &["clean", "--dry-run", "--category", "CAT-01"]);
    step(&fx, &["undo", "no-such-op"]);
    step(&fx, &["undo", "--yes"]);
    step(&fx, &["trash", "put", cache, "--dry-run"]);
    step(&fx, &["trash", "put", cache, keychain, "--yes"]);
    step(&fx, &["trash", "put", cache]);
    step(&fx, &["trash", "put", "/", "--yes"]);
    step(&fx, &["trash", "put", "/nonexistent/x", "--yes"]);

    // Con el cerrojo de ejecución tomado por otro proceso.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(fx.state.join("exec.lock"))
        .expect("lock");
    lock.try_lock().expect("tomar el cerrojo");
    step(&fx, &["clean", "--yes"]);
    step(&fx, &["doctor"]);
    drop(lock);

    step(&fx, &["clean", "--yes"]);
    step(&fx, &["clean", "--yes"]);
    step(&fx, &["history"]);
    step(&fx, &["undo", "--dry-run"]);
    step(&fx, &["undo", "--items", "2", "--yes"]);
    step(&fx, &["undo", "--items", "9", "--yes"]);
    // «Vaciar la Papelera» para un elemento: queda no recuperable.
    let mut in_trash: Vec<_> = fs::read_dir(&fx.trash)
        .expect("Papelera")
        .flatten()
        .map(|e| e.path())
        .collect();
    in_trash.sort();
    remove(in_trash.first().expect("algo en la Papelera"));
    step(&fx, &["undo", "--dry-run"]);
    step(&fx, &["undo", "--yes"]);
    step(&fx, &["history"]);
    step(&fx, &["undo", "--yes"]);
    step(&fx, &["trash", "put", cache, "--yes"]);
    step(&fx, &["history"]);

    // «Liberar ahora» (T-11) de esa última operación.
    let op = last_op(&fx);
    step(&fx, &["free", "no-such-op", "--yes"]);
    step(&fx, &["free", &op, "--dry-run"]);
    step(&fx, &["free", &op]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    step(&fx, &["free", &op, "--yes"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    step(&fx, &["free", &op, "--yes"]);
    step(&fx, &["free", &op, "--yes"]);
    step(&fx, &["history"]);

    // La Papelera entera (T-12): lo de Rotten Apple y algo ajeno.
    step(&fx, &["clean", "--yes"]);
    fs::write(fx.trash.join("de-otra-app.txt"), b"ajeno").expect("write");
    step(&fx, &["trash"]);
    step(&fx, &["trash", "empty", "--dry-run"]);
    step(&fx, &["trash", "empty"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o000)).expect("chmod");
    step(&fx, &["trash"]);
    step(&fx, &["trash", "empty", "--yes"]);
    fs::set_permissions(&fx.trash, fs::Permissions::from_mode(0o755)).expect("chmod");
    step(&fx, &["trash", "empty", "--yes"]);
    step(&fx, &["trash", "empty", "--yes"]);
    step(&fx, &["trash"]);
    step(&fx, &["history"]);

    // Configuración corrupta.
    fs::write(fx.state.join("config.toml"), "[exclusions\npaths = [").expect("write");
    step(&fx, &["scan"]);
    step(&fx, &["doctor"]);
    step(&fx, &["clean", "--dry-run"]);
    step(&fx, &["exclude", "list"]);
    step(&fx, &["exclude", "add", "~/Library/Logs"]);
    step(&fx, &["trash", "put", cache, "--dry-run"]);
    out
}

/// (código, JSON sin lo volátil) de un paso con `--json`.
fn json_step(lang: &'static str) -> impl FnMut(&Fx, &[&str]) -> (i32, Value) {
    move |fx, args| {
        let mut args = args.to_vec();
        args.push("--json");
        let (code, doc) = fx.run_lang(lang, &args);
        (code, mask_op_ids(&redact(fx, &doc)))
    }
}

/// Un paso con la salida para personas (`ROTTEN_HUMAN=1`).
fn human(fx: &Fx, lang: &str, pseudo: bool, args: &[&str]) -> Output {
    let mut c = fx.cmd_lang(lang);
    c.env("ROTTEN_HUMAN", "1").args(args);
    if pseudo {
        c.env("ROTTEN_PSEUDO_LOCALE", "1");
    }
    c.output().expect("ejecutar")
}

/// stdout y stderr juntos.
fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// RF-CLI-09 y RNF-25: el JSON no depende del idioma, en ningún comando.
#[test]
fn json_is_identical_in_spanish_and_english() {
    let es = scenario(json_step(ES));
    let en = scenario(json_step(EN));
    assert_eq!(es.len(), en.len());
    let mut commands = std::collections::BTreeSet::new();
    for ((args, (code_es, doc_es)), (_, (code_en, doc_en))) in es.iter().zip(&en) {
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
        [
            "clean", "doctor", "exclude", "free", "history", "scan", "trash", "undo"
        ]
    );
}

/// RF-CLI-09: `LANG=es_ES.UTF-8` imprime en español y `LANG=en_US.UTF-8`
/// en inglés, en todos los comandos y con los mismos códigos de salida.
#[test]
fn people_read_their_own_language() {
    let es = scenario(|fx, args| human(fx, ES, false, args));
    let en = scenario(|fx, args| human(fx, EN, false, args));
    for ((args, o_es), (_, o_en)) in es.iter().zip(&en) {
        assert_eq!(o_es.status.code(), o_en.status.code(), "{args}");
        assert_ne!(o_es.status.code(), Some(101), "{args}: {}", text(o_es));
        let (t_es, t_en) = (text(o_es), text(o_en));
        assert!(!t_es.trim().is_empty(), "{args}: sin salida");
        // La lista de exclusiones son solo datos (una por línea).
        if !(args == "exclude list" && o_es.status.success()) {
            assert_ne!(t_es, t_en, "{args}: la misma salida en los dos idiomas");
        }
    }
    let find = |v: &[(String, Output)], args: &str| {
        text(
            &v.iter()
                .find(|(a, _)| a == args)
                .unwrap_or_else(|| panic!("{args}"))
                .1,
        )
    };
    let scan = find(&es, "scan");
    assert!(
        scan.contains("Escaneo") && scan.contains("Preseleccionado"),
        "{scan}"
    );
    let scan = find(&en, "scan");
    assert!(
        scan.contains("Scan") && scan.contains("Preselected"),
        "{scan}"
    );
    // La revisión del plan explica cada regla (RF-LIM-04).
    let plan = find(&es, "clean --dry-run");
    assert!(
        plan.contains("Si se borra:") && plan.contains("Por qué este riesgo:"),
        "{plan}"
    );
    let plan = find(&en, "clean --dry-run");
    assert!(
        plan.contains("If deleted:") && plan.contains("Why this risk:"),
        "{plan}"
    );
    // Los errores del núcleo también se traducen.
    let err = find(&es, "undo no-such-op");
    assert!(err.contains("no existe"), "{err}");
    let err = find(&en, "undo no-such-op");
    assert!(err.contains("does not exist"), "{err}");
}

/// Quita lo que va entre U+2068 y U+2069 (los argumentos, que no son
/// texto del catálogo), con anidamiento.
fn strip_isolated(line: &str) -> String {
    let mut depth = 0;
    let mut out = String::new();
    for c in line.chars() {
        match c {
            '\u{2068}' => depth += 1,
            '\u{2069}' => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// RNF-24: pseudo-localización. Cada línea (sin la sangría) es un mensaje
/// entero del catálogo: empieza por `⟦`, acaba en `⟧` y no tiene nada fuera.
/// Un texto sin catálogo, o pegado a otro, rompe la prueba.
#[test]
fn pseudo_locale_finds_no_loose_or_concatenated_text() {
    let runs = scenario(|fx, args| human(fx, ES, true, args));
    let mut lines = 0;
    for (args, o) in &runs {
        assert_ne!(o.status.code(), Some(101), "{args}: {}", text(o));
        // Los argumentos pueden tener varias líneas (un error de TOML):
        // primero se quitan y luego se mira cada línea.
        for line in strip_isolated(&text(o)).lines() {
            let l = line.trim_start();
            if l.is_empty() {
                continue;
            }
            lines += 1;
            assert!(
                l.starts_with('⟦') && l.ends_with('⟧') && l.matches('⟦').count() == 1,
                "{args}: texto fuera del catálogo: {line:?}\n{}",
                text(o)
            );
        }
    }
    assert!(lines > 100, "{lines}");
}

/// La ayuda de cada comando, en los dos idiomas (un mensaje que falta
/// hace `panic!` en las compilaciones de depuración: código 101).
#[test]
fn help_in_both_languages() {
    let fx = fixture();
    let commands: &[&[&str]] = &[
        &[],
        &["scan"],
        &["clean"],
        &["history"],
        &["undo"],
        &["doctor"],
        &["exclude"],
        &["exclude", "add"],
        &["exclude", "list"],
        &["exclude", "remove"],
        &["free"],
        &["trash"],
        &["trash", "put"],
        &["trash", "empty"],
        &["completion"],
    ];
    for cmd in commands {
        for (lang, usage, options) in [(ES, "Uso:", "Opciones:"), (EN, "Usage:", "Options:")] {
            let out = fx
                .cmd_lang(lang)
                .args(*cmd)
                .arg("--help")
                .output()
                .expect("ejecutar");
            let help = text(&out);
            assert_eq!(out.status.code(), Some(0), "{cmd:?} {lang}: {help}");
            assert!(
                help.contains(usage) && help.contains(options),
                "{cmd:?} {lang}: {help}"
            );
        }
    }
    // `--lang` manda sobre `LANG`, también en la ayuda.
    let out = fx
        .cmd_lang(EN)
        .args(["--lang", "es", "scan", "--help"])
        .output()
        .expect("ejecutar");
    assert!(text(&out).contains("Escaneo inteligente"), "{}", text(&out));
}

/// `rotten completion` con `LANG` y con `--lang` (T-14): el script sale en
/// ese idioma, sin textos del otro (tampoco en los subcomandos de `help`)
/// y con sintaxis válida aunque las descripciones lleven `:`, `«»`,
/// paréntesis o los marcadores de la pseudo-localización.
#[test]
fn completion_scripts_in_both_languages() {
    let fx = fixture();
    let tmp = tempfile::tempdir().expect("tempdir");
    const IN_ES: &[&str] = &["Escaneo inteligente", "Papelera"];
    const IN_EN: &[&str] = &["Smart scan", "Trash"];
    /// LANG, flags, pseudo-localización, texto propio, textos del otro idioma.
    type Case = (
        &'static str,
        &'static [&'static str],
        bool,
        &'static str,
        &'static [&'static str],
    );
    let cases: &[Case] = &[
        (ES, &[], false, "Escaneo inteligente", IN_EN),
        (EN, &[], false, "Smart scan", IN_ES),
        (EN, &["--lang", "es"], false, "Escaneo inteligente", IN_EN),
        (ES, &["--lang", "en"], false, "Smart scan", IN_ES),
        (ES, &[], true, "Smårt scån", IN_ES),
    ];
    for (lang, flags, pseudo, own, others) in cases {
        for shell in ["zsh", "bash", "fish"] {
            let mut c = fx.cmd_lang(lang);
            c.args(*flags).args(["completion", shell]);
            if *pseudo {
                c.env("ROTTEN_PSEUDO_LOCALE", "1");
            }
            let out = c.output().expect("ejecutar");
            let what = format!("{shell} LANG={lang} {flags:?} pseudo={pseudo}");
            assert_eq!(out.status.code(), Some(0), "{what}: {}", text(&out));
            let script = String::from_utf8(out.stdout).expect("UTF-8");
            // bash no lleva descripciones.
            if shell != "bash" {
                assert!(script.contains(own), "{what}: falta «{own}»");
            }
            for other in *others {
                assert!(!script.contains(other), "{what}: «{other}» del otro idioma");
            }
            match syntax_ok(shell, &script, tmp.path()) {
                Some(Ok(())) | None => {}
                Some(Err(e)) => panic!("{what}: {shell} -n: {e}"),
            }
        }
    }
}
