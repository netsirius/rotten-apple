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
//! | `info` | `start`, `end` | comando, versión (y en `config get|set`, la clave si es una de las conocidas); código de salida y duración |
//! | `info` | `operation` | comando, `op_id`, hechos, omitidos, fallidos y bytes |
//! | `warn` | `item` omitido o fallido | `op_id`, id del elemento, ruta, bytes y código del motivo (`reason`) o número de error del sistema (`os_error`) |
//! | `warn`/`error` | `error` | comando, código estable (`error.code`), código de salida y la clave de `config` si la hay |
//! | `debug` | `item` hecho | `op_id`, id, ruta, bytes y dónde quedó |
//! | `debug` | `env` | de dónde sale la carpeta personal, tipo de Papelera, idioma y si la salida es JSON |
//!
//! `operation` e `item` solo salen cuando una operación se ejecuta (`clean`,
//! `undo`, `free`, `trash put`, `trash empty`); las simulaciones
//! (`--dry-run`) dejan solo `start` y `end`.
//!
//! **Qué no se registra, nunca.** Ningún texto libre: ni los mensajes de
//! error ni los detalles de los motivos de omisión, porque pueden llevar lo
//! que escribió el usuario (el valor de `config set`, una exclusión, una
//! operación o una categoría tecleadas), rutas fuera de la carpeta personal
//! o líneas de un fichero (el error de TOML cita la línea rota). Solo
//! códigos estables, números y rutas. Tampoco el contenido de ficheros, la
//! línea de órdenes, el JSON que se emite ni las variables de entorno.
//!
//! Los errores de uso que detecta clap al analizar los argumentos (salida
//! `2`: opción desconocida, falta un argumento…) no se registran: el log
//! empieza después del análisis, y registrarlos obligaría a guardar la
//! línea de órdenes.
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
//! carpeta, rotar o escribir se ignora y se sigue sin log. Solo se escribe
//! en un fichero normal con un único enlace, dentro de una carpeta real: si
//! `rotten.log` o la carpeta son un enlace simbólico, un FIFO u otra cosa,
//! no se escribe (se abre con `O_NOFOLLOW` y `O_NONBLOCK`, y se comprueba
//! lo abierto), para no escribir en un fichero ajeno ni quedarse bloqueado. Lo que ocurre
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
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
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
    /// Campos seguros del comando que se añaden a sus errores (la clave de
    /// `config`).
    context: serde_json::Map<String, Value>,
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
        context: serde_json::Map::new(),
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

fn refuse(what: &str) -> io::Error {
    io::Error::other(format!("the log {what}: not written"))
}

/// ¿Un fichero normal y con un único nombre? Con otro nombre (enlace
/// duro), escribir en el log sería escribir en otro fichero.
fn is_plain_file(m: &fs::Metadata) -> bool {
    m.file_type().is_file() && m.nlink() == 1
}

/// Añade `line` a `dir/rotten.log`, rotando antes si pasaría de `max`.
/// Solo escribe en un fichero normal de una carpeta real (ver el `//!`).
fn append(dir: &Path, line: &str, max: u64, keep: usize) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    if !fs::symlink_metadata(dir)?.file_type().is_dir() {
        return Err(refuse("folder is not a real folder"));
    }
    let path = dir.join(FILE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(m) if !is_plain_file(&m) => return Err(refuse("file is not a plain file")),
        Ok(m) if m.len() > 0 && m.len() + line.len() as u64 > max => rotate(dir, keep),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    // `O_NOFOLLOW`: si entre la comprobación y la apertura aparece un
    // enlace, falla. `O_NONBLOCK`: un FIFO no bloquea la apertura (y se
    // rechaza abajo).
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)?;
    if !is_plain_file(&f.metadata()?) {
        return Err(refuse("file is not a plain file"));
    }
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

/// El número de error del sistema de un mensaje de E/S (`… (os error 13)`):
/// lo único que se guarda de él.
fn os_error(message: &str) -> Option<i32> {
    let start = message.rfind("(os error ")? + "(os error ".len();
    let rest = &message[start..];
    rest[..rest.find(')')?].parse().ok()
}

// Eventos del CLI.

