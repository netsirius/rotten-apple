//! `rotten update` (T-15, 07 §2): actualiza Rotten Apple por la vía con la
//! que se instaló, sin abrir nunca una conexión por su cuenta (ni para ver
//! si hay versión nueva ni para descargarla).
//!
//! - Binario en el Cellar de Homebrew: delega en `brew upgrade` del *tap*,
//!   con el `brew` de ese mismo prefijo. Es la única acción automática;
//!   pide confirmación (o `--yes`) porque cambia el sistema, y la descarga
//!   la hace Homebrew.
//! - Dentro de un `.app`: la app se actualizará con Sparkle (H-7, T-41);
//!   hasta entonces, se explica.
//! - Instalado con `cargo install` o en cualquier otra ruta: se explica
//!   cómo actualizar.
//!
//! La vía se decide sobre la ruta real del ejecutable (`current_exe`, con
//! los enlaces resueltos) con [`detect`], que es pura y tiene pruebas para
//! cada caso. `ROTTEN_UPDATE_EXE` sustituye a `current_exe` en las pruebas
//! del binario, como `ROTTEN_HOME` a la carpeta personal.
//!
//! Códigos: `0` hecho, nada que hacer automáticamente (se explica) o
//! cancelado · `1` no se encuentra `brew`, no se puede lanzar o termina con
//! error (su código va en el mensaje) · `2` falta `--yes` sin terminal. Con
//! `--dry-run`, el código que tendría la ejecución, sin lanzar nada.

use std::ffi::OsStr;
use std::io;
use std::os::fd::AsFd;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use crate::i18n::{I18n, Lang, tr};
use crate::{Exit, Ui};

const COMMAND: &str = "update";

// ─── PENDIENTE DEL AUTOR (T-39) ─────────────────────────────────────────
// El *tap*, la fórmula y el repositorio aún no existen. Son los nombres
// provisionales de 08 §4.8 (T-39: `brew install netsirius/tap/rotten`) y
// del `$id` de los esquemas. El nombre del binario también es provisional
// (07, D-08). Cambiarlos aquí cambia la salida de `rotten update`.
/// *Tap* de Homebrew con la fórmula del CLI (provisional).
pub const HOMEBREW_TAP: &str = "netsirius/tap";
/// Fórmula del CLI en ese *tap* (provisional).
pub const HOMEBREW_FORMULA: &str = "rotten";
/// Repositorio para `cargo install --git` (el workspace no se publica en
/// crates.io).
pub const REPO_URL: &str = "https://github.com/netsirius/rotten-apple";
/// Paquete del CLI dentro del repositorio.
pub const CARGO_PACKAGE: &str = "rotten";
// ────────────────────────────────────────────────────────────────────────

/// Sustituye a `current_exe()` (pruebas del binario).
pub const EXE_ENV: &str = "ROTTEN_UPDATE_EXE";

/// Prefijos de Homebrew por defecto (Apple silicon e Intel), además del de
/// `HOMEBREW_PREFIX`.
const DEFAULT_BREW_PREFIXES: &[&str] = &["/opt/homebrew", "/usr/local"];

/// Cómo se instaló el binario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// En `<prefix>/Cellar/<formula>/<versión>/…`.
    Homebrew { prefix: PathBuf, formula: String },
    /// Dentro del bundle `app` (`…/X.app/Contents/…`).
    App { app: PathBuf },
    /// En `bin`, la carpeta de `cargo install` (`$CARGO_HOME/bin` o
    /// `~/.cargo/bin`).
    Cargo { bin: PathBuf },
    /// Cualquier otra ruta.
    Other,
}

