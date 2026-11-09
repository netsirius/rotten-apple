//! `rotten update` (T-15): detección de la vía de instalación desde el
//! binario, delegación en `brew upgrade` y que nunca abre conexiones.
//!
//! La detección en sí (cada caso de ruta y de entorno) está en las pruebas
//! unitarias de `src/cmd/update.rs`. Aquí se prueba el binario: con
//! instalaciones simuladas (`ROTTEN_UPDATE_EXE`) y un `brew` falso que apunta
//! sus argumentos, y con el ejecutable real sin sustituir.
//!
//! Sin red, en tres capas:
//! - `Cargo.lock`: ningún crate de red en el árbol de dependencias de
//!   `rotten`.
//! - El binario enlazado no importa `socket`, `connect` ni la resolución de
//!   nombres, ni enlaza `CFNetwork` o `Network.framework`.
//! - En ejecución: con `PATH` lleno de trampas (`curl`, `wget`, `git`, `nc`,
//!   `ssh`, `brew`) que dejan una marca si se ejecutan, `update` no lanza
//!   ninguna, y `--dry-run` tampoco el `brew` del prefijo.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::{Fx, Install, fixture};
use serde_json::Value;

/// Programas que podrían abrir una conexión. Cada trampa deja
/// `<nombre>.ran` en su carpeta.
const TRAPS: &[&str] = &["curl", "wget", "git", "nc", "ssh", "brew"];

