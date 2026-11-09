//! `rotten completion <zsh|bash|fish>` (T-14, 07 §2).
//!
//! - Cada script se genera, con `0`, aunque la salida vaya a una tubería
//!   (sin JSON) y sin tocar la carpeta personal.
//! - Completa todos los subcomandos y todas las opciones del CLI. La lista
//!   sale de `--help` del propio binario, no de una lista a mano: un
//!   comando nuevo queda cubierto sin tocar esta prueba.
//! - La sintaxis es válida para cada shell (`zsh -n`, `bash -n`, `fish -n`)
//!   si está instalada; si no, se dice y se sigue.
//!
//! El idioma de las descripciones se prueba en `i18n.rs`.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{Fx, fixture, syntax_ok};

/// `rotten <cmd> --help`, en inglés.
fn help(fx: &Fx, cmd: &[&str]) -> String {
    let out = fx.cmd().args(cmd).arg("--help").output().expect("ejecutar");
    assert_eq!(out.status.code(), Some(0), "{cmd:?}");
    String::from_utf8(out.stdout).expect("UTF-8")
}

/// Subcomandos de una ayuda (sección «Commands:», sin `help`).
fn subcommands(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|l| !l.starts_with("Commands"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| *c != "help")
        .map(str::to_owned)
        .collect()
}

/// Opciones largas de una ayuda (`--algo`).
fn long_options(help: &str) -> BTreeSet<String> {
    help.split(|c: char| c.is_whitespace() || c == ',' || c == '[' || c == '<')
        .filter(|w| w.starts_with("--") && w.len() > 2)
        .map(|w| {
            w.trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_owned()
        })
        .filter(|w| w.len() > 2)
        .collect()
}

/// Todos los comandos del CLI (como rutas) con sus opciones.
fn cli_tree(fx: &Fx) -> Vec<(Vec<String>, BTreeSet<String>)> {
    let mut out = Vec::new();
    let mut stack: Vec<Vec<String>> = vec![vec![]];
    while let Some(path) = stack.pop() {
        let args: Vec<&str> = path.iter().map(String::as_str).collect();
        let h = help(fx, &args);
        for sub in subcommands(&h) {
            let mut p = path.clone();
            p.push(sub);
            stack.push(p);
        }
        out.push((path, long_options(&h)));
    }
    out
}

fn generate(fx: &Fx, shell: &str) -> String {
    let out = fx
        .cmd()
        .args(["completion", shell])
        .output()
        .expect("ejecutar");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{shell}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("UTF-8")
}

#[test]
fn scripts_complete_every_subcommand_and_option() {
    let fx = fixture();
    let tree = cli_tree(&fx);
    let names: BTreeSet<&str> = tree
        .iter()
        .filter_map(|(p, _)| p.last().map(String::as_str))
        .collect();
    // Lo que hay hoy, como mínimo; los comandos nuevos entran solos.
    for c in [
        "scan",
        "clean",
        "history",
        "undo",
        "free",
        "doctor",
        "exclude",
        "add",
        "list",
        "remove",
        "trash",
        "put",
        "empty",
        "completion",
    ] {
        assert!(names.contains(c), "falta {c} en la ayuda: {names:?}");
    }
    let options: BTreeSet<&str> = tree
        .iter()
        .flat_map(|(_, o)| o.iter().map(String::as_str))
        .collect();
    for o in [
        "--dry-run",
        "--json",
        "--yes",
        "--no-color",
        "--debug",
        "--lang",
        "--category",
        "--permanent",
        "--include-review",
        "--items",
    ] {
        assert!(options.contains(o), "falta {o} en la ayuda: {options:?}");
    }

    for shell in ["zsh", "bash", "fish"] {
        let script = generate(&fx, shell);
        for (path, opts) in &tree {
            let Some(name) = path.last() else { continue };
            // Cómo aparece un subcomando en cada script.
            let parent = path[..path.len() - 1].join("__subcmd__");
            let as_sub = match shell {
                "zsh" => format!("'{name}:"),
                "bash" if parent.is_empty() => format!("rotten,{name})"),
                "bash" => format!("rotten__subcmd__{parent},{name})"),
                _ => format!("-a \"{name}\""),
            };
            assert!(
                script.contains(&as_sub),
                "{shell}: falta el subcomando {path:?} ({as_sub})"
            );
            for o in opts {
                let bare = o.trim_start_matches('-');
                let found = match shell {
                    "zsh" => {
                        script.contains(&format!("{o}[")) || script.contains(&format!("{o}=["))
                    }
                    "bash" => script.split_whitespace().any(|w| w.trim_matches('"') == o),
                    _ => script.contains(&format!("-l {bare} ")),
                };
                assert!(found, "{shell}: falta la opción {o} de {path:?}");
            }
        }
        // Los valores cerrados también se completan.
        for v in ["zsh", "bash", "fish", "es", "en"] {
            assert!(script.contains(v), "{shell}: falta el valor {v}");
        }
    }
}

#[test]
fn scripts_have_valid_syntax() {
    let fx = fixture();
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut checked = 0;
    for shell in ["zsh", "bash", "fish"] {
        let script = generate(&fx, shell);
        match syntax_ok(shell, &script, tmp.path()) {
            Some(Ok(())) => checked += 1,
            Some(Err(e)) => panic!("{shell} -n: {e}"),
            None => eprintln!("{shell} no está instalada: sin comprobar su sintaxis"),
        }
    }
    // macOS trae zsh y bash.
    assert!(checked >= 2, "solo {checked} shells comprobadas");
}

/// La salida es el script aunque vaya a una tubería, sin la carpeta
/// personal ni el log; una shell desconocida es un error de uso.
#[test]
fn completion_needs_nothing_and_rejects_unknown_shells() {
    let fx = fixture();
    fs::remove_dir(fx.log_dir()).expect("rmdir");
    let out = fx
        .cmd()
        .env_remove("ROTTEN_HOME")
        .env_remove("HOME")
        .args(["completion", "bash", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("_rotten()"));
    assert!(!fx.log_dir().exists(), "completion no escribe en el log");

    for args in [&["completion"][..], &["completion", "powershell"][..]] {
        let out = fx.cmd().args(args).output().expect("ejecutar");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
    }
}
