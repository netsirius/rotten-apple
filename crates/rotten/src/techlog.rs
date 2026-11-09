//! Log técnico local y rotado (RF-SEG-11, RNF-33, T-14).
//!
//! Complementa al journal: el journal es la fuente de verdad de cada
//! operación (y la que permite deshacer); este log sirve para diagnosticar.
//!
//! **Dónde.** `<home>/Library/Logs/<BUNDLE_ID>/rotten.log` (con
//! `ROTTEN_HOME`, dentro de la carpeta personal de prueba). La carpeta se crea
//! con permisos `0700` y el fichero con `0600` la primera vez que se escribe.
//! Nunca sale de la máquina: no hay envío ni telemetría.
//!
//! **Rotación.** Por tamaño: antes de escribir una línea que haría pasar
//! `rotten.log` de [`MAX_BYTES`], se renombra a `rotten.log.1` (y este a
//! `.2`, hasta [`KEEP`]; el más antiguo se pierde). Como mucho hay
//! `KEEP + 1` ficheros, unos 4 MB.
//!
//! **Formato.** Una línea JSON por evento (*JSON Lines*), en inglés técnico
//! y sin traducir: `ts` (RFC 3339, UTC), `level`, `pid`, `event` y sus
//! campos.
//!
//! **Qué se registra.**
//!
//! | Nivel | Evento | Campos |
//! |---|---|---|
//! | `info` | `start`, `end` | comando, versión; código de salida y duración |
//! | `info` | `operation` | comando, `op_id`, hechos, omitidos, fallidos y bytes |
//! | `warn` | `item` omitido o fallido | `op_id`, id del elemento, ruta, bytes, código del motivo y mensaje |
//! | `warn`/`error` | `error` | comando, código estable (`error.code`), código de salida y mensaje en inglés |
//! | `debug` | `item` hecho | `op_id`, id, ruta, bytes y dónde quedó |
//! | `debug` | `env` | de dónde sale la carpeta personal, tipo de Papelera, idioma y si la salida es JSON |
//!
//! `operation` e `item` solo salen cuando una operación se ejecuta (`clean`,
//! `undo`, `free`, `trash put`, `trash empty`); las simulaciones
//! (`--dry-run`) dejan solo `start` y `end`.
//!
//! **Qué no se registra, nunca.** Contenido de ficheros (ni fragmentos: de
//! los mensajes de error solo va la primera línea, porque los errores de
//! los analizadores, como el de TOML, citan el texto del fichero en las
//! siguientes); la línea de órdenes tal cual; el JSON que se emite; las
//! variables de entorno; nada que no sea de esta máquina.
//!
//! **Rutas (RNF-33).** Son datos personales: se quedan en este log local y
//! la carpeta personal se abrevia como `~`, para que no aparezca el nombre
//! de usuario si alguien comparte el fichero.
//!
//! **Niveles.** `info` por defecto. `ROTTEN_LOG=off|error|warn|info|debug`
//! lo cambia; `--debug` lo sube a `debug` y además copia cada línea a
//! stderr (nunca a stdout, para no romper `--json`).
//!
//! **Nunca hace fallar una operación.** Cualquier error al crear la
//! carpeta, rotar o escribir se ignora y se sigue sin log. Lo que ocurre
//! antes de conocer la carpeta personal se guarda en memoria y se escribe
//! al conocerla; si no se llega a conocer (falta `HOME`), solo sale por
//! stderr con `--debug`. `rotten completion` no escribe en el log.
//!
//! **Coste.** Cada línea abre el fichero en modo `O_APPEND`, escribe y lo
//! cierra (así una rotación de otro proceso no deja a este escribiendo en
//! `rotten.log.1`). En `info` son unas pocas líneas por comando; en
//! `debug`, una más por elemento de la operación.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use ra_core::{BUNDLE_ID, ExecReport, Outcome};
use serde_json::{Value, json};

/// Nombre del fichero activo; los rotados llevan `.1`, `.2`…
pub const FILE_NAME: &str = "rotten.log";
/// Tamaño a partir del cual se rota.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// Ficheros rotados que se conservan.
pub const KEEP: usize = 3;
/// Variable con el nivel.
pub const LEVEL_ENV: &str = "ROTTEN_LOG";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    /// `ROTTEN_LOG`: `None` es `off`; un valor desconocido, `info`.
    fn from_env(v: Option<&str>) -> Option<Level> {
        match v.map(str::to_ascii_lowercase).as_deref() {
            Some("off") => None,
            Some("error") => Some(Level::Error),
            Some("warn") => Some(Level::Warn),
            Some("debug") => Some(Level::Debug),
            _ => Some(Level::Info),
        }
    }
}

/// Carpeta del log para una carpeta personal.
pub fn dir_for(home: &Path) -> PathBuf {
    home.join("Library/Logs").join(BUNDLE_ID)
}

