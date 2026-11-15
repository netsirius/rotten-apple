//! `rotten` sin subcomando (T-16, 07 §2).
//!
//! El menú solo existe con terminal (stdin y stdout), y aquí no la hay:
//! sus teclas y su dibujo se prueban en `src/cmd/menu.rs` con el backend
//! de pruebas de `ratatui`. Lo que sí se prueba con el binario:
//!
//! - Sin terminal, la ayuda por stderr (stdout queda vacío) y `2`, en el
//!   idioma de la persona, también con `--yes` y con `ROTTEN_HUMAN=1`.
//! - Con `--json`, el error `subcommand_required` (`2`), igual en los dos
//!   idiomas; valida contra `schemas/menu.v2.schema.json` (`Fx::run`).
//! - Ninguno de los dos crea el estado de la app ni toca la carpeta
//!   personal.

mod common;

use std::fs;

use common::fixture;

#[test]
fn without_a_terminal_it_prints_the_help_and_exits_2() {
    let fx = fixture();
    fs::remove_dir_all(&fx.state).expect("rm -r");
    for (lang, usage, menu) in [
        ("es_ES.UTF-8", "Uso:", "abre el menú interactivo"),
        ("en_US.UTF-8", "Usage:", "opens the interactive menu"),
    ] {
        for args in [&[][..], &["--yes"], &["--quiet", "--no-color"]] {
            for human in [false, true] {
                let mut c = fx.cmd_lang(lang);
                if human {
                    c.env("ROTTEN_HUMAN", "1");
                }
                let out = c.args(args).output().expect("ejecutar");
                let stderr = String::from_utf8_lossy(&out.stderr);
                assert_eq!(out.status.code(), Some(2), "{lang} {args:?}: {stderr}");
                assert!(out.stdout.is_empty(), "{lang} {args:?}: stdout no vacío");
                assert!(stderr.contains(usage), "{lang} {args:?}: {stderr}");
                assert!(stderr.contains(menu), "{lang} {args:?}: {stderr}");
                assert!(stderr.contains("scan"), "{lang} {args:?}: {stderr}");
                assert!(!stderr.contains('\x1b'), "{lang} {args:?}: colores");
            }
        }
    }
    assert!(
        !fx.state.exists(),
        "sin subcomando se creó el estado de la app"
    );
}

#[test]
fn with_json_it_is_a_usage_error_in_any_language() {
    let fx = fixture();
    fs::remove_dir_all(&fx.state).expect("rm -r");
    let (code, es) = fx.run_lang("es_ES.UTF-8", &["--json"]);
    assert_eq!(code, 2, "{es:#}");
    assert_eq!(es["command"], "menu");
    assert_eq!(es["error"]["code"], "subcommand_required");
    assert_eq!(es["error"]["exit_code"], 2);
    let (code, en) = fx.run_lang("en_US.UTF-8", &["--json", "--lang", "es"]);
    assert_eq!(code, 2);
    assert_eq!(es, en, "el JSON no depende del idioma");
    assert!(!fx.state.exists(), "--json sin subcomando creó el estado");
}
