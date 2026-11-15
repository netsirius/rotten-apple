//! Colores (T-14, 07 §4): solo en una terminal, sin `--json`, sin
//! `--no-color` y sin `NO_COLOR` no vacía. La regla está probada en
//! `style::tests`; aquí, el binario.

mod common;

use std::process::{Command, Output};

use common::{Fx, fixture};

const ESC: u8 = 0x1b;

fn has_ansi(o: &Output) -> bool {
    o.stdout.contains(&ESC) || o.stderr.contains(&ESC)
}

fn show(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Pasos con cabeceras, resultados, omisiones y un error.
const STEPS: &[&[&str]] = &[
    &["scan"],
    &["clean", "--dry-run"],
    &["undo", "no-such-op"],
    &["clean", "--yes"],
    &["undo", "--yes"],
];

/// En una tubería nunca hay colores, ni para personas (`ROTTEN_HUMAN=1`)
/// ni en JSON.
#[test]
fn no_colors_when_the_output_is_not_a_terminal() {
    let fx = fixture();
    for args in STEPS {
        for json in [false, true] {
            let mut c = fx.cmd();
            if !json {
                c.env("ROTTEN_HUMAN", "1");
            }
            c.env_remove("NO_COLOR").args(*args);
            if json {
                c.arg("--json");
            }
            let o = c.output().expect("ejecutar");
            assert!(!o.stdout.is_empty() || !o.stderr.is_empty(), "{args:?}");
            assert!(!has_ansi(&o), "{args:?} (json: {json}): {}", show(&o));
        }
    }
}

/// `script(1)` da una terminal al proceso. Devuelve stdout y stderr
/// juntos, tal como los vería la persona.
fn in_terminal(fx: &Fx, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rotten");
    let mut c = Command::new("script");
    c.arg("-q")
        .arg("/dev/null")
        .arg(bin)
        .args(args)
        .env("ROTTEN_HOME", &fx.home)
        .env("ROTTEN_TRASH_DIR", &fx.trash)
        .env("LANG", "en_US.UTF-8")
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("ROTTEN_STATE_DIR")
        .env_remove("NO_COLOR");
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().expect("ejecutar script");
    assert!(
        o.status.success() || o.status.code() == Some(1),
        "script: {}",
        show(&o)
    );
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("openpty"),
        "sin pty: {}",
        show(&o)
    );
    o
}

/// En una terminal hay colores, salvo con `--no-color`, `NO_COLOR` no
/// vacía o `--json`. Necesita una pty (`script`), que el sandbox de las
/// pruebas no permite: `cargo test -- --ignored` fuera de él.
#[test]
#[ignore = "necesita una pty (script(1)); el sandbox no la permite"]
fn colors_on_a_terminal_unless_disabled() {
    let fx = fixture();
    // Para que `scan` tenga cabecera y resultado, y el error vaya en rojo.
    for args in [&["scan"][..], &["undo", "no-such-op"][..]] {
        let o = in_terminal(&fx, &[], args);
        assert!(
            has_ansi(&o),
            "{args:?} sin colores en una terminal: {}",
            show(&o)
        );
        let o = in_terminal(&fx, &[("NO_COLOR", "")], args);
        assert!(
            has_ansi(&o),
            "{args:?}: NO_COLOR vacía no cuenta: {}",
            show(&o)
        );

        let mut no_color = args.to_vec();
        no_color.insert(0, "--no-color");
        let o = in_terminal(&fx, &[], &no_color);
        assert!(!has_ansi(&o), "{args:?} --no-color: {}", show(&o));
        for v in ["1", "0", "true"] {
            let o = in_terminal(&fx, &[("NO_COLOR", v)], args);
            assert!(!has_ansi(&o), "{args:?} NO_COLOR={v}: {}", show(&o));
        }
        let mut json = args.to_vec();
        json.push("--json");
        let o = in_terminal(&fx, &[], &json);
        assert!(!has_ansi(&o), "{args:?} --json: {}", show(&o));
    }
    // La ayuda de clap tampoco, con `--no-color`.
    let o = in_terminal(&fx, &[], &["--no-color", "--help"]);
    assert!(!has_ansi(&o), "--help --no-color: {}", show(&o));
}