struct State {
    /// Nivel del fichero (`None`: sin fichero).
    file_level: Option<Level>,
    /// `--debug`: todo, también a stderr.
    stderr: bool,
    /// Carpeta del log, cuando ya se conoce la carpeta personal.
    dir: Option<PathBuf>,
    /// Carpeta personal, para abreviar las rutas con `~`.
    home: Option<String>,
    /// Líneas anteriores a conocer la carpeta (ya filtradas por nivel).
    pending: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state(f: impl FnOnce(&mut State)) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = guard.as_mut() {
        f(s);
    }
}

/// Arranca el log (aún sin carpeta: lo que llegue antes se guarda).
pub fn init(debug: bool) {
    let env = std::env::var(LEVEL_ENV).ok();
    let file_level = if debug {
        Some(Level::Debug)
    } else {
        Level::from_env(env.as_deref())
    };
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(State {
        file_level,
        stderr: debug,
        dir: None,
        home: None,
        pending: Vec::new(),
    });
}

/// Ya se sabe la carpeta personal: el log va a [`dir_for`] y se escribe lo
/// pendiente.
pub fn set_home(home: &Path) {
    with_state(|s| {
        s.dir = Some(dir_for(home));
        s.home = home.to_str().map(str::to_owned);
        if let Some(dir) = &s.dir {
            for l in std::mem::take(&mut s.pending) {
                write_file(dir, &l);
            }
        }
    });
}

