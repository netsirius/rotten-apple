//! Log técnico local y rotado (T-14: RF-SEG-11, RNF-33).
//!
//! - Aceptación de RF-SEG-11: ficheros con un marcador conocido pasan por
//!   `scan`, `clean`, `undo`, `trash put`, `free` y `trash empty` (con
//!   `--debug`, el nivel más detallado) y el marcador no aparece en el log;
//!   lo que sí debe estar (las operaciones, sus elementos) está, para que la
//!   prueba no pase con un log vacío.
//! - Tampoco lo que teclea el usuario (el valor de `config set`, una
//!   exclusión, una operación o una categoría): de los errores solo va el
//!   código.
//! - El log solo se escribe en un fichero normal: ni a través de un enlace
//!   ni en un FIFO (en el que se quedaría bloqueado).
//! - La carpeta del log está protegida: no se puede mandar a la Papelera.
//! - Las rutas se quedan en local y con la carpeta personal abreviada
//!   (RNF-33).
//! - `--debug` copia el log a stderr y nunca a stdout (`--json` sigue
//!   siendo un único documento válido).
//! - Si el log no se puede escribir, la operación sigue igual.
//! - Rotten Apple no propone tirar su propio log (CAT-03).

mod common;

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use common::{Fx, LOG_DIR, fixture};
use serde_json::Value;

const MARKER: &str = "MARCADOR-T14-c0ntenido-9f2b7e";

/// Todo lo que hay en la carpeta del log (activo y rotados), junto.
fn read_logs(fx: &Fx) -> String {
    let mut out = String::new();
    let Ok(rd) = fs::read_dir(fx.log_dir()) else {
        return out;
    };
    let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files {
        out.push_str(&String::from_utf8_lossy(&fs::read(&f).expect("leer log")));
    }
    out
}

/// Cada línea del log, como JSON.
fn events(log: &str) -> Vec<Value> {
    log.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("línea no JSON ({e}): {l}")))
        .collect()
}

/// Escribe `content` en `rel` con la fecha de hace `days` días (para que
/// la antigüedad mínima de la regla no lo bloquee).
fn write_old(fx: &Fx, rel: &str, content: &str, days: u64) {
    let p = fx.p(rel);
    fs::create_dir_all(p.parent().expect("padre")).expect("mkdir");
    fs::write(&p, content).expect("write");
    let when = SystemTime::now() - Duration::from_secs(days * 86_400);
    File::options()
        .write(true)
        .open(&p)
        .expect("open")
        .set_modified(when)
        .expect("mtime");
}