impl Method {
    fn code(&self) -> &'static str {
        match self {
            Method::Homebrew { .. } => "homebrew",
            Method::App { .. } => "app",
            Method::Cargo { .. } => "cargo",
            Method::Other => "other",
        }
    }

    /// El comando exacto: el que lanza `rotten update` (Homebrew) o el que
    /// se aconseja ejecutar (cargo). `None` si no hay un comando que valga.
    pub fn argv(&self) -> Option<Vec<String>> {
        match self {
            Method::Homebrew { prefix, formula } => Some(vec![
                brew_path(prefix).to_string_lossy().into_owned(),
                "upgrade".into(),
                qualified_formula(formula),
            ]),
            Method::Cargo { .. } => Some(
                [
                    "cargo",
                    "install",
                    "--locked",
                    "--git",
                    REPO_URL,
                    CARGO_PACKAGE,
                ]
                .map(String::from)
                .to_vec(),
            ),
            Method::App { .. } | Method::Other => None,
        }
    }

    /// Si `rotten update` lanza el comando por sí mismo.
    fn automatic(&self) -> bool {
        matches!(self, Method::Homebrew { .. })
    }
}

/// El `brew` del prefijo que contiene el Cellar (no el primero de `PATH`).
fn brew_path(prefix: &Path) -> PathBuf {
    prefix.join("bin/brew")
}

/// `<tap>/<fórmula>` si es la nuestra; si el Cellar tiene otro nombre (otra
/// fórmula u otro *tap*), ese nombre tal cual y que Homebrew lo resuelva.
fn qualified_formula(formula: &str) -> String {
    if formula == HOMEBREW_FORMULA {
        format!("{HOMEBREW_TAP}/{HOMEBREW_FORMULA}")
    } else {
        formula.to_owned()
    }
}

/// La componente que sigue a `base` en `exe`, si `exe` está debajo de
/// `base` y hay algo más después (la fórmula y su versión).
fn formula_under(exe: &Path, cellar: &Path) -> Option<String> {
    let mut rest = exe.strip_prefix(cellar).ok()?.components();
    match (rest.next(), rest.next()) {
        (Some(Component::Normal(f)), Some(_)) => Some(f.to_string_lossy().into_owned()),
        _ => None,
    }
}

/// Decide la vía de instalación a partir de la ruta **ya resuelta** del
/// ejecutable y de las variables de entorno (`env`: `HOMEBREW_PREFIX`,
/// `CARGO_HOME`, `HOME`). Pura: no lee el disco.
pub fn detect(exe: &Path, env: impl Fn(&str) -> Option<String>) -> Method {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);

    // Homebrew: el prefijo de `HOMEBREW_PREFIX` o uno de los de siempre.
    let prefixes = var("HOMEBREW_PREFIX")
        .into_iter()
        .chain(DEFAULT_BREW_PREFIXES.iter().map(PathBuf::from));
    for prefix in prefixes {
        if let Some(formula) = formula_under(exe, &prefix.join("Cellar")) {
            return Method::Homebrew { prefix, formula };
        }
    }

    // Dentro de un bundle: `…/X.app/Contents/…`.
    let mut bundle = PathBuf::new();
    let mut parts = exe.components().peekable();
    while let Some(c) = parts.next() {
        bundle.push(c);
        let is_app = Path::new(c.as_os_str())
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"));
        if is_app && parts.peek() == Some(&Component::Normal(OsStr::new("Contents"))) {
            return Method::App { app: bundle };
        }
    }

    // `cargo install`: `$CARGO_HOME/bin` o `~/.cargo/bin`.
    let cargo_home = var("CARGO_HOME").or_else(|| var("HOME").map(|h| h.join(".cargo")));
    if let Some(bin) = cargo_home.map(|c| c.join("bin"))
        && exe.parent() == Some(bin.as_path())
    {
        return Method::Cargo { bin };
    }

    Method::Other
}