/// Inicio del comando. `config_key` es la clave de `config get|set`: solo
/// se guarda si es una de las conocidas (si no, es texto del usuario) y
/// se añade también a los errores del comando.
pub fn start(command: &str, config_key: Option<&str>) {
    let mut fields = json!({ "command": command, "version": env!("CARGO_PKG_VERSION") });
    if let Some(k) = config_key {
        let key = if ra_core::CONFIG_KEYS.contains(&k) {
            json!(k)
        } else {
            json!("(unknown)")
        };
        with_state(|s| {
            s.context.insert("key".into(), key.clone());
        });
        if let Value::Object(m) = &mut fields {
            m.insert("key".into(), key);
        }
    }
    event(Level::Info, "start", fields);
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

/// Un error que termina el comando: su código estable, nunca el mensaje
/// (puede llevar lo que escribió el usuario). Los de uso y los esperables
/// (`2`, `4`) son `warn`; el resto, `error`.
pub fn error(command: &str, code: &str, exit: u8) {
    let level = if matches!(exit, 2 | 4) {
        Level::Warn
    } else {
        Level::Error
    };
    let mut fields = json!({
        "command": command,
        "code": code,
        "exit_code": exit,
    });
    with_state(|s| {
        if let Value::Object(m) = &mut fields {
            m.extend(s.context.clone());
        }
    });
    event(level, "error", fields);
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
        // Campo a campo: los detalles de los motivos (la exclusión que
        // coincide, el nombre de una app, un mensaje de E/S) no van.
        let mut fields = json!({
            "op_id": op,
            "item_id": item.item_id,
            "path": item.path,
            "bytes": item.bytes,
        });
        let level = match &item.outcome {
            Outcome::Done { result_path, .. } => {
                fields["outcome"] = json!("done");
                if let Some(p) = result_path {
                    fields["result_path"] = json!(p);
                }
                Level::Debug
            }
            Outcome::Would { .. } => {
                fields["outcome"] = json!("would");
                Level::Debug
            }
            Outcome::Skipped { cause, .. } => {
                fields["outcome"] = json!("skipped");
                fields["reason"] = json!(cause.code());
                Level::Warn
            }
            Outcome::Failed { error } => {
                fields["outcome"] = json!("failed");
                if let Some(n) = os_error(error) {
                    fields["os_error"] = json!(n);
                }
                Level::Warn
            }
        };
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
    fn only_the_os_error_number_of_an_io_message() {
        assert_eq!(
            os_error("cannot move ~/a: Permission denied (os error 13)"),
            Some(13)
        );
        assert_eq!(os_error("it changed since the review"), None);
        assert_eq!(os_error("(os error x)"), None);
    }

    /// M2: nunca a través de un enlace (simbólico o duro), ni en un FIFO,
    /// ni en una carpeta que es un enlace. El destino queda intacto.
    #[test]
    fn writes_only_to_a_plain_file_in_a_real_folder() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let victim = tmp.path().join("victim.txt");
        fs::write(&victim, "precious").expect("write");

        let dir = tmp.path().join("symlink");
        fs::create_dir(&dir).expect("mkdir");
        std::os::unix::fs::symlink(&victim, dir.join(FILE_NAME)).expect("ln -s");
        assert!(append(&dir, "x\n", 100, 1).is_err());
        // Tampoco al rotar: el enlace no se renombra como si fuera el log.
        assert!(append(&dir, &"y".repeat(200), 100, 1).is_err());

        let dir = tmp.path().join("hardlink");
        fs::create_dir(&dir).expect("mkdir");
        fs::hard_link(&victim, dir.join(FILE_NAME)).expect("ln");
        assert!(append(&dir, "x\n", 100, 1).is_err());

        let real = tmp.path().join("real");
        fs::create_dir(&real).expect("mkdir");
        let dir = tmp.path().join("dirlink");
        std::os::unix::fs::symlink(&real, &dir).expect("ln -s");
        assert!(append(&dir, "x\n", 100, 1).is_err());
        assert!(!real.join(FILE_NAME).exists());

        assert_eq!(fs::read_to_string(&victim).expect("leer"), "precious");
    }

    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fifo = tmp.path().join(FILE_NAME);
        let st = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(st.success());
        let t = Instant::now();
        assert!(append(tmp.path(), "x\n", 100, 1).is_err());
        assert!(t.elapsed().as_secs() < 5);
    }
}