#[test]
fn the_log_never_contains_file_contents() {
    let fx = fixture();
    // Dentro de lo que `clean` mueve por defecto (CAT-03 y CAT-10).
    write_old(&fx, "Library/Logs/old-app/app.log", &MARKER.repeat(50), 30);
    write_old(&fx, "Library/Logs/install.log", MARKER, 20);
    write_old(
        &fx,
        "Library/Saved Application State/com.example.closed.savedState/windows.plist",
        MARKER,
        3,
    );

    let mut stderr = String::new();
    let mut run = |args: &[&str]| -> Value {
        let out = fx
            .cmd()
            .arg("--debug")
            .args(args)
            .arg("--json")
            .output()
            .expect("ejecutar");
        stderr.push_str(&String::from_utf8_lossy(&out.stderr));
        let doc: Value = serde_json::from_slice(&out.stdout).expect("stdout es JSON");
        assert_eq!(out.status.code(), Some(0), "{args:?}: {doc:#}");
        doc
    };
    run(&["scan"]);
    let clean = run(&["clean", "--yes"]);
    assert_eq!(clean["report"]["done"], 3, "{clean:#}");
    let op = clean["report"]["op_id"].as_str().expect("op_id").to_owned();
    let undo = run(&["undo", "--yes"]);
    assert_eq!(undo["report"]["done"], 3, "{undo:#}");
    // Los ficheros han vuelto con su contenido.
    let back = fs::read_to_string(fx.p("Library/Logs/install.log")).expect("leer");
    assert_eq!(back, MARKER);

    // Elegido a mano, liberado, y la Papelera entera con algo ajeno.
    write_old(&fx, "Downloads/notas.txt", MARKER, 1);
    let notes = fx.p("Downloads/notas.txt");
    let put = run(&["trash", "put", notes.to_str().expect("UTF-8"), "--yes"]);
    assert_eq!(put["report"]["done"], 1, "{put:#}");
    let put_op = put["report"]["op_id"].as_str().expect("op_id").to_owned();
    let free = run(&["free", &put_op, "--yes"]);
    assert_eq!(free["report"]["done"], 1, "{free:#}");
    run(&["clean", "--yes"]);
    fs::write(fx.trash.join("ajeno.txt"), MARKER).expect("write");
    let empty = run(&["trash", "empty", "--yes"]);
    assert_eq!(empty["report"]["done"], 4, "{empty:#}");

    let log = read_logs(&fx);
    assert!(!log.contains(MARKER), "el marcador está en el log:\n{log}");
    assert!(
        !stderr.contains(MARKER),
        "el marcador está en stderr:\n{stderr}"
    );

    // El log no está vacío: la operación, sus elementos y su deshacer.
    let ev = events(&log);
    let ops: Vec<&Value> = ev.iter().filter(|e| e["event"] == "operation").collect();
    let commands: Vec<&str> = ops
        .iter()
        .map(|o| o["command"].as_str().expect("command"))
        .collect();
    assert_eq!(
        commands,
        ["clean", "undo", "trash", "free", "clean", "trash"],
        "{log}"
    );
    assert_eq!(ops[0]["op_id"], op.as_str());
    assert_eq!(ops[0]["done"], 3);
    assert_eq!(ops[2]["op_id"], put_op.as_str());
    assert_eq!(ops[5]["done"], 4);
    let items: Vec<&Value> = ev
        .iter()
        .filter(|e| e["event"] == "item" && e["op_id"] == op.as_str())
        .collect();
    assert_eq!(items.len(), 3, "{log}");
    assert!(
        items
            .iter()
            .any(|i| i["path"] == "~/Library/Logs/old-app" && i["outcome"] == "done"),
        "{log}"
    );
    for cmd in ["scan", "clean", "undo", "trash", "free"] {
        assert!(
            ev.iter()
                .any(|e| e["event"] == "end" && e["command"] == cmd && e["exit_code"] == 0),
            "falta el final de {cmd}:\n{log}"
        );
    }
    // RNF-33: la carpeta personal (con el nombre de usuario) no aparece.
    let home = fx.home.to_str().expect("UTF-8");
    assert!(!log.contains(home), "{log}");
}

#[test]
fn debug_goes_to_stderr_and_never_to_stdout() {
    let fx = fixture();
    let out = fx
        .cmd()
        .args(["--debug", "scan", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    // stdout es un único documento JSON válido.
    let doc: Value = serde_json::from_slice(&out.stdout).expect("stdout es JSON");
    assert_eq!(doc["command"], "scan");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let ev = events(&stderr);
    assert!(ev.iter().any(|e| e["event"] == "start"), "{stderr}");
    assert!(ev.iter().any(|e| e["level"] == "debug"), "{stderr}");

    // Para personas, tampoco: el log solo va a stderr.
    let out = fx
        .cmd()
        .env("ROTTEN_HUMAN", "1")
        .args(["--debug", "scan"])
        .output()
        .expect("ejecutar");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("\"event\""), "{stdout}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("\"event\":\"end\""));

    // Sin `--debug`, nada en stderr y sin el nivel `debug` en el fichero.
    let fx = fixture();
    let out = fx
        .cmd()
        .args(["scan", "--json"])
        .output()
        .expect("ejecutar");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let log = read_logs(&fx);
    let ev = events(&log);
    assert!(ev.iter().any(|e| e["event"] == "start"), "{log}");
    assert!(ev.iter().all(|e| e["level"] != "debug"), "{log}");
}

