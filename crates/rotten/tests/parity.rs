//! Prueba de paridad de comandos (T-02 · RF-CLI-20, RF-CLI-21).
//!
//! `parity.toml` lista cada comando de 07 §2 y §3 con su fase (tras 08
//! §2.1) y su estado. La prueba:
//!
//! - ejecuta cada comando «done» sobre la carpeta personal de prueba
//!   (`--dry-run --json`, o `--json` si es de lectura) y valida su JSON
//!   contra `schemas/` (lo hace `Fx::run`);
//! - falla si un comando «pending» ya existe, o si el CLI tiene un
//!   subcomando que no está en la tabla;
//! - con `ROTTEN_PARITY_STRICT=1`, falla mientras quede algún comando del MVP
//!   pendiente. Esa variante es la puerta de salida del MVP (T-42); no va en
//!   la CI de cada PR.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use common::fixture;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    command: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    cli: String,
    mole: String,
    source: String,
    phase: Phase,
    status: Status,
    task: String,
    #[serde(default)]
    run: Option<Vec<String>>,
    #[serde(default)]
    setup: Vec<Vec<String>>,
    #[serde(default)]
    exit: i32,
    #[serde(default = "yes")]
    json: bool,
    /// Solo documentación.
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Mvp,
    V1,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Done,
    Pending,
}

impl Entry {
    /// Las palabras del subcomando: lo que va tras `rotten` hasta el primer
    /// argumento (`<…>`, `[…]`) o flag.
    fn words(&self) -> Vec<&str> {
        self.cli
            .split_whitespace()
            .skip(1)
            .take_while(|w| !w.starts_with(['-', '<', '[']))
            .collect()
    }
}

/// Argumentos de `run` o `setup`, con `$HOME` sustituido por la carpeta
/// personal de prueba (los comandos que reciben rutas, como `trash put`, no
/// expanden `~`: eso lo hace la shell) y `$OP` por la operación de la
/// última preparación (`free <op-id>`).
fn args_in<'a>(raw: &'a [String], home: &str, op: &str) -> Vec<std::borrow::Cow<'a, str>> {
    raw.iter()
        .map(|a| {
            if a.contains("$HOME") || a.contains("$OP") {
                a.replace("$HOME", home).replace("$OP", op).into()
            } else {
                a.as_str().into()
            }
        })
        .collect()
}

fn table() -> Vec<Entry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity.toml");
    let text = fs::read_to_string(&path).expect("leer parity.toml");
    let t: Table = toml::from_str(&text).unwrap_or_else(|e| panic!("parity.toml: {e}"));
    t.command
}

#[test]
fn table_is_well_formed() {
    let entries = table();
    let mut ids = BTreeSet::new();
    for e in &entries {
        assert!(ids.insert(e.id.as_str()), "id repetido: {}", e.id);
        assert!(e.cli.starts_with("rotten"), "{}: cli = {:?}", e.id, e.cli);
        assert!(!e.source.is_empty(), "{}: falta source", e.id);
        match e.status {
            Status::Done => assert!(e.run.is_some(), "{}: «done» sin `run`", e.id),
            Status::Pending => {
                assert!(e.task.starts_with("T-"), "{}: «pending» sin tarea", e.id);
                assert!(e.run.is_none(), "{}: `run` solo en «done»", e.id);
            }
        }
        if !e.mole.is_empty() {
            // 08 §2.1 (D-11): todo lo de Mole entra en el MVP, salvo el
            // servidor MCP, que 08 §2.1 deja en v2.
            assert!(
                e.phase == Phase::Mvp || e.id == "mcp",
                "{}: equivalente de `{}` fuera del MVP",
                e.id,
                e.mole
            );
        }
    }
    // Los 15 comandos de Mole (07 §2), con su equivalente.
    let mole: BTreeSet<&str> = entries
        .iter()
        .filter_map(|e| e.mole.split_whitespace().nth(1))
        .filter(|w| !w.starts_with('-'))
        .collect();
    for m in [
        "clean",
        "uninstall",
        "optimize",
        "purge",
        "installer",
        "slim",
        "analyze",
        "status",
        "history",
        "touchid",
        "completion",
        "update",
        "remove",
    ] {
        assert!(mole.contains(m), "falta el equivalente de `mo {m}`");
    }
    assert!(
        entries.iter().any(|e| e.mole == "mo"),
        "falta el menú (`mo`)"
    );
    assert!(
        entries.iter().any(|e| e.mole == "mo --version"),
        "falta `--version`"
    );
}

