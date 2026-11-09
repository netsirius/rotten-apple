//! `rotten config` y el enlace `~/.config/rotten/config.toml` (T-13, 07 §2
//! y §3, RF-SEG-05 c).
//!
//! - Con la configuración corrupta, `config set` se niega (`5`) y no cambia
//!   ni un byte.
//! - El enlace solo lo crea `config set` (sin `--dry-run`), solo si en esa
//!   ruta no hay nada, y nunca sustituye lo que haya. Ninguna lectura lo crea.
//! - Si en esa ruta hay otra cosa, `doctor` lo avisa (`5`) y las acciones
//!   destructivas abortan con `5` sin mover nada.

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use common::{Fx, fixture};
use serde_json::json;

fn link(fx: &Fx) -> PathBuf {
    fx.p(".config/rotten/config.toml")
}

fn config(fx: &Fx) -> PathBuf {
    fx.state.join("config.toml")
}

/// Qué hay en una ruta, sin seguir enlaces: tipo, destino y contenido.
fn describe(p: &Path) -> Option<(String, Option<PathBuf>, Option<Vec<u8>>)> {
    let m = fs::symlink_metadata(p).ok()?;
    let ft = m.file_type();
    Some((
        format!("{ft:?}"),
        ft.is_symlink().then(|| fs::read_link(p).expect("readlink")),
        ft.is_file().then(|| fs::read(p).expect("read")),
    ))
}

/// Tipo, destino si es un enlace y contenido si es un fichero.
type Entry = (String, Option<PathBuf>, Option<Vec<u8>>);

/// Todo lo que hay bajo `base` (sin seguir enlaces), con su contenido.
fn tree(base: &Path) -> Vec<(PathBuf, Option<Entry>)> {
    let mut out = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(p.clone());
            }
            out.push((p.clone(), describe(&p)));
        }
    }
    out.sort();
    out
}

#[test]
fn reads_never_create_the_link() {
    let fx = fixture();
    for args in [
        &["config", "path", "--json"][..],
        &["config", "get", "language", "--json"],
        &["config", "get", "exclusions.paths", "--json"],
        &["config", "set", "language", "es", "--dry-run", "--json"],
        &["doctor", "--json"],
        &["scan", "--json"],
        &["exclude", "list", "--json"],
        &["clean", "--dry-run", "--json"],
    ] {
        let (code, doc) = fx.run(args);
        assert!(code == 0 || code == 4, "{args:?}: {doc:#}");
        assert!(
            describe(&fx.p(".config")).is_none(),
            "{args:?} ha creado ~/.config"
        );
    }
    let (_, doc) = fx.run(&["config", "path", "--json"]);
    assert_eq!(doc["link"]["state"], "missing");
}

