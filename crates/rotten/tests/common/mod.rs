//! Lo que comparten las pruebas de integración del CLI (`cli.rs`,
//! `schemas.rs`, `parity.rs`): la carpeta personal de prueba y la validación
//! de cada documento JSON contra `schemas/` (T-01).
//!
//! Cada prueba usa una carpeta personal temporal (`ROTTEN_HOME`) y una
//! Papelera temporal (`ROTTEN_TRASH_DIR`): nunca se toca el `~` real ni su
//! Papelera.

// Cada binario de pruebas usa una parte distinta de este módulo.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use assert_cmd::Command;
use ra_catalog::fixture::{Entry, Link, materialize};
use serde_json::{Map, Value, json};

pub struct Fx {
    _tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub trash: PathBuf,
    pub state: PathBuf,
}

fn e(path: &str, size: u64, age_days: u32) -> Entry {
    Entry {
        path: path.into(),
        size,
        age_days,
        dir: false,
    }
}

/// Carpeta personal de prueba con algo de cada categoría y datos que nunca
/// deben tocarse.
pub fn fixture() -> Fx {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");
    let home = root.join("home");
    let trash = root.join("trash");
    fs::create_dir_all(&home).expect("mkdir");
    fs::create_dir_all(&trash).expect("mkdir");
    let entries = vec![
        // CAT-03, preseleccionado.
        e("Library/Logs/old-app/app.log", 3000, 30),
        e("Library/Logs/install.log", 800, 20),
        // CAT-03 demasiado reciente: bloqueado.
        e("Library/Logs/current.log", 10, 0),
        // CAT-10, preseleccionado.
        e(
            "Library/Saved Application State/com.example.closed.savedState/windows.plist",
            500,
            3,
        ),
        // CAT-01: «Seguro» pero sin marcar (lista permitida vacía).
        e("Library/Caches/com.example.editor/data.bin", 9000, 30),
        // Nunca.
        e("Library/Caches/com.apple.bird/state", 10, 30),
        e("Documents/tesis.docx", 100, 30),
        e("Library/Keychains/login.keychain-db", 100, 30),
        e("Library/Preferences/com.example.editor.plist", 100, 30),
    ];
    let links = vec![Link {
        path: "Library/Caches/com.example.link".into(),
        target: "Documents".into(),
    }];
    materialize(&home, &entries, &links, SystemTime::now()).expect("materializar");
    // El estado de la app se crea antes de las instantáneas, que lo ignoran.
    let state = home.join("Library/Application Support/io.github.netsirius.RottenApple");
    fs::create_dir_all(&state).expect("mkdir");
    Fx {
        _tmp: tmp,
        home,
        trash,
        state,
    }
}

impl Fx {
    pub fn cmd(&self) -> Command {
        self.cmd_lang("en_US.UTF-8")
    }

    /// El binario con `LANG` fijado (y sin `LC_ALL` ni `LC_MESSAGES`, que
    /// tienen prioridad).
    pub fn cmd_lang(&self, lang: &str) -> Command {
        let mut c = Command::cargo_bin("rotten").expect("binario rotten");
        c.env("ROTTEN_HOME", &self.home)
            .env("ROTTEN_TRASH_DIR", &self.trash)
            .env_remove("ROTTEN_STATE_DIR")
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .env("LANG", lang);
        c
    }

    /// Ejecuta y devuelve (código, JSON de stdout). El documento se valida
    /// siempre contra su esquema: ninguna prueba ve un JSON que no valide.
    pub fn run(&self, args: &[&str]) -> (i32, Value) {
        self.run_lang("en_US.UTF-8", args)
    }

    /// Como [`Fx::run`], con `LANG` fijado.
    pub fn run_lang(&self, lang: &str, args: &[&str]) -> (i32, Value) {
        let out = self.cmd_lang(lang).args(args).output().expect("ejecutar");
        let code = out.status.code().expect("código");
        let json = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "salida no JSON de {args:?} ({e}):\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        });
        if let Err(errors) = validate(&json) {
            panic!(
                "el JSON de {args:?} no valida contra su esquema:\n  {}\n{json:#}",
                errors.join("\n  ")
            );
        }
        (code, json)
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }
}

/// `schemas/` en la raíz del repositorio.
pub fn schemas_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

/// Esquemas de `schemas/<comando>.v<N>.schema.json`, por (comando, N).
pub fn schema_files() -> BTreeMap<(String, u64), (PathBuf, Value)> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(schemas_dir()).expect("leer schemas/") {
        let path = entry.expect("entrada de schemas/").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("nombre UTF-8")
            .to_owned();
        let Some(stem) = name.strip_suffix(".schema.json") else {
            continue;
        };
        let (command, version) = stem
            .rsplit_once(".v")
            .unwrap_or_else(|| panic!("{name}: se espera <comando>.v<N>.schema.json"));
        let version: u64 = version
            .parse()
            .unwrap_or_else(|_| panic!("{name}: versión no numérica"));
        let text = fs::read_to_string(&path).expect("leer esquema");
        let schema: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{name}: no es JSON válido ({e})"));
        out.insert((command.to_owned(), version), (path, schema));
    }
    out
}