/// La ruta del ejecutable con los enlaces resueltos (o tal cual, si no se
/// puede resolver).
fn resolved_exe() -> io::Result<PathBuf> {
    let exe = match std::env::var_os(EXE_ENV).filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe()?,
    };
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// Un comando para copiar en la terminal: cada argumento entre comillas
/// simples si lleva algo que la shell interpretaría.
fn shell_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            let plain = !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:@=+,".contains(c));
            if plain {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn emit(ui: &Ui, method: &Method, exe: &Path, dry_run: bool, ran: bool) {
    ui.emit(
        COMMAND,
        json!({
            "dry_run": dry_run,
            "method": method.code(),
            "executable": exe,
            "automatic": method.automatic(),
            "argv": method.argv(),
            "ran": ran,
        }),
    );
}

/// Lo que hay que hacer cuando `rotten update` no lanza nada por sí mismo.
fn explain(ui: &Ui, method: &Method, exe: &Path) {
    match method {
        Method::App { app } => {
            ui.say(tr!(ui, "update-app", app = ui.path(app)));
            ui.say(tr!(ui, "update-app-howto"));
        }
        Method::Cargo { bin } => {
            ui.say(tr!(ui, "update-cargo", dir = ui.path(bin)));
            let line = shell_line(&method.argv().unwrap_or_default());
            ui.say(tr!(ui, "update-run-yourself", command = line));
        }
        Method::Other => {
            ui.say(tr!(ui, "update-other", exe = ui.path(exe)));
            let install = format!("brew install {HOMEBREW_TAP}/{HOMEBREW_FORMULA}");
            ui.say(tr!(ui, "update-other-brew", command = install));
            ui.say(tr!(ui, "update-other-source"));
        }
        Method::Homebrew { .. } => {}
    }
}

/// Lanza `brew` heredando la terminal. Con `--json`, la salida de `brew` va
/// a stderr para que stdout sea solo el documento JSON.
fn run_brew(ui: &Ui, argv: &[String]) -> io::Result<std::process::ExitStatus> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    if ui.json {
        cmd.stdout(Stdio::from(io::stderr().as_fd().try_clone_to_owned()?));
    }
    cmd.status()
}

fn brew_failed(ui: &Ui, code: Option<i32>) -> Exit {
    let code = code.map_or_else(|| "signal".to_owned(), |c| c.to_string());
    let english = tr!(
        I18n::with(Lang::En, false),
        "error-update-brew-failed",
        code = code.as_str()
    );
    let human = tr!(ui, "error-update-brew-failed", code = code.as_str());
    ui.fail(COMMAND, Exit::Error, "brew_failed", &english, &human)
}