/// Cada comando «done» existe y su JSON valida contra su esquema.
#[test]
fn done_commands_run_and_validate() {
    let mut checked = Vec::new();
    for e in table().iter().filter(|e| e.status == Status::Done) {
        let fx = fixture();
        let home = fx.home.to_str().expect("ruta UTF-8");
        let mut op = String::new();
        for s in &e.setup {
            let args = args_in(s, home, &op);
            let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
            let (code, doc) = fx.run(&args);
            assert_eq!(code, 0, "{}: preparación {args:?} falló: {doc:#}", e.id);
            if let Some(o) = doc["report"]["op_id"].as_str() {
                op = o.to_owned();
            }
        }
        assert!(
            !e.run.iter().flatten().any(|a| a.contains("$OP")) || !op.is_empty(),
            "{}: `$OP` sin una preparación que cree una operación",
            e.id
        );
        let args = args_in(e.run.as_deref().expect("run"), home, &op);
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        if e.json {
            let (code, doc) = fx.run(&args);
            assert_eq!(code, e.exit, "{}: {args:?} → {doc:#}", e.id);
        } else {
            let out = fx.cmd().args(&args).output().expect("ejecutar");
            assert_eq!(
                out.status.code(),
                Some(e.exit),
                "{}: {args:?}\n{}",
                e.id,
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(!out.stdout.is_empty(), "{}: {args:?} sin salida", e.id);
        }
        checked.push(e.id.as_str().to_owned());
    }
    assert!(checked.len() >= 10, "solo {checked:?}");
}

/// Un comando «pending» que ya existe obliga a actualizar la tabla. Se
/// comprueba con `--help`, que no ejecuta nada: un subcomando desconocido
/// es un error de uso (`2`).
///
/// Si el comando es a la vez el grupo de otro ya hecho (`rotten trash`
/// pendiente, `rotten trash put` hecho), `--help` sale con `0` porque el
/// grupo existe. Entonces se sondea a secas: sin implementar, falta el
/// subcomando y también es un error de uso.
#[test]
fn pending_commands_do_not_exist_yet() {
    let fx = fixture();
    let entries = table();
    let done: Vec<Vec<&str>> = entries
        .iter()
        .filter(|e| e.status == Status::Done)
        .map(Entry::words)
        .collect();
    for e in entries.iter().filter(|e| e.status == Status::Pending) {
        let words = e.words();
        if words.is_empty() {
            continue; // El menú (`rotten` a secas) no se puede sondear así.
        }
        let is_group = done
            .iter()
            .any(|d| d.len() > words.len() && d.starts_with(&words));
        let mut cmd = fx.cmd();
        cmd.args(&words);
        if !is_group {
            cmd.arg("--help");
        }
        let out = cmd.output().expect("ejecutar");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`rotten {}` ya existe: marca «{}» como done en parity.toml, con `run` y su esquema",
            words.join(" "),
            e.id
        );
    }
}

/// Todo subcomando del CLI está en la tabla.
#[test]
fn every_cli_subcommand_is_in_the_table() {
    let entries = table();
    let known: BTreeSet<&str> = entries
        .iter()
        .filter_map(|e| e.words().first().copied())
        .collect();
    let fx = fixture();
    let out = fx.cmd().arg("--help").output().expect("ejecutar");
    let help = String::from_utf8_lossy(&out.stdout);
    let commands: Vec<&str> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| *c != "help")
        .collect();
    assert!(!commands.is_empty(), "sin «Commands:» en --help:\n{help}");
    for c in commands {
        assert!(known.contains(c), "`rotten {c}` no está en parity.toml");
    }
}

/// Puerta del MVP (T-42): solo con `ROTTEN_PARITY_STRICT=1`.
#[test]
fn strict_mvp_has_nothing_pending() {
    if std::env::var("ROTTEN_PARITY_STRICT").as_deref() != Ok("1") {
        return;
    }
    let pending: Vec<String> = table()
        .iter()
        .filter(|e| e.phase == Phase::Mvp && e.status == Status::Pending)
        .map(|e| format!("{} ({})", e.cli, e.task))
        .collect();
    assert!(
        pending.is_empty(),
        "quedan {} comandos del MVP pendientes:\n  {}",
        pending.len(),
        pending.join("\n  ")
    );
}