/// Abrevia la carpeta personal como `~` en todos los textos.
fn abbreviate(v: &mut Value, home: &str) {
    match v {
        Value::String(s) => {
            if s.contains(home) {
                let mut out = String::with_capacity(s.len());
                let mut rest = s.as_str();
                while let Some(i) = rest.find(home) {
                    let after = &rest[i + home.len()..];
                    out.push_str(&rest[..i]);
                    // Solo la carpeta entera: `/Users/ana` no abrevia `/Users/anabel`.
                    if after.is_empty() || after.starts_with('/') {
                        out.push('~');
                    } else {
                        out.push_str(home);
                    }
                    rest = after;
                }
                out.push_str(rest);
                *s = out;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| abbreviate(x, home)),
        Value::Object(m) => m.values_mut().for_each(|x| abbreviate(x, home)),
        _ => {}
    }
}

/// Una línea JSON: primero `ts`, `level`, `pid` y `event`; luego los campos.
fn line(level: Level, name: &str, fields: &Value) -> String {
    let head = format!(
        "{{\"ts\":{},\"level\":{},\"pid\":{},\"event\":{}",
        json!(jiff::Timestamp::now().to_string()),
        json!(level.as_str()),
        std::process::id(),
        json!(name),
    );
    let rest = match fields {
        Value::Object(m) if !m.is_empty() => fields.to_string(),
        _ => return format!("{head}}}\n"),
    };
    format!("{head},{}\n", rest.strip_prefix('{').unwrap_or(&rest))
}

/// Sin log si falla: nunca se interrumpe la operación.
fn write_file(dir: &Path, line: &str) {
    let _ = append(dir, line, MAX_BYTES, KEEP);
}

/// Un evento con sus campos (un objeto JSON).
pub fn event(level: Level, name: &str, mut fields: Value) {
    with_state(|s| {
        let to_file = s.file_level.is_some_and(|l| level <= l);
        if !to_file && !s.stderr {
            return;
        }
        if let Some(home) = &s.home {
            abbreviate(&mut fields, home);
        }
        let l = line(level, name, &fields);
        if s.stderr {
            let _ = io::stderr().write_all(l.as_bytes());
        }
        if !to_file {
            return;
        }
        match &s.dir {
            Some(dir) => write_file(dir, &l),
            None => s.pending.push(l),
        }
    });
}

/// Añade `line` a `dir/rotten.log`, rotando antes si pasaría de `max`.
fn append(dir: &Path, line: &str, max: u64, keep: usize) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let path = dir.join(FILE_NAME);
    if let Ok(m) = fs::symlink_metadata(&path)
        && m.len() > 0
        && m.len() + line.len() as u64 > max
    {
        rotate(dir, keep);
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    // Una sola escritura con `O_APPEND`: las líneas de dos procesos no se
    // mezclan.
    f.write_all(line.as_bytes())
}

/// `rotten.log` → `.1` → `.2` … → `.keep`; lo que había en `.keep` se pierde.
fn rotate(dir: &Path, keep: usize) {
    let numbered = |i: usize| dir.join(format!("{FILE_NAME}.{i}"));
    let current = dir.join(FILE_NAME);
    if keep == 0 {
        let _ = fs::remove_file(&current);
        return;
    }
    for i in (1..keep).rev() {
        let _ = fs::rename(numbered(i), numbered(i + 1));
    }
    let _ = fs::rename(&current, numbered(1));
}

/// Primera línea de un mensaje (las siguientes pueden citar un fichero).
fn first_line(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

// Eventos del CLI.

pub fn start(command: &str) {
    event(
        Level::Info,
        "start",
        json!({ "command": command, "version": env!("CARGO_PKG_VERSION") }),
    );
}

pub fn finish(command: &str, exit: u8, started: Instant) {
    event(
        Level::Info,
        "end",
        json!({
            "command": command,
            "exit_code": exit,
            "duration_ms": started.elapsed().as_millis() as u64,
        }),
    );
}

pub fn env(env: &ra_core::Env, lang: &str, json_output: bool) {
    let trash = match env.trash {
        ra_core::TrashBackend::System => "system",
        ra_core::TrashBackend::Dir(_) => "dir",
    };
    event(
        Level::Debug,
        "env",
        json!({
            "home_source": env.home_source,
            "trash": trash,
            "lang": lang,
            "json": json_output,
        }),
    );
}

/// Un error que termina el comando. Los de uso y los esperables (`2`, `4`)
/// son `warn`; el resto, `error`.
pub fn error(command: &str, code: &str, exit: u8, message: &str) {
    let level = if matches!(exit, 2 | 4) {
        Level::Warn
    } else {
        Level::Error
    };
    event(
        level,
        "error",
        json!({
            "command": command,
            "code": code,
            "exit_code": exit,
            "message": first_line(message),
        }),
    );
}

/// El resultado de una operación que cambia algo, con lo omitido o fallido
/// de cada elemento (y, en `debug`, también lo hecho).
pub fn report(command: &str, r: &ExecReport) {
    let op = r.op_id.as_deref().unwrap_or_default();
    event(
        Level::Info,
        "operation",
        json!({
            "command": command,
            "op_id": op,
            "done": r.done,
            "skipped": r.skipped,
            "failed": r.failed,
            "bytes_done": r.bytes_done,
        }),
    );
    for item in &r.items {
        let (level, outcome) = match &item.outcome {
            Outcome::Done { .. } => (Level::Debug, "done"),
            Outcome::Would { .. } => (Level::Debug, "would"),
            Outcome::Skipped { .. } => (Level::Warn, "skipped"),
            Outcome::Failed { .. } => (Level::Warn, "failed"),
        };
        let mut fields = serde_json::to_value(item).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut fields {
            for k in ["message", "error"] {
                if let Some(Value::String(s)) = m.get_mut(k) {
                    *s = first_line(s).to_owned();
                }
            }
            m.insert("op_id".into(), json!(op));
            m.insert("outcome".into(), json!(outcome));
        }
        event(level, "item", fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .expect("leer")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn rotates_by_size_and_keeps_a_bounded_number_of_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("Logs/app");
        let line = format!("{}\n", "x".repeat(99));
        for i in 0..50 {
            append(&dir, &line, 250, 2).unwrap_or_else(|e| panic!("línea {i}: {e}"));
        }
        assert_eq!(files(&dir), ["rotten.log", "rotten.log.1", "rotten.log.2"]);
        for f in files(&dir) {
            let len = fs::metadata(dir.join(&f)).expect("stat").len();
            assert!(len <= 250 && len > 0, "{f}: {len}");
        }
        // Permisos: solo el usuario.
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(FILE_NAME)), 0o600);
    }

    #[test]
    fn a_line_bigger_than_the_limit_is_still_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let big = format!("{}\n", "y".repeat(500));
        append(tmp.path(), &big, 100, 1).expect("append");
        append(tmp.path(), &big, 100, 1).expect("append");
        assert_eq!(files(tmp.path()), ["rotten.log", "rotten.log.1"]);
    }

    #[test]
    fn writing_where_it_cannot_is_an_error_not_a_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("not-a-dir");
        fs::write(&file, b"").expect("write");
        assert!(append(&file.join("sub"), "x\n", 100, 1).is_err());
    }

    #[test]
    fn home_is_abbreviated_only_as_a_whole_folder() {
        let mut v = json!({
            "path": "/Users/ana/Library/Logs/x",
            "message": "cannot move /Users/ana/a to /Users/anabel/b",
            "home": "/Users/ana",
            "n": 3,
        });
        abbreviate(&mut v, "/Users/ana");
        assert_eq!(v["path"], "~/Library/Logs/x");
        assert_eq!(v["message"], "cannot move ~/a to /Users/anabel/b");
        assert_eq!(v["home"], "~");
    }

    #[test]
    fn levels_from_the_environment() {
        assert_eq!(Level::from_env(None), Some(Level::Info));
        assert_eq!(Level::from_env(Some("off")), None);
        assert_eq!(Level::from_env(Some("DEBUG")), Some(Level::Debug));
        assert_eq!(Level::from_env(Some("warn")), Some(Level::Warn));
        assert_eq!(Level::from_env(Some("nonsense")), Some(Level::Info));
        assert!(Level::Debug > Level::Info && Level::Warn < Level::Info);
    }

    #[test]
    fn only_the_first_line_of_a_message() {
        assert_eq!(
            first_line("TOML parse error at line 1\n1 | secret = 1\n  | ^"),
            "TOML parse error at line 1"
        );
        assert_eq!(first_line(""), "");
    }
}