pub fn run(ui: &Ui, dry_run: bool) -> Exit {
    let exe = match resolved_exe() {
        Ok(p) => p,
        Err(e) => {
            let english = tr!(
                I18n::with(Lang::En, false),
                "error-update-no-exe",
                detail = e.to_string()
            );
            let human = tr!(ui, "error-update-no-exe", detail = e.to_string());
            return ui.fail(COMMAND, Exit::Error, "executable_unknown", &english, &human);
        }
    };
    let method = detect(&exe, |k| std::env::var(k).ok());

    let Method::Homebrew { prefix, .. } = &method else {
        // Nada que lanzar: se explica cómo actualizar y no es un error.
        if ui.json {
            emit(ui, &method, &exe, dry_run, false);
        } else {
            explain(ui, &method, &exe);
            if dry_run {
                ui.say(tr!(ui, "dry-run-nothing-changed"));
            }
        }
        return Exit::Ok;
    };

    let argv = method.argv().unwrap_or_default();
    let line = shell_line(&argv);
    let brew = brew_path(prefix);
    if !brew.is_file() {
        let english = tr!(
            I18n::with(Lang::En, false),
            "error-update-no-brew",
            path = brew.display().to_string()
        );
        let human = tr!(ui, "error-update-no-brew", path = ui.path(&brew));
        return ui.fail(COMMAND, Exit::Error, "brew_not_found", &english, &human);
    }
    if !ui.json {
        ui.say(tr!(ui, "update-homebrew", prefix = ui.path(prefix)));
    }
    if dry_run {
        if ui.json {
            emit(ui, &method, &exe, true, false);
        } else {
            ui.say(tr!(ui, "update-would-run", command = line));
            ui.say(tr!(ui, "dry-run-nothing-changed"));
        }
        return Exit::Ok;
    }
    let question = tr!(ui, "update-confirm", command = line.as_str());
    if let Err(code) = ui.confirm(COMMAND, &question) {
        return code;
    }
    if !ui.json {
        ui.say(tr!(ui, "update-running", command = line.as_str()));
    }
    match run_brew(ui, &argv) {
        Ok(status) if status.success() => {
            if ui.json {
                emit(ui, &method, &exe, false, true);
            } else {
                ui.say(tr!(ui, "update-done"));
            }
            Exit::Ok
        }
        Ok(status) => brew_failed(ui, status.code()),
        Err(e) => {
            let english = tr!(
                I18n::with(Lang::En, false),
                "error-update-brew-spawn",
                command = line.as_str(),
                detail = e.to_string()
            );
            let human = tr!(
                ui,
                "error-update-brew-spawn",
                command = line.as_str(),
                detail = e.to_string()
            );
            ui.fail(COMMAND, Exit::Error, "brew_spawn_failed", &english, &human)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| m.get(k).cloned()
    }

    fn brew(prefix: &str, formula: &str) -> Method {
        Method::Homebrew {
            prefix: prefix.into(),
            formula: formula.into(),
        }
    }

    const HOME: &[(&str, &str)] = &[("HOME", "/Users/ana")];

    #[test]
    fn homebrew_cellar_on_apple_silicon() {
        let exe = Path::new("/opt/homebrew/Cellar/rotten/0.1.0/bin/rotten");
        let m = detect(exe, env(HOME));
        assert_eq!(m, brew("/opt/homebrew", "rotten"));
        assert_eq!(
            m.argv().expect("comando"),
            ["/opt/homebrew/bin/brew", "upgrade", "netsirius/tap/rotten"]
        );
        assert!(m.automatic());
    }

    #[test]
    fn homebrew_cellar_on_intel_prefix() {
        let exe = Path::new("/usr/local/Cellar/rotten/0.2.0_1/bin/rotten");
        assert_eq!(detect(exe, env(HOME)), brew("/usr/local", "rotten"));
    }

    #[test]
    fn homebrew_cellar_under_custom_prefix() {
        let exe = Path::new("/Users/ana/brew/Cellar/rotten/0.1.0/bin/rotten");
        let vars = [
            ("HOME", "/Users/ana"),
            ("HOMEBREW_PREFIX", "/Users/ana/brew"),
        ];
        let m = detect(exe, env(&vars));
        assert_eq!(m, brew("/Users/ana/brew", "rotten"));
        assert_eq!(m.argv().expect("comando")[0], "/Users/ana/brew/bin/brew");
        // Sin `HOMEBREW_PREFIX`, ese Cellar no es de un prefijo conocido.
        assert_eq!(detect(exe, env(HOME)), Method::Other);
    }

    /// `HOMEBREW_PREFIX` solo dice dónde buscar: un binario fuera de su
    /// Cellar no es de Homebrew.
    #[test]
    fn homebrew_prefix_alone_is_not_homebrew() {
        let vars = [("HOME", "/Users/ana"), ("HOMEBREW_PREFIX", "/opt/homebrew")];
        for exe in [
            "/Users/ana/bin/rotten",
            // `bin/rotten` del prefijo es un enlace: aquí llega ya resuelto.
            // Sin resolver no se reconoce.
            "/opt/homebrew/bin/rotten",
            // El Cellar a secas, sin fórmula ni versión.
            "/opt/homebrew/Cellar/rotten",
        ] {
            assert_eq!(detect(Path::new(exe), env(&vars)), Method::Other, "{exe}");
        }
    }

    /// Una fórmula con otro nombre (otro *tap*): se actualiza por su nombre.
    #[test]
    fn homebrew_other_formula_is_upgraded_by_its_own_name() {
        let exe = Path::new("/opt/homebrew/Cellar/rotten-apple/1.0/bin/rotten");
        let m = detect(exe, env(HOME));
        assert_eq!(m, brew("/opt/homebrew", "rotten-apple"));
        assert_eq!(m.argv().expect("comando")[2], "rotten-apple");
    }

    #[test]
    fn inside_an_app_bundle() {
        for (exe, app) in [
            (
                "/Applications/Rotten Apple.app/Contents/MacOS/rotten",
                "/Applications/Rotten Apple.app",
            ),
            (
                "/Users/ana/Applications/Rotten Apple.app/Contents/Resources/bin/rotten",
                "/Users/ana/Applications/Rotten Apple.app",
            ),
            // Extensión en mayúsculas.
            (
                "/Applications/X.APP/Contents/MacOS/rotten",
                "/Applications/X.APP",
            ),
        ] {
            let m = detect(Path::new(exe), env(HOME));
            assert_eq!(m, Method::App { app: app.into() }, "{exe}");
            assert_eq!(m.argv(), None);
            assert!(!m.automatic());
        }
        // Una carpeta que se llama `.app` pero no es un bundle.
        assert_eq!(
            detect(Path::new("/Users/ana/x.app/bin/rotten"), env(HOME)),
            Method::Other
        );
    }

    /// Una app instalada con un cask sigue siendo una app (Sparkle), aunque
    /// esté bajo el prefijo de Homebrew.
    #[test]
    fn app_inside_homebrew_caskroom_is_an_app() {
        let exe = "/opt/homebrew/Caskroom/rotten/1.0/Rotten Apple.app/Contents/MacOS/rotten";
        assert!(matches!(
            detect(Path::new(exe), env(HOME)),
            Method::App { .. }
        ));
    }

    #[test]
    fn cargo_install_in_default_cargo_home() {
        let m = detect(Path::new("/Users/ana/.cargo/bin/rotten"), env(HOME));
        assert_eq!(
            m,
            Method::Cargo {
                bin: "/Users/ana/.cargo/bin".into()
            }
        );
        assert_eq!(
            shell_line(&m.argv().expect("comando")),
            "cargo install --locked --git https://github.com/netsirius/rotten-apple rotten"
        );
        assert!(!m.automatic());
    }

    #[test]
    fn cargo_install_in_custom_cargo_home() {
        let vars = [("HOME", "/Users/ana"), ("CARGO_HOME", "/opt/cargo")];
        let exe = Path::new("/opt/cargo/bin/rotten");
        assert_eq!(
            detect(exe, env(&vars)),
            Method::Cargo {
                bin: "/opt/cargo/bin".into()
            }
        );
        // Con `CARGO_HOME`, `~/.cargo/bin` ya no es la carpeta de cargo.
        assert_eq!(
            detect(Path::new("/Users/ana/.cargo/bin/rotten"), env(&vars)),
            Method::Other
        );
        // Una subcarpeta de `bin` no es `bin`.
        assert_eq!(
            detect(Path::new("/opt/cargo/bin/x/rotten"), env(&vars)),
            Method::Other
        );
    }

    #[test]
    fn anything_else_is_other() {
        for exe in [
            "/Users/ana/src/rotten-apple/target/release/rotten",
            "/usr/local/bin/rotten",
            "/tmp/rotten",
        ] {
            let m = detect(Path::new(exe), env(HOME));
            assert_eq!(m, Method::Other, "{exe}");
            assert_eq!(m.argv(), None);
        }
        // Sin ninguna variable de entorno.
        assert_eq!(
            detect(Path::new("/Users/ana/.cargo/bin/rotten"), env(&[])),
            Method::Other
        );
        // Variables vacías: como si no estuvieran.
        let vars = [("HOME", ""), ("CARGO_HOME", ""), ("HOMEBREW_PREFIX", "")];
        assert_eq!(
            detect(Path::new("/.cargo/bin/rotten"), env(&vars)),
            Method::Other
        );
    }

    #[test]
    fn shell_line_quotes_what_the_shell_would_interpret() {
        let argv = ["/Users/ana b/brew/bin/brew", "upgrade", "it's"].map(String::from);
        assert_eq!(
            shell_line(&argv),
            r"'/Users/ana b/brew/bin/brew' upgrade 'it'\''s'"
        );
    }
}