#[test]
fn levels_are_configurable() {
    let fx = fixture();
    let out = fx
        .cmd()
        .env("ROTTEN_LOG", "off")
        .args(["scan", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(read_logs(&fx), "", "ROTTEN_LOG=off no escribe nada");

    // `warn`: los errores sí, el inicio y el final no.
    let out = fx
        .cmd()
        .env("ROTTEN_LOG", "warn")
        .args(["undo", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(1));
    let ev = events(&read_logs(&fx));
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(ev[0]["event"], "error");
    assert_eq!(ev[0]["code"], "nothing_to_undo");
    assert_eq!(ev[0]["exit_code"], 1);
}

/// Los errores de los analizadores citan el fichero en las líneas
/// siguientes: del mensaje solo va la primera.
#[test]
fn errors_are_logged_without_quoting_files() {
    let fx = fixture();
    fs::write(
        fx.state.join("config.toml"),
        // El error de TOML cita la línea 2, con el marcador.
        format!("[exclusions]\npaths = [1, \"{MARKER}\"]\n"),
    )
    .expect("write");
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 5);
    assert_eq!(doc["error"]["code"], "config_corrupt");
    let log = read_logs(&fx);
    assert!(!log.contains(MARKER), "{log}");
    let ev = events(&log);
    let err = ev.iter().find(|e| e["event"] == "error").expect("error");
    assert_eq!(err["code"], "config_corrupt");
    assert_eq!(err["exit_code"], 5);
    assert_eq!(err["level"], "error");
}

/// El log nunca hace fallar una operación: si no se puede escribir, se
/// sigue sin él.
#[test]
fn an_unwritable_log_never_fails_an_operation() {
    // La carpeta del log es un fichero.
    let fx = fixture();
    fs::remove_dir(fx.log_dir()).expect("rmdir");
    fs::write(fx.log_dir(), b"").expect("write");
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 3);
    let (code, doc) = fx.run(&["undo", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    // Con `--debug`, la copia a stderr sigue funcionando.
    let out = fx
        .cmd()
        .args(["--debug", "scan", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains("\"event\":\"end\""));

    // Sin permiso de escritura en la carpeta del log.
    let fx = fixture();
    fs::set_permissions(fx.log_dir(), fs::Permissions::from_mode(0o500)).expect("chmod");
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    fs::set_permissions(fx.log_dir(), fs::Permissions::from_mode(0o700)).expect("chmod");
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["report"]["done"], 3);
    assert_eq!(read_logs(&fx), "");
}

/// El log se crea solo, con permisos del usuario, aunque falte
/// `~/Library/Logs`.
#[test]
fn the_log_folder_is_created_private() {
    let fx = fixture();
    fs::remove_dir_all(fx.p("Library/Logs")).expect("rm");
    let (code, _) = fx.run(&["history", "--json"]);
    assert_eq!(code, 0);
    let mode = |p: &Path| fs::metadata(p).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode(&fx.log_dir()), 0o700);
    assert_eq!(mode(&fx.log_dir().join("rotten.log")), 0o600);
    assert!(read_logs(&fx).contains("\"command\":\"history\""));
}

/// CAT-03 limpia `~/Library/Logs/*`, pero nunca el log de Rotten Apple,
/// aunque sea antiguo.
#[test]
fn rotten_apple_never_proposes_its_own_log() {
    assert_eq!(
        Path::new(LOG_DIR).file_name().and_then(|n| n.to_str()),
        Some(ra_core::BUNDLE_ID),
        "el bundle ID ha cambiado: actualiza LOG_DIR y la exclusión de cat-03-user-logs.toml"
    );
    let fx = fixture();
    write_old(&fx, &format!("{LOG_DIR}/rotten.log.1"), "{}\n", 60);
    let when = SystemTime::now() - Duration::from_secs(60 * 86_400);
    File::open(fx.log_dir())
        .expect("open")
        .set_modified(when)
        .expect("mtime");
    let (code, doc) = fx.run(&[
        "clean",
        "--dry-run",
        "--include-review",
        "--category",
        "CAT-03",
        "--json",
    ]);
    assert_eq!(code, 0, "{doc:#}");
    let text = doc.to_string();
    assert!(!text.contains(ra_core::BUNDLE_ID), "{doc:#}");
    // Y sigue proponiendo los demás registros.
    assert!(text.contains("old-app"), "{doc:#}");
}

/// Ejecuta y devuelve el código; con `timeout`, mata el proceso si no
/// termina antes (y falla).
fn run_bounded(fx: &Fx, args: &[&str], timeout: Duration) -> i32 {
    let bin = env!("CARGO_BIN_EXE_rotten");
    let mut child = std::process::Command::new(bin)
        .args(args)
        .env("ROTTEN_HOME", &fx.home)
        .env("ROTTEN_TRASH_DIR", &fx.trash)
        .env_remove("ROTTEN_STATE_DIR")
        .env_remove("ROTTEN_LOG")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("lanzar");
    let start = std::time::Instant::now();
    loop {
        if let Some(st) = child.try_wait().expect("esperar") {
            return st.code().expect("código");
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{args:?} sigue sin terminar tras {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// M2: `rotten.log` es un enlace a un documento: no se escribe en él (ni
/// se rota como si fuera el log) y el comando sigue igual.
#[test]
fn the_log_never_writes_through_a_symlink() {
    let fx = fixture();
    let victim = fx.p("Documents/victim.txt");
    fs::write(&victim, "precious").expect("write");
    std::os::unix::fs::symlink(&victim, fx.log_dir().join("rotten.log")).expect("ln -s");
    for args in [
        &["history", "--json"][..],
        &["--debug", "clean", "--yes", "--json"][..],
    ] {
        assert_eq!(
            run_bounded(&fx, args, Duration::from_secs(30)),
            0,
            "{args:?}"
        );
    }
    assert_eq!(fs::read_to_string(&victim).expect("leer"), "precious");
    assert!(
        fs::symlink_metadata(fx.log_dir().join("rotten.log"))
            .expect("lstat")
            .file_type()
            .is_symlink()
    );
    // La carpeta del log es un enlace a otra: tampoco.
    let fx = fixture();
    let elsewhere = fx.p("Documents/otra");
    fs::create_dir(&elsewhere).expect("mkdir");
    fs::remove_dir(fx.log_dir()).expect("rmdir");
    std::os::unix::fs::symlink(&elsewhere, fx.log_dir()).expect("ln -s");
    assert_eq!(
        run_bounded(&fx, &["history", "--json"], Duration::from_secs(30)),
        0
    );
    assert_eq!(fs::read_dir(&elsewhere).expect("leer").count(), 0);
}

/// M2: un FIFO en `rotten.log` no bloquea el comando.
#[test]
fn a_fifo_in_place_of_the_log_does_not_block() {
    let fx = fixture();
    let fifo = fx.log_dir().join("rotten.log");
    let st = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");
    assert!(st.success());
    let code = run_bounded(&fx, &["history", "--json"], Duration::from_secs(10));
    assert_eq!(code, 0);
}

/// M3: lo que teclea el usuario no llega al log; del error, el código y,
/// en `config`, la clave si es una de las conocidas.
#[test]
fn typed_values_never_reach_the_log() {
    let fx = fixture();
    let typed = |rest: &str| format!("{MARKER}-{rest}");
    let steps: Vec<(Vec<String>, i32, &str)> = vec![
        (
            vec![
                "config".into(),
                "set".into(),
                "language".into(),
                typed("hunter2"),
            ],
            2,
            "invalid_config_value",
        ),
        (
            vec!["config".into(), "get".into(), typed("clave")],
            2,
            "unknown_config_key",
        ),
        (vec!["undo".into(), typed("op")], 1, "unknown_operation"),
        (
            vec!["scan".into(), "--category".into(), typed("cat")],
            2,
            "unknown_category",
        ),
        (
            vec!["exclude".into(), "add".into(), typed("relativa/ruta")],
            2,
            "invalid_exclusion",
        ),
    ];
    for (args, exit, code) in &steps {
        let mut a: Vec<&str> = args.iter().map(String::as_str).collect();
        a.push("--json");
        let (got, doc) = fx.run(&a);
        assert_eq!(got, *exit, "{a:?}: {doc:#}");
        assert_eq!(doc["error"]["code"], *code, "{a:?}: {doc:#}");
    }
    let log = read_logs(&fx);
    assert!(!log.contains(MARKER), "{log}");
    let errors: Vec<Value> = events(&log)
        .into_iter()
        .filter(|e| e["event"] == "error")
        .collect();
    let codes: Vec<&str> = errors
        .iter()
        .map(|e| e["code"].as_str().expect("code"))
        .collect();
    assert_eq!(
        codes,
        steps.iter().map(|s| s.2).collect::<Vec<_>>(),
        "{log}"
    );
    assert_eq!(errors[0]["key"], "language", "{log}");
    assert_eq!(errors[1]["key"], "(unknown)", "{log}");
    assert!(errors.iter().all(|e| e.get("message").is_none()), "{log}");
}

/// M4: la carpeta del log está protegida, también por selección explícita
/// (y su carpeta padre, que la contiene).
#[test]
fn the_log_folder_cannot_be_trashed() {
    let fx = fixture();
    write_old(&fx, &format!("{LOG_DIR}/rotten.log.1"), "{}\n", 60);
    let log_dir = fx.log_dir();
    let logs = fx.p("Library/Logs");
    for target in [log_dir.clone(), log_dir.join("rotten.log.1"), logs] {
        let t = target.to_str().expect("UTF-8");
        for extra in [&["--dry-run"][..], &["--yes"][..]] {
            let mut args = vec!["trash", "put", t, "--json"];
            args.extend_from_slice(extra);
            let (code, doc) = fx.run(&args);
            assert_eq!(code, 5, "{args:?}: {doc:#}");
            assert_eq!(doc["error"]["code"], "selection_protected", "{doc:#}");
        }
    }
    assert!(log_dir.join("rotten.log.1").exists());
    assert_eq!(fs::read_dir(&fx.trash).expect("Papelera").count(), 0);
}