#[test]
fn set_creates_the_link_to_the_canonical_config() {
    let fx = fixture();
    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link_created"], true);
    assert_eq!(doc["link"]["state"], "linked");
    assert_eq!(fs::read_link(link(&fx)).expect("enlace"), config(&fx));
    // Leer por el enlace es leer la configuración.
    assert_eq!(
        fs::read_to_string(link(&fx)).expect("read"),
        fs::read_to_string(config(&fx)).expect("read")
    );
    // Otro `set`: el enlace sigue, no se vuelve a crear.
    let (code, doc) = fx.run(&["config", "set", "journal.retention_days", "30", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link_created"], false);
    assert_eq!(doc["link"]["state"], "linked");
    let (_, doc) = fx.run(&["config", "get", "journal.retention_days", "--json"]);
    assert_eq!(doc["value"], 30);
}

/// Nombre de la prueba y cómo poner esa cosa en la ruta del enlace.
type Foreign = (&'static str, Box<dyn Fn()>);

/// Lo que puede haber ya en la ruta del enlace y que nunca se sustituye.
fn foreign_things(fx: &Fx) -> Vec<Foreign> {
    let l = link(fx);
    let other = fx.trash.join("otra.toml");
    let (l1, l2, l3, l4) = (l.clone(), l.clone(), l.clone(), l.clone());
    vec![
        (
            "file",
            Box::new(move || fs::write(&l1, "language = \"en\"\n").expect("write")),
        ),
        (
            "directory",
            Box::new(move || fs::create_dir(&l2).expect("mkdir")),
        ),
        (
            "symlink",
            Box::new(move || symlink(&other, &l3).expect("ln")),
        ),
        (
            "symlink",
            Box::new(move || symlink("config.toml.bak", &l4).expect("ln")),
        ),
    ]
}

fn clear(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p).expect("rm -r"),
        Ok(_) => fs::remove_file(p).expect("rm"),
        Err(_) => {}
    }
}

#[test]
fn set_never_replaces_what_is_already_there() {
    let fx = fixture();
    fs::create_dir_all(link(&fx).parent().expect("padre")).expect("mkdir");
    for (kind, make) in foreign_things(&fx) {
        clear(&link(&fx));
        make();
        let before = (describe(&link(&fx)), describe(&config(&fx)));
        let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
        assert_eq!(code, 5, "{kind}: {doc:#}");
        assert_eq!(doc["error"]["code"], "config_link_conflict", "{kind}");
        assert_eq!(
            (describe(&link(&fx)), describe(&config(&fx))),
            before,
            "{kind}: no se toca ni el enlace ni la configuración"
        );
        let (_, doc) = fx.run(&["config", "path", "--json"]);
        assert_eq!(doc["link"]["state"], "conflict", "{kind}");
        assert_eq!(doc["link"]["found"]["kind"], kind, "{kind}");
    }
}

/// Criterio de aceptación: con la configuración corrupta, `config set` se
/// niega y devuelve `5`, sin cambiar nada ni crear el enlace.
#[test]
fn set_refuses_a_corrupt_config_with_5() {
    let fx = fixture();
    let garbage = "[exclusions\npaths = [";
    fs::write(config(&fx), garbage).expect("write");
    for args in [
        &["config", "set", "language", "es", "--json"][..],
        &["config", "set", "journal.retention_days", "30", "--json"],
        &["config", "set", "language", "es", "--dry-run", "--json"],
    ] {
        let (code, doc) = fx.run(args);
        assert_eq!(code, 5, "{args:?}: {doc:#}");
        assert_eq!(doc["error"]["code"], "config_corrupt");
        assert_eq!(fs::read_to_string(config(&fx)).expect("read"), garbage);
        assert!(describe(&link(&fx)).is_none(), "no se crea el enlace");
    }
    let (code, doc) = fx.run(&["config", "get", "language", "--json"]);
    assert_eq!((code, &doc["error"]["code"]), (5, &json!("config_corrupt")));
}

/// Una clave desconocida o un valor de otro tipo: `2` y nada escrito.
#[test]
fn set_validates_keys_and_types_before_writing() {
    let fx = fixture();
    for (args, code_str) in [
        (["config", "set", "nope", "1"], "unknown_config_key"),
        (["config", "set", "language", "fr"], "invalid_config_value"),
        (
            ["config", "set", "journal.retention_days", "treinta"],
            "invalid_config_value",
        ),
        (
            ["config", "set", "journal.retention_days", "0"],
            "invalid_config_value",
        ),
        (
            ["config", "set", "exclusions.paths", "~/Library"],
            "read_only_config_key",
        ),
    ] {
        let mut a = args.to_vec();
        a.push("--json");
        let (code, doc) = fx.run(&a);
        assert_eq!(
            (code, &doc["error"]["code"]),
            (2, &json!(code_str)),
            "{a:?}"
        );
        assert!(describe(&config(&fx)).is_none(), "{a:?}: escribió algo");
        assert!(describe(&link(&fx)).is_none(), "{a:?}: creó el enlace");
    }
}

/// Criterio de aceptación: si en la ruta del enlace hay otra cosa, `doctor`
/// lo avisa y las acciones destructivas abortan con `5` (RF-SEG-05 c).
#[test]
fn something_else_at_the_link_path_blocks_destructive_actions() {
    let fx = fixture();
    fs::create_dir_all(link(&fx).parent().expect("padre")).expect("mkdir");
    let cache = fx.p("Library/Caches/com.example.editor");
    let cache = cache.to_str().expect("UTF-8").to_owned();
    for (kind, make) in foreign_things(&fx) {
        clear(&link(&fx));
        make();
        let (code, doc) = fx.run(&["doctor", "--json"]);
        assert_eq!(code, 5, "{kind}: {doc:#}");
        assert_eq!(doc["report"]["config_link"]["state"], "conflict");

        // Después de `doctor`, que crea el cerrojo en la carpeta de estado.
        let home_before = tree(&fx.home);

        for args in [
            &["clean", "--yes", "--json"][..],
            &["clean", "--dry-run", "--json"],
            &["clean", "--permanent", "--yes", "--json"],
            &["trash", "put", &cache, "--yes", "--json"],
            &["exclude", "add", "~/Library/Logs", "--json"],
        ] {
            let (code, doc) = fx.run(args);
            assert_eq!(code, 5, "{kind} {args:?}: {doc:#}");
            assert_eq!(doc["error"]["code"], "config_link_conflict");
        }
        assert_eq!(tree(&fx.home), home_before, "{kind}: algo cambió");
        assert!(tree(&fx.trash).is_empty(), "{kind}: algo fue a la Papelera");
        assert!(!fx.state.join("journal").exists(), "{kind}: hay journal");
    }
    // Sin la otra cosa, se vuelve a poder limpiar.
    clear(&link(&fx));
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    // Y el enlace correcto (aunque la configuración aún no exista) no
    // bloquea nada.
    symlink(config(&fx), link(&fx)).expect("ln");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"]["state"], "linked");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
}

/// La configuración escrita por `config set` la leen igual el resto de
/// comandos: las exclusiones siguen aplicándose.
#[test]
fn set_keeps_the_exclusions() {
    let fx = fixture();
    fx.run(&["exclude", "add", "~/Library/Logs/old-app", "--json"]);
    // `exclude add` escribe la configuración, pero el enlace solo lo crea
    // `config set`.
    assert!(describe(&link(&fx)).is_none());
    let (code, _) = fx.run(&["config", "set", "language", "en", "--json"]);
    assert_eq!(code, 0);
    let (_, doc) = fx.run(&["exclude", "list", "--json"]);
    assert_eq!(doc["exclusions"], json!(["~/Library/Logs/old-app"]));
    let (_, doc) = fx.run(&["config", "get", "exclusions.paths", "--json"]);
    assert_eq!(doc["value"], json!(["~/Library/Logs/old-app"]));
    let text = fs::read_to_string(config(&fx)).expect("read");
    assert!(text.contains("language = \"en\""), "{text}");
}
