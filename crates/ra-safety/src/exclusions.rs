//! Exclusiones del usuario (`config.toml`), fail-closed (RF-SEG-05, CU-07).
//!
//! - Si `config.toml` no existe, no hay exclusiones: es una instalación limpia.
//! - Si existe pero no se puede leer, no es TOML válido, tiene claves
//!   desconocidas, un patrón inválido o una ruta relativa, el estado es
//!   [`ConfigState::Corrupt`]: el escaneo puede seguir con un aviso, pero
//!   ninguna acción destructiva se ejecuta hasta corregirlo.
//!
//! Las exclusiones prevalecen sobre cualquier regla. Una ruta excluida
//! bloquea también a sus ancestros: mover la carpeta que la contiene la
//! movería con ella.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::paths::{self, fold};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub exclusions: ExclusionLists,
}

fn default_schema() -> u32 {
    CONFIG_SCHEMA_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            exclusions: ExclusionLists::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusionLists {
    /// Rutas absolutas o con `~`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Patrones glob sobre la ruta completa (`~` permitido al principio).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globs: Vec<String>,
    /// IDs de regla del catálogo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// IDs de categoría (`CAT-01`…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// Bundle IDs (coinciden con cualquier componente de la ruta).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundle_ids: Vec<String>,
}

/// Una exclusión tal como la escribe el usuario en `rotten exclude`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Exclusion {
    Path(String),
    Glob(String),
    Rule(String),
    Category(String),
    BundleId(String),
}

impl Exclusion {
    /// `rule:<id>`, `cat:<CAT-xx>`, `bundle:<id>`, un patrón glob (`*`, `?`,
    /// `[`) o una ruta absoluta / con `~`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("the exclusion is empty".into());
        }
        let ex = if let Some(v) = s.strip_prefix("rule:") {
            Exclusion::Rule(v.to_owned())
        } else if let Some(v) = s
            .strip_prefix("cat:")
            .or_else(|| s.strip_prefix("category:"))
        {
            Exclusion::Category(v.to_owned())
        } else if let Some(v) = s.strip_prefix("bundle:") {
            Exclusion::BundleId(v.to_owned())
        } else if s.contains(['*', '?', '[']) {
            Exclusion::Glob(s.to_owned())
        } else {
            Exclusion::Path(s.to_owned())
        };
        ex.validate()?;
        Ok(ex)
    }

    fn validate(&self) -> Result<(), String> {
        let probe_home = Path::new("/home-probe");
        match self {
            Exclusion::Path(p) => paths::expand_home(p, probe_home)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Exclusion::Glob(g) => {
                let expanded = expand_glob_home(g, probe_home);
                if !expanded.starts_with('/') {
                    return Err(format!(
                        "the pattern must be absolute or start with ~/: {g}"
                    ));
                }
                GlobBuilder::new(&expanded)
                    .build()
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
            Exclusion::Rule(v) | Exclusion::Category(v) | Exclusion::BundleId(v) => {
                if v.trim().is_empty() || v.contains('/') {
                    Err(format!("invalid value: «{v}»"))
                } else {
                    Ok(())
                }
            }
        }
    }
}

impl fmt::Display for Exclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exclusion::Path(v) | Exclusion::Glob(v) => f.write_str(v),
            Exclusion::Rule(v) => write!(f, "rule:{v}"),
            Exclusion::Category(v) => write!(f, "cat:{v}"),
            Exclusion::BundleId(v) => write!(f, "bundle:{v}"),
        }
    }
}

impl Config {
    fn list_mut(&mut self, ex: &Exclusion) -> (&mut Vec<String>, String) {
        let l = &mut self.exclusions;
        match ex {
            Exclusion::Path(v) => (&mut l.paths, v.clone()),
            Exclusion::Glob(v) => (&mut l.globs, v.clone()),
            Exclusion::Rule(v) => (&mut l.rules, v.clone()),
            Exclusion::Category(v) => (&mut l.categories, v.clone()),
            Exclusion::BundleId(v) => (&mut l.bundle_ids, v.clone()),
        }
    }