/// Pone las trampas delante en `PATH` y devuelve su carpeta.
fn set_traps(fx: &mut Fx) -> PathBuf {
    let dir = fx.p("traps");
    fs::create_dir_all(&dir).expect("mkdir");
    for t in TRAPS {
        let p = dir.join(t);
        fs::write(&p, "#!/bin/sh\ntouch \"$0.ran\"\nexit 0\n").expect("trampa");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let path = format!("{}:/usr/bin:/bin", dir.display());
    fx.env.push(("PATH".into(), path));
    dir
}

fn sprung(traps: &Path) -> Vec<String> {
    TRAPS
        .iter()
        .filter(|t| traps.join(format!("{t}.ran")).exists())
        .map(|t| (*t).to_owned())
        .collect()
}

fn brew_calls(fx: &Fx) -> Vec<String> {
    fs::read_to_string(fx.brew_log())
        .map(|s| s.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn human(fx: &Fx, args: &[&str]) -> Output {
    fx.cmd()
        .env("ROTTEN_HUMAN", "1")
        .args(args)
        .output()
        .expect("ejecutar")
}

/// Aceptación de T-15: `rotten update --dry-run --json` informa del método y
/// del comando exacto, sin ejecutar nada. En todas las vías.
#[test]
fn dry_run_reports_method_and_exact_command_and_runs_nothing() {
    let mut fx = fixture();
    let traps = set_traps(&mut fx);
    let prefix = fx.brew_prefix();
    let brew = prefix.join("bin/brew");
    let cases = [
        (
            Install::HomebrewLink,
            "homebrew",
            Some(vec![
                brew.to_str().expect("UTF-8").to_owned(),
                "upgrade".into(),
                "netsirius/tap/rotten".into(),
            ]),
        ),
        (
            Install::Homebrew,
            "homebrew",
            Some(vec![
                brew.to_str().expect("UTF-8").to_owned(),
                "upgrade".into(),
                "netsirius/tap/rotten".into(),
            ]),
        ),
        (Install::App, "app", None),
        (
            Install::Cargo,
            "cargo",
            Some(
                [
                    "cargo",
                    "install",
                    "--locked",
                    "--git",
                    "https://github.com/netsirius/rotten-apple",
                    "rotten",
                ]
                .map(String::from)
                .to_vec(),
            ),
        ),
        (Install::Other, "other", None),
    ];
    for (kind, method, argv) in cases {
        let exe = fx.use_install(kind);
        let (code, doc) = fx.run(&["update", "--dry-run", "--json"]);
        assert_eq!(code, 0, "{kind:?}: {doc:#}");
        assert_eq!(doc["dry_run"], true);
        assert_eq!(doc["method"], method, "{kind:?}");
        assert_eq!(doc["ran"], false);
        assert_eq!(doc["automatic"], method == "homebrew");
        let want = argv.map_or(Value::Null, Value::from);
        assert_eq!(doc["argv"], want, "{kind:?}");
        // La ruta resuelta: el enlace de `bin/` lleva al Cellar.
        let resolved = fs::canonicalize(&exe).expect("canon");
        assert_eq!(doc["executable"], resolved.to_str().expect("UTF-8"));
        if kind == Install::HomebrewLink {
            assert!(resolved.starts_with(prefix.join("Cellar")), "{resolved:?}");
        }
        // Y sin `--json`, el comando para personas.
        let out = human(&fx, &["update", "--dry-run"]);
        assert_eq!(out.status.code(), Some(0));
        let text = String::from_utf8_lossy(&out.stdout);
        if method == "homebrew" {
            // La ruta entera de `brew`: se puede copiar tal cual.
            let want = format!("Would run: {} upgrade netsirius/tap/rotten", brew.display());
            assert!(text.contains(&want), "{text}");
        }
        assert!(text.contains("Dry run: nothing was changed."), "{text}");
    }
    assert_eq!(
        brew_calls(&fx),
        Vec::<String>::new(),
        "--dry-run lanzó brew"
    );
    assert_eq!(sprung(&traps), Vec::<String>::new());
}

/// Con Homebrew, sin `--dry-run`: confirmación (o `--yes`), el `brew` del
/// prefijo del Cellar con `upgrade <tap>/<fórmula>`, y su resultado.
#[test]
fn homebrew_runs_brew_upgrade_after_confirmation() {
    let mut fx = fixture();
    let traps = set_traps(&mut fx);
    fx.use_install(Install::HomebrewLink);

    // Sin terminal y sin `--yes`: error de uso y brew sin tocar (07 §1.4).
    let (code, doc) = fx.run(&["update", "--json"]);
    assert_eq!((code, &doc["error"]["code"]), (2, &"needs_yes".into()));
    let out = human(&fx, &["update"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(brew_calls(&fx).is_empty());

    // Con `--yes`. Con `--json`, lo que brew escribe en stdout va a stderr:
    // stdout es solo el documento JSON (que `Fx::run` analiza y valida).
    fx.env
        .push(("FAKE_BREW_SAY".into(), "==> Upgrading rotten".into()));
    let out = fx
        .cmd()
        .args(["update", "--yes", "--json"])
        .output()
        .expect("ejecutar");
    assert_eq!(out.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&out.stdout).expect("solo JSON en stdout");
    assert_eq!(
        (doc["ran"].clone(), doc["dry_run"].clone()),
        (true.into(), false.into())
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("==> Upgrading rotten"));
    assert_eq!(brew_calls(&fx), ["upgrade netsirius/tap/rotten"]);

    // Para personas, brew hereda la terminal: su salida sale tal cual.
    let out = human(&fx, &["update", "--yes"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("==> Upgrading rotten"), "{text}");
    assert!(text.contains("Homebrew has finished."), "{text}");
    assert_eq!(brew_calls(&fx).len(), 2);

    // brew falla: `1` (07 §5) y su código en el mensaje.
    fx.env.push(("FAKE_BREW_EXIT".into(), "3".into()));
    let (code, doc) = fx.run(&["update", "--yes", "--json"]);
    assert_eq!(code, 1, "{doc:#}");
    assert_eq!(doc["error"]["code"], "brew_failed");
    assert_eq!(doc["error"]["message"], "brew upgrade exited with code 3");
    let out = human(&fx, &["update", "--yes", "--lang", "es"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("terminó con el código 3"),
        "{out:?}"
    );

    // Sin `brew` en el prefijo: `1`, también con `--dry-run`.
    fs::remove_file(fx.brew_prefix().join("bin/brew")).expect("rm");
    for args in [
        ["update", "--yes", "--json"],
        ["update", "--dry-run", "--json"],
    ] {
        let (code, doc) = fx.run(&args);
        assert_eq!((code, &doc["error"]["code"]), (1, &"brew_not_found".into()));
    }
    // El `brew` de `PATH` (una trampa) nunca se usa: va el del prefijo.
    assert_eq!(sprung(&traps), Vec::<String>::new());
}

/// Sin acción automática: se explica cómo actualizar, con `0`, y no se lanza
/// nada (ni siquiera con `--yes`).
#[test]
fn manual_methods_explain_and_exit_zero() {
    let mut fx = fixture();
    let traps = set_traps(&mut fx);
    let cases = [
        (
            Install::App,
            vec![
                "Installed inside the app ~/Applications/Rotten Apple.app.",
                "Sparkle, which is not available yet",
            ],
        ),
        (
            Install::Cargo,
            vec![
                "Installed with cargo install (~/.cargo/bin).",
                "To update, run: cargo install --locked --git https://github.com/netsirius/rotten-apple rotten",
            ],
        ),
        (
            Install::Other,
            vec![
                "Unknown how ~/bin/rotten was installed.",
                "brew install netsirius/tap/rotten",
            ],
        ),
    ];
    for (kind, lines) in cases {
        fx.use_install(kind);
        for args in [&["update"][..], &["update", "--yes"]] {
            let out = human(&fx, args);
            assert_eq!(out.status.code(), Some(0), "{kind:?} {args:?}: {out:?}");
            let text = String::from_utf8_lossy(&out.stdout);
            for l in &lines {
                assert!(text.contains(l), "{kind:?}: falta «{l}» en\n{text}");
            }
        }
        let (code, doc) = fx.run(&["update", "--yes", "--json"]);
        assert_eq!((code, &doc["ran"]), (0, &false.into()), "{doc:#}");
    }
    assert!(brew_calls(&fx).is_empty());
    assert_eq!(sprung(&traps), Vec::<String>::new());
}

/// Sin `ROTTEN_UPDATE_EXE`, la ruta es la de `current_exe()` resuelta. El
/// binario real enlazado en un Cellar y llamado por el enlace de `bin/`,
/// como lo deja Homebrew, se reconoce como Homebrew.
#[test]
fn real_executable_is_detected_from_its_resolved_path() {
    let fx = fixture();
    let real = fs::canonicalize(assert_cmd::cargo::cargo_bin("rotten")).expect("binario");
    let (code, doc) = fx.run(&["update", "--dry-run", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert_eq!(doc["executable"], real.to_str().expect("UTF-8"));

    // Una copia del binario real en un Cellar de prueba.
    let prefix = fx.brew_prefix();
    let cellar = prefix.join("Cellar/rotten/0.1.0/bin");
    fs::create_dir_all(&cellar).expect("mkdir");
    fs::create_dir_all(prefix.join("bin")).expect("mkdir");
    let copy = cellar.join("rotten");
    if fs::hard_link(&real, &copy).is_err() {
        fs::copy(&real, &copy).expect("copiar el binario");
    }
    let link = prefix.join("bin/rotten");
    std::os::unix::fs::symlink("../Cellar/rotten/0.1.0/bin/rotten", &link).expect("enlace");
    let out = assert_cmd::Command::new(&link)
        .env("ROTTEN_HOME", &fx.home)
        .env("HOMEBREW_PREFIX", &prefix)
        .env_remove("ROTTEN_UPDATE_EXE")
        .args(["update", "--dry-run", "--json"])
        .output()
        .expect("ejecutar");
    // Sin `brew` en ese prefijo: `brew_not_found`, que ya dice que lo ha
    // reconocido como Homebrew y dónde ha buscado `brew`.
    let doc: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(out.status.code(), Some(1), "{doc:#}");
    assert_eq!(doc["error"]["code"], "brew_not_found");
    let brew = prefix.join("bin/brew");
    let want = format!("brew not found at {}", brew.display());
    assert_eq!(doc["error"]["message"], want.as_str());
}

/// Crates que abren conexiones (clientes HTTP, TLS, *runtimes* asíncronos
/// con red). Si alguno entra en el árbol de `rotten`, hay que decidirlo a
/// propósito (`vulns --online` de v1, 07 §3) y cambiar esta prueba.
const NETWORK_CRATES: &[&str] = &[
    "reqwest",
    "hyper",
    "h2",
    "ureq",
    "curl",
    "curl-sys",
    "isahc",
    "attohttpc",
    "surf",
    "rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "security-framework",
    "tokio",
    "async-std",
    "mio",
    "socket2",
    "tungstenite",
    "trust-dns-resolver",
    "hickory-resolver",
];

/// Sin red (1/3): ningún crate de red en el árbol de dependencias de
/// `rotten` según `Cargo.lock` (incluye las de desarrollo: más estricto).
#[test]
fn no_network_crates_in_the_dependency_tree() {
    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let text = fs::read_to_string(&lock).expect("Cargo.lock");
    let doc: toml::Value = toml::from_str(&text).expect("Cargo.lock válido");
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in doc["package"].as_array().expect("package") {
        let name = p["name"].as_str().expect("name").to_owned();
        let list = p
            .get("dependencies")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str())
            // "nombre" o "nombre versión" (si hay varias versiones).
            .map(|d| d.split(' ').next().expect("nombre").to_owned());
        deps.entry(name).or_default().extend(list);
    }
    let mut seen = BTreeSet::new();
    let mut stack = vec!["rotten".to_owned()];
    while let Some(n) = stack.pop() {
        if seen.insert(n.clone()) {
            stack.extend(deps.get(&n).into_iter().flatten().cloned());
        }
    }
    assert!(seen.len() > 20, "árbol sospechosamente pequeño: {seen:?}");
    let found: Vec<&str> = NETWORK_CRATES
        .iter()
        .copied()
        .filter(|c| seen.contains(*c))
        .collect();
    assert!(
        found.is_empty(),
        "crates de red en el árbol de rotten: {found:?}"
    );
}

/// Herramienta de Xcode (`nm`, `otool`): su stdout. En el sandbox avisan en
/// stderr de que no pueden crear su caché; no importa.
fn xcode_tool(tool: &str, args: &[&str]) -> String {
    let out = std::process::Command::new(tool)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{tool}: {e} (hacen falta las Command Line Tools)"));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success() && !stdout.trim().is_empty(),
        "{tool} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// Sin red (2/3): el binario enlazado no puede abrir una conexión por sí
/// mismo. No importa ninguna función de sockets ni de resolución de
/// nombres, no enlaza los *frameworks* de red y no nombra las clases de red
/// de Foundation. Afecta a todo `rotten`: si un comando necesita red algún
/// día (`vulns --online`, v1), esta prueba debe cambiar a propósito.
#[test]
fn binary_imports_no_network_functions() {
    let bin = assert_cmd::cargo::cargo_bin("rotten");
    let bin = bin.to_str().expect("UTF-8");
    let imports = xcode_tool("nm", &["-u", bin]);
    let symbols: BTreeSet<&str> = imports.lines().map(str::trim).collect();
    assert!(
        symbols.contains("_open"),
        "nm no da lo esperado:\n{imports}"
    );
    let forbidden = [
        "_socket",
        "_connect",
        "_connectx",
        "_sendto",
        "_sendmsg",
        "_getaddrinfo",
        "_gethostbyname",
        "_gethostbyname2",
        "_res_query",
        "_DNSServiceCreateConnection",
    ];
    let found: Vec<&str> = forbidden
        .into_iter()
        .filter(|s| symbols.contains(s))
        .collect();
    assert!(
        found.is_empty(),
        "rotten importa funciones de red: {found:?}"
    );

    let libs = xcode_tool("otool", &["-L", bin]);
    for l in libs.lines().skip(1) {
        let lib = l.trim().split(" (").next().expect("ruta");
        let allowed = [
            "/System/Library/Frameworks/Foundation.framework/",
            "/System/Library/Frameworks/CoreFoundation.framework/",
            "/usr/lib/libobjc.A.dylib",
            "/usr/lib/libiconv.2.dylib",
            "/usr/lib/libSystem.B.dylib",
        ];
        assert!(
            allowed.iter().any(|a| lib.starts_with(a)),
            "rotten enlaza {lib}, que no estaba revisada (¿red?)"
        );
    }

    let bytes = fs::read(bin).expect("leer el binario");
    for class in [
        "NSURLSession",
        "NSURLConnection",
        "NSURLDownload",
        "NSStream",
        "CFSocket",
        "nw_connection",
    ] {
        assert!(
            !bytes.windows(class.len()).any(|w| w == class.as_bytes()),
            "el binario nombra {class}"
        );
    }
}
