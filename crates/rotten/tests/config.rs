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

/// Todo lo que hay bajo `base` (sin seguir enlaces), con su contenido. Se
/// salta el log técnico (T-14), que crece en cada ejecución.
fn tree(base: &Path) -> Vec<(PathBuf, Option<Entry>)> {
    let log = base.join(common::LOG_DIR);
    let mut out = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.starts_with(&log) {
                continue;
            }
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
    let (code, doc) = fx.run(&["config", "set", "language", "en", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link_created"], false);
    assert_eq!(doc["link"]["state"], "linked");
    let (_, doc) = fx.run(&["config", "get", "language", "--json"]);
    assert_eq!(doc["value"], "en");
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
    // Algo en la Papelera, para que `free` y `trash empty` tengan qué borrar.
    let op = clean(&fx);
    let trash_before = tree(&fx.trash);
    let garbage = "[exclusions\npaths = [";
    fs::write(config(&fx), garbage).expect("write");
    // B2: tampoco se borra nada de la Papelera.
    for args in destructive_on_the_trash(&op) {
        let (code, doc) = fx.run(&args);
        assert_eq!(code, 5, "{args:?}: {doc:#}");
        assert_eq!(doc["error"]["code"], "config_corrupt", "{args:?}");
    }
    assert_eq!(
        tree(&fx.trash),
        trash_before,
        "se borró algo de la Papelera"
    );
    for args in [
        &["config", "set", "language", "es", "--json"][..],
        &["config", "set", "language", "system", "--json"],
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
        // I2: no se expone hasta que algo la use (T-105).
        (
            ["config", "set", "journal.retention_days", "30"],
            "unknown_config_key",
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
    // Algo en la Papelera, para que `free` y `trash empty` tengan qué borrar.
    let op = clean(&fx);
    let trash_before = tree(&fx.trash);
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

        let mut all: Vec<Vec<&str>> = vec![
            vec!["clean", "--yes", "--json"],
            vec!["clean", "--dry-run", "--json"],
            vec!["clean", "--permanent", "--yes", "--json"],
            vec!["trash", "put", &cache, "--yes", "--json"],
            vec!["exclude", "add", "~/Library/Logs", "--json"],
        ];
        // B2: `free` y `trash empty` también.
        all.extend(destructive_on_the_trash(&op));
        for args in all {
            let (code, doc) = fx.run(&args);
            assert_eq!(code, 5, "{kind} {args:?}: {doc:#}");
            assert_eq!(doc["error"]["code"], "config_link_conflict");
        }
        // Ni la carpeta personal (con el journal) ni la Papelera cambian.
        assert_eq!(tree(&fx.home), home_before, "{kind}: algo cambió");
        assert_eq!(tree(&fx.trash), trash_before, "{kind}: la Papelera cambió");
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

/// `clean --yes` con la configuración en orden: la operación. Deja algo en
/// la Papelera para `free` y `trash empty`.
fn clean(fx: &Fx) -> String {
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    doc["report"]["op_id"].as_str().expect("op").to_owned()
}

/// Lo que borra de la Papelera (B2): revisado y ejecutado.
fn destructive_on_the_trash(op: &str) -> Vec<Vec<&str>> {
    vec![
        vec!["free", op, "--dry-run", "--json"],
        vec!["free", op, "--yes", "--json"],
        vec!["trash", "empty", "--dry-run", "--json"],
        vec!["trash", "empty", "--yes", "--json"],
    ]
}

/// Salida para personas (stdout y stderr juntos) y código de salida.
fn human(fx: &Fx, lang: &str, args: &[&str]) -> (i32, String) {
    let out = fx
        .cmd_lang(lang)
        .env("ROTTEN_HUMAN", "1")
        .env("NO_COLOR", "1")
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

/// B2: `undo` no depende de la configuración: con otra cosa en la ruta del
/// enlace sigue devolviendo las cosas a su sitio.
#[test]
fn undo_still_restores_with_something_else_at_the_link_path() {
    let fx = fixture();
    let op = clean(&fx);
    let restored = fx.p("Library/Logs/old-app/app.log");
    assert!(!restored.exists());
    fs::create_dir_all(link(&fx).parent().expect("padre")).expect("mkdir");
    fs::write(link(&fx), "language = \"en\"\n").expect("write");
    let (code, doc) = fx.run(&["undo", &op, "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert!(restored.exists());
    // Y lo que había en la ruta del enlace sigue ahí.
    assert_eq!(
        fs::read_to_string(link(&fx)).expect("read"),
        "language = \"en\"\n"
    );
}

/// B1: la configuración canónica y `~/.config/rotten/config.toml` son
/// enlaces al mismo fichero de dotfiles. `config set` escribe en ese
/// fichero y deja los dos enlaces como estaban.
#[test]
fn set_writes_through_a_canonical_config_linked_to_a_dotfile() {
    let fx = fixture();
    let dotfile = fx.p("dotfiles/rotten.toml");
    fs::create_dir_all(dotfile.parent().expect("padre")).expect("mkdir");
    fs::write(
        &dotfile,
        "schema_version = 1\n\n[exclusions]\npaths = [\"~/Library/Logs/old-app\"]\n",
    )
    .expect("write");
    symlink(&dotfile, config(&fx)).expect("ln");
    fs::create_dir_all(link(&fx).parent().expect("padre")).expect("mkdir");
    symlink(&dotfile, link(&fx)).expect("ln");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"]["state"], "linked");

    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["changed"], true);
    assert_eq!(doc["link"]["state"], "linked");

    assert_eq!(
        fs::read_link(config(&fx)).expect("sigue siendo enlace"),
        dotfile
    );
    assert_eq!(
        fs::read_link(link(&fx)).expect("sigue siendo enlace"),
        dotfile
    );
    let text = fs::read_to_string(&dotfile).expect("read");
    assert!(text.contains("language = \"es\""), "{text}");
    assert!(text.contains("~/Library/Logs/old-app"), "{text}");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"]["state"], "linked");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    // Lo que se edita en el fichero de dotfiles es lo que se aplica.
    fs::write(&dotfile, "schema_version = 1\n\n[exclusions]\n").expect("write");
    let (_, doc) = fx.run(&["exclude", "list", "--json"]);
    assert_eq!(doc["exclusions"], json!([]));
}

/// M10: si `~/.config` es un enlace (dotfiles), el enlace de Rotten se crea
/// dentro de su destino y `~/.config` sigue siendo el mismo enlace.
#[test]
fn set_creates_the_link_inside_a_symlinked_config_folder() {
    let fx = fixture();
    let dotconfig = fx.p("dotfiles/config");
    fs::create_dir_all(&dotconfig).expect("mkdir");
    symlink(&dotconfig, fx.p(".config")).expect("ln");
    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link_created"], true);
    assert_eq!(doc["link"]["state"], "linked");
    assert_eq!(fs::read_link(fx.p(".config")).expect("enlace"), dotconfig);
    assert_eq!(
        fs::read_link(dotconfig.join("rotten/config.toml")).expect("enlace"),
        config(&fx)
    );
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"]["state"], "linked");
}

/// I1: con `ROTTEN_STATE_DIR` la configuración es otra, así que el enlace
/// de `~/.config` no aplica: ni se crea ni se comprueba, y `doctor` y
/// `config path` dicen por qué.
#[test]
fn with_rotten_state_dir_the_link_does_not_apply() {
    let mut fx = fixture();
    let other = fx.trash.parent().expect("raíz").join("other-state");
    fs::create_dir_all(&other).expect("mkdir");
    let other_s = other.to_str().expect("UTF-8").to_owned();
    let na = json!({"state": "not_applicable", "reason": "state_dir_override"});

    // Sin enlace previo, `config set` no crea nada en `~/.config`.
    fx.env.push(("ROTTEN_STATE_DIR".into(), other_s.clone()));
    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link"], na);
    assert_eq!(doc["link_created"], false);
    assert!(describe(&fx.p(".config")).is_none(), "creó ~/.config");
    assert!(other.join("config.toml").exists());

    // Un enlace a la configuración por defecto, hecho sin la variable…
    fx.env.clear();
    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!((code, &doc["link_created"]), (0, &json!(true)), "{doc:#}");
    let link_before = describe(&link(&fx));

    // …no bloquea nada ni se toca con la variable puesta.
    fx.env.push(("ROTTEN_STATE_DIR".into(), other_s));
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"], na);
    let (code, doc) = fx.run(&["config", "path", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link"], na);
    // En español: es el idioma de esa configuración (I2).
    let (_, text) = human(&fx, "en_US.UTF-8", &["config", "path"]);
    assert!(text.contains("no aplica con ROTTEN_STATE_DIR"), "{text}");
    let (code, doc) = fx.run(&["config", "set", "language", "en", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["link"], na);
    assert_eq!(describe(&link(&fx)), link_before);
    // Y lo que hay en esa ruta tampoco bloquea.
    clear(&link(&fx));
    fs::write(link(&fx), "language = \"en\"\n").expect("write");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
}

/// I2: `language` se aplica. Sin `--lang`, el idioma de la configuración
/// manda sobre `LANG`; con `--lang`, manda `--lang`.
#[test]
fn the_configured_language_applies_unless_lang_is_given() {
    let fx = fixture();
    let es = "es_ES.UTF-8";
    let (_, text) = human(&fx, es, &["config", "path"]);
    assert!(text.contains("Configuración:"), "{text}");

    let (code, doc) = fx.run(&["config", "set", "language", "en", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let (_, text) = human(&fx, es, &["config", "path"]);
    assert!(text.contains("Configuration:"), "{text}");
    assert!(!text.contains("Configuración"), "{text}");
    let (_, text) = human(&fx, "en_US.UTF-8", &["config", "path", "--lang", "es"]);
    assert!(text.contains("Configuración:"), "{text}");

    let (code, doc) = fx.run(&["config", "set", "language", "system", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let (_, text) = human(&fx, es, &["config", "path"]);
    assert!(text.contains("Configuración:"), "{text}");
}

/// I3: las claves desconocidas fuera de `[exclusions]` se toleran (una
/// versión más nueva pudo escribirlas), se avisan y se conservan. Una
/// sección que parece un `[exclusions]` mal escrito no se tolera.
#[test]
fn unknown_keys_outside_exclusions_are_tolerated_and_warned() {
    let fx = fixture();
    fs::write(
        config(&fx),
        "schema_version = 1\ntheme = \"dark\"\n\n[journal]\nretention_days = 30\n\n[exclusions]\n",
    )
    .expect("write");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(
        doc["report"]["config_unknown_keys"],
        json!(["journal", "theme"])
    );
    let (_, text) = human(&fx, "en_US.UTF-8", &["doctor"]);
    assert!(text.contains("journal, theme"), "{text}");
    let (code, doc) = fx.run(&["scan", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let warnings = doc["warnings"].to_string();
    assert!(warnings.contains("journal, theme"), "{doc:#}");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");

    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let text = fs::read_to_string(config(&fx)).expect("read");
    assert!(text.contains("theme = \"dark\""), "{text}");
    assert!(text.contains("retention_days = 30"), "{text}");

    // Fail-closed: una exclusión en una sección mal escrita no se ignora.
    fs::write(config(&fx), "[exclusion]\npaths = [\"~/Library/Logs\"]\n").expect("write");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!((code, &doc["error"]["code"]), (5, &json!("config_corrupt")));
}

/// M1: con la configuración corrupta, el mensaje del JSON es una línea; el
/// fragmento del TOML va aparte, en `detail`. Para personas no cambia.
#[test]
fn a_corrupt_config_puts_the_toml_fragment_in_detail() {
    let fx = fixture();
    fs::write(config(&fx), "[exclusions]\npaths = [1, \"FRAGMENT-xyz\"]\n").expect("write");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 5, "{doc:#}");
    assert_eq!(doc["error"]["code"], "config_corrupt");
    let message = doc["error"]["message"].as_str().expect("message");
    assert!(!message.contains('\n'), "{message:?}");
    assert!(!message.contains("FRAGMENT-xyz"), "{message:?}");
    let detail = doc["error"]["detail"].as_str().expect("detail");
    assert!(detail.contains("FRAGMENT-xyz"), "{detail:?}");
    let (code, text) = human(&fx, "en_US.UTF-8", &["clean", "--dry-run"]);
    assert_eq!(code, 5);
    assert!(text.contains("FRAGMENT-xyz"), "{text}");
}

/// M5: si `~/.config/rotten` es un fichero, el enlace no se puede crear:
/// es un caso propio, con su mensaje, y `config set` sale con `4` (la
/// configuración sí se guarda). No bloquea nada.
#[test]
fn a_file_where_the_link_folder_goes_is_its_own_case() {
    let fx = fixture();
    let blocker = fx.p(".config/rotten");
    fs::create_dir_all(blocker.parent().expect("padre")).expect("mkdir");
    fs::write(&blocker, "x\n").expect("write");
    let parent = json!(blocker.to_str().expect("UTF-8"));

    let (code, doc) = fx.run(&["config", "set", "language", "es", "--json"]);
    assert_eq!(code, 4, "{doc:#}");
    assert_eq!(doc["changed"], true);
    assert_eq!(doc["link"]["state"], "parent_not_directory");
    assert_eq!(doc["link"]["parent"], parent);
    let err = doc["link_error"].as_str().expect("link_error");
    assert!(err.contains("is not a folder"), "{err}");
    let text = fs::read_to_string(config(&fx)).expect("read");
    assert!(text.contains("language = \"es\""), "{text}");

    let (_, doc) = fx.run(&["config", "path", "--json"]);
    assert_eq!(doc["link"]["state"], "parent_not_directory");
    let (code, doc) = fx.run(&["doctor", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["config_link"]["parent"], parent);
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    // En español: es el idioma guardado (I2).
    let (code, text) = human(&fx, "en_US.UTF-8", &["config", "set", "language", "en"]);
    assert_eq!(code, 4, "{text}");
    assert!(text.contains("es un fichero, no una carpeta"), "{text}");
    assert_eq!(fs::read_to_string(&blocker).expect("read"), "x\n");
}

/// M13: `scan` ya avisa de que con otra cosa en la ruta del enlace no se
/// podrá limpiar, en el JSON (`warnings`) y para personas.
#[test]
fn scan_warns_about_something_else_at_the_link_path() {
    let fx = fixture();
    fs::create_dir_all(link(&fx).parent().expect("padre")).expect("mkdir");
    fs::write(link(&fx), "language = \"en\"\n").expect("write");
    let (code, doc) = fx.run(&["scan", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    let warnings = doc["warnings"].to_string();
    assert!(
        warnings.contains("not the link to the configuration"),
        "{doc:#}"
    );
    let (_, text) = human(&fx, "en_US.UTF-8", &["scan"]);
    assert!(
        text.contains("nothing is cleaned until you move it away"),
        "{text}"
    );
    let (_, text) = human(&fx, "es_ES.UTF-8", &["scan"]);
    assert!(!text.contains("nothing is cleaned"), "{text}");
}