    /// Añade una exclusión. Devuelve `false` si ya estaba.
    pub fn add(&mut self, ex: &Exclusion) -> bool {
        let (list, v) = self.list_mut(ex);
        if list.contains(&v) {
            return false;
        }
        list.push(v);
        true
    }

    /// Quita una exclusión. Devuelve `false` si no estaba.
    pub fn remove(&mut self, ex: &Exclusion) -> bool {
        let (list, v) = self.list_mut(ex);
        let before = list.len();
        list.retain(|x| x != &v);
        list.len() != before
    }

    /// Todas las exclusiones, en el formato de `rotten exclude list`.
    pub fn all(&self) -> Vec<Exclusion> {
        let l = &self.exclusions;
        let mut out = Vec::new();
        out.extend(l.paths.iter().cloned().map(Exclusion::Path));
        out.extend(l.globs.iter().cloned().map(Exclusion::Glob));
        out.extend(l.rules.iter().cloned().map(Exclusion::Rule));
        out.extend(l.categories.iter().cloned().map(Exclusion::Category));
        out.extend(l.bundle_ids.iter().cloned().map(Exclusion::BundleId));
        out
    }

    /// Escribe el fichero de forma atómica (temporal + `rename`).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path without a folder"))?;
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".config.toml.{}.tmp", std::process::id()));
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }
}

/// Estado de la configuración tras intentar cargarla.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigState {
    Missing,
    Loaded(Config),
    Corrupt { path: PathBuf, reason: String },
}