/// Validador de un esquema y, para diagnosticar, uno por cada variante de
/// su `oneOf` (éxito, error…): sin ellos, el fallo solo diría «no valida
/// contra ninguna variante».
struct Compiled {
    whole: jsonschema::Validator,
    variants: Vec<(String, jsonschema::Validator)>,
}

fn compile(path: &Path, schema: &Value) -> jsonschema::Validator {
    jsonschema::validator_for(schema)
        .unwrap_or_else(|e| panic!("{}: esquema no válido ({e})", path.display()))
}

fn validators() -> &'static BTreeMap<(String, u64), Compiled> {
    static V: OnceLock<BTreeMap<(String, u64), Compiled>> = OnceLock::new();
    V.get_or_init(|| {
        schema_files()
            .into_iter()
            .map(|(key, (path, schema))| {
                let variants = schema["oneOf"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|variant| {
                        let mut one = schema.clone();
                        if let Value::Object(m) = &mut one {
                            m.remove("oneOf");
                            m.insert("allOf".into(), Value::Array(vec![variant.clone()]));
                        }
                        let name = variant["$ref"].as_str().unwrap_or("?").to_owned();
                        (name, compile(&path, &one))
                    })
                    .collect();
                let whole = compile(&path, &schema);
                (key, Compiled { whole, variants })
            })
            .collect()
    })
}

fn errors_of(v: &jsonschema::Validator, doc: &Value) -> Vec<String> {
    v.iter_errors(doc)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect()
}

/// Valida un documento del CLI contra el esquema de su `command` y su
/// `schema_version`. Sin esquema para esa pareja, también es un error: subir
/// `schema_version` obliga a publicar el esquema nuevo.
pub fn validate(doc: &Value) -> Result<(), Vec<String>> {
    let command = doc["command"]
        .as_str()
        .ok_or_else(|| vec!["falta «command»".to_owned()])?;
    let version = doc["schema_version"]
        .as_u64()
        .ok_or_else(|| vec!["falta «schema_version»".to_owned()])?;
    let compiled = validators()
        .get(&(command.to_owned(), version))
        .ok_or_else(|| vec![format!("no hay schemas/{command}.v{version}.schema.json")])?;
    let errors = errors_of(&compiled.whole, doc);
    if errors.is_empty() {
        return Ok(());
    }
    // La variante más cercana da el error útil (p. ej. la clave que falta).
    let closest = compiled
        .variants
        .iter()
        .map(|(name, v)| (name, errors_of(v, doc)))
        .min_by_key(|(_, e)| e.len());
    Err(match closest {
        Some((name, e)) if !e.is_empty() => {
            e.into_iter().map(|msg| format!("[{name}] {msg}")).collect()
        }
        _ => errors,
    })
}

/// Claves cuyo valor cambia en cada ejecución o depende de la máquina.
pub const VOLATILE: &[&str] = &[
    "created_at",
    "started_at",
    "finished_at",
    "plan_id",
    "plan_hash",
    "op_id",
    "undo_of",
    "digest",
    "newest_mtime",
    "dev",
    "ino",
    "mtime",
    "mtime_nsec",
    "ctime",
    "ctime_nsec",
    "parent_mode",
    "macos_version",
    "arch",
    "running_apps",
    // Cifras del volumen de la máquina; en un sandbox, además, `statfs` sale
    // redondeado y la capacidad disponible no se conoce (`null`).
    "volume",
];

/// Oculta lo que no es estable: rutas del fixture, fechas, IDs, hashes,
/// identidades de fichero y datos de la máquina. El tamaño de las carpetas
/// en `identity.size` depende del sistema de ficheros y también se oculta.
pub fn redact(fx: &Fx, v: &Value) -> Value {
    redact_in(fx, v, None)
}

fn redact_in(fx: &Fx, v: &Value, key: Option<&str>) -> Value {
    if let Some(k) = key
        && VOLATILE.contains(&k)
        && !v.is_null()
    {
        return json!(format!("[{k}]"));
    }
    match v {
        Value::String(s) => {
            let home = fx.home.to_str().expect("ruta UTF-8");
            let trash = fx.trash.to_str().expect("ruta UTF-8");
            json!(s.replace(trash, "[TRASH]").replace(home, "[HOME]"))
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| redact_in(fx, x, None)).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                let mut r = redact_in(fx, x, Some(k));
                if k == "identity"
                    && let Value::Object(id) = &mut r
                {
                    id.insert("size".into(), json!("[size]"));
                }
                out.insert(k.clone(), r);
            }
            // La versión del binario en `doctor`.
            if key == Some("report") && out.contains_key("catalog_version") {
                out.insert("version".into(), json!("[version]"));
            }
            Value::Object(out)
        }
        _ => v.clone(),
    }
}