impl ConfigState {
    /// Carga `config.toml` y valida todas las exclusiones contra `home`.
    pub fn load(path: &Path, home: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return ConfigState::Missing,
            Err(e) => return Self::corrupt(path, format!("cannot be read: {e}")),
        };
        let config: Config = match toml::from_str(&text) {
            Ok(c) => c,
            Err(e) => return Self::corrupt(path, format!("invalid TOML: {e}")),
        };
        if config.schema_version != CONFIG_SCHEMA_VERSION {
            return Self::corrupt(
                path,
                format!("unsupported schema_version {}", config.schema_version),
            );
        }
        if let Err(e) = Exclusions::compile(&config, home) {
            return Self::corrupt(path, e);
        }
        ConfigState::Loaded(config)
    }

    fn corrupt(path: &Path, reason: String) -> Self {
        ConfigState::Corrupt {
            path: path.to_path_buf(),
            reason,
        }
    }

    /// Exclusiones listas para usar, o el motivo por el que no se pueden
    /// aplicar (y por tanto no se puede actuar).
    pub fn exclusions(&self, home: &Path) -> Result<Exclusions, String> {
        match self {
            ConfigState::Missing => Exclusions::compile(&Config::default(), home),
            ConfigState::Loaded(c) => Exclusions::compile(c, home),
            ConfigState::Corrupt { path, reason } => {
                Err(format!("{} is corrupt: {reason}", path.display()))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExclusionMatch {
    Path(String),
    ContainsExcludedPath(String),
    Glob(String),
    ContainsGlobMatch(String),
    Rule(String),
    Category(String),
    BundleId(String),
}

/// Exclusiones compiladas para una carpeta personal concreta.
#[derive(Debug, Clone)]
pub struct Exclusions {
    paths: Vec<(String, PathBuf)>,
    globs: Vec<String>,
    glob_set: GlobSet,
    rules: Vec<String>,
    categories: Vec<String>,
    bundle_ids: Vec<String>,
}

fn expand_glob_home(g: &str, home: &Path) -> String {
    if g == "~" {
        home.display().to_string()
    } else if let Some(rest) = g.strip_prefix("~/") {
        format!("{}/{}", home.display(), rest)
    } else {
        g.to_owned()
    }
}

impl Exclusions {
    pub fn compile(config: &Config, home: &Path) -> Result<Self, String> {
        let l = &config.exclusions;
        let mut paths_out = Vec::new();
        for p in &l.paths {
            let abs = paths::expand_home(p, home).map_err(|e| format!("exclusion «{p}»: {e}"))?;
            paths_out.push((p.clone(), abs));
        }
        let mut builder = GlobSetBuilder::new();
        for g in &l.globs {
            let expanded = expand_glob_home(g, home);
            if !expanded.starts_with('/') {
                return Err(format!("exclusion «{g}»: the pattern must be absolute"));
            }
            let glob = GlobBuilder::new(&fold(&expanded))
                .case_insensitive(true)
                .literal_separator(true)
                .build()
                .map_err(|e| format!("exclusion «{g}»: {e}"))?;
            builder.add(glob);
        }
        let glob_set = builder.build().map_err(|e| e.to_string())?;
        for v in l.rules.iter().chain(&l.categories).chain(&l.bundle_ids) {
            if v.trim().is_empty() {
                return Err("there is an empty exclusion".into());
            }
        }
        Ok(Self {
            paths: paths_out,
            globs: l.globs.clone(),
            glob_set,
            rules: l.rules.iter().map(|s| fold(s)).collect(),
            categories: l.categories.iter().map(|s| fold(s)).collect(),
            bundle_ids: l.bundle_ids.iter().map(|s| fold(s)).collect(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
            && self.globs.is_empty()
            && self.rules.is_empty()
            && self.categories.is_empty()
            && self.bundle_ids.is_empty()
    }

    /// Comprueba un candidato. Si es una carpeta y hay patrones glob, también
    /// recorre sus descendientes (sin seguir enlaces): si alguno coincide, la
    /// carpeta entera queda excluida.
    pub fn check(&self, path: &Path, rule_id: &str, category: &str) -> Option<ExclusionMatch> {
        if self.rules.contains(&fold(rule_id)) {
            return Some(ExclusionMatch::Rule(rule_id.to_owned()));
        }
        if self.categories.contains(&fold(category)) {
            return Some(ExclusionMatch::Category(category.to_owned()));
        }
        for (raw, abs) in &self.paths {
            if paths::is_within(path, abs) {
                return Some(ExclusionMatch::Path(raw.clone()));
            }
            if paths::is_within(abs, path) {
                return Some(ExclusionMatch::ContainsExcludedPath(raw.clone()));
            }
        }
        for comp in paths::folded_components(path) {
            for id in &self.bundle_ids {
                if comp == *id || comp.starts_with(&format!("{id}.")) {
                    return Some(ExclusionMatch::BundleId(id.clone()));
                }
            }
        }
        if !self.globs.is_empty() {
            if let Some(i) = self.glob_match(path) {
                return Some(ExclusionMatch::Glob(self.globs[i].clone()));
            }
            if let Some(i) = self.glob_match_descendants(path) {
                return Some(ExclusionMatch::ContainsGlobMatch(self.globs[i].clone()));
            }
        }
        None
    }

    fn glob_match(&self, path: &Path) -> Option<usize> {
        let folded = fold(&path.to_string_lossy());
        self.glob_set.matches(&folded).first().copied()
    }

    fn glob_match_descendants(&self, path: &Path) -> Option<usize> {
        let meta = fs::symlink_metadata(path).ok()?;
        if !meta.is_dir() {
            return None;
        }
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let p = entry.path();
                if let Some(i) = self.glob_match(&p) {
                    return Some(i);
                }
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(p);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/ana";

    fn compile(toml_text: &str) -> Result<Exclusions, String> {
        let c: Config = toml::from_str(toml_text).map_err(|e| e.to_string())?;
        Exclusions::compile(&c, Path::new(HOME))
    }

    #[test]
    fn missing_file_is_empty_and_valid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = ConfigState::load(&dir.path().join("config.toml"), Path::new(HOME));
        assert_eq!(s, ConfigState::Missing);
        assert!(s.exclusions(Path::new(HOME)).expect("vacías").is_empty());
    }

    #[test]
    fn corrupt_files_fail_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("config.toml");
        for bad in [
            "esto no es toml [",
            "[exclusions]\npaths = \"no es una lista\"\n",
            "[exclusion]\npaths = []\n",
            "[exclusions]\npaths = [\"relativa/ruta\"]\n",
            "[exclusions]\nglobs = [\"~/Library/[\"]\n",
            "schema_version = 99\n",
        ] {
            fs::write(&p, bad).expect("write");
            let s = ConfigState::load(&p, Path::new(HOME));
            assert!(matches!(s, ConfigState::Corrupt { .. }), "{bad:?} → {s:?}");
            assert!(s.exclusions(Path::new(HOME)).is_err());
        }
    }

    #[test]
    fn path_exclusion_covers_descendants_and_ancestors() {
        let ex =
            compile("[exclusions]\npaths = [\"~/Library/Caches/com.foo/keep\"]\n").expect("válida");
        let hit = |p: &str| ex.check(Path::new(p), "r", "CAT-01").is_some();
        assert!(hit("/Users/ana/Library/Caches/com.foo/keep/x"));
        assert!(hit("/Users/ana/Library/Caches/com.foo"));
        assert!(hit("/users/ANA/library/caches/COM.FOO"));
        assert!(!hit("/Users/ana/Library/Caches/com.bar"));
    }

    #[test]
    fn rule_category_and_bundle() {
        let ex = compile(
            "[exclusions]\nrules = [\"user-caches.other\"]\ncategories = [\"cat-08\"]\nbundle_ids = [\"com.foo\"]\n",
        )
        .expect("válida");
        let p = Path::new("/Users/ana/Library/Caches/x");
        assert!(ex.check(p, "user-caches.other", "CAT-01").is_some());
        assert!(ex.check(p, "r", "CAT-08").is_some());
        assert!(
            ex.check(
                Path::new("/Users/ana/Library/Saved Application State/com.foo.savedState"),
                "r",
                "CAT-10"
            )
            .is_some()
        );
        assert!(ex.check(p, "r", "CAT-01").is_none());
    }

    #[test]
    fn glob_matches_item_and_descendants() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = fs::canonicalize(dir.path()).expect("canon");
        let cache = home.join("Library/Caches/com.app");
        fs::create_dir_all(cache.join("sub")).expect("mkdir");
        fs::write(cache.join("sub/precious.db"), b"x").expect("write");
        let c: Config =
            toml::from_str("[exclusions]\nglobs = [\"~/Library/Caches/**/*.db\"]\n").expect("toml");
        let ex = Exclusions::compile(&c, &home).expect("válida");
        assert!(matches!(
            ex.check(&cache, "r", "CAT-01"),
            Some(ExclusionMatch::ContainsGlobMatch(_))
        ));
        assert!(
            ex.check(&home.join("Library/Caches/other"), "r", "CAT-01")
                .is_none()
        );
    }

    #[test]
    fn parse_and_roundtrip() {
        let mut c = Config::default();
        for s in [
            "~/Library/Caches/a",
            "~/x/*.log",
            "rule:r1",
            "cat:CAT-03",
            "bundle:com.a",
        ] {
            let ex = Exclusion::parse(s).expect("válida");
            assert_eq!(ex.to_string(), s);
            assert!(c.add(&ex));
            assert!(!c.add(&ex));
        }
        assert_eq!(c.all().len(), 5);
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("sub/config.toml");
        c.save(&p).expect("save");
        assert_eq!(
            ConfigState::load(&p, Path::new(HOME)),
            ConfigState::Loaded(c.clone())
        );
        assert!(c.remove(&Exclusion::parse("rule:r1").expect("válida")));
        assert!(Exclusion::parse("relativa").is_err());
        assert!(Exclusion::parse("").is_err());
    }
}
