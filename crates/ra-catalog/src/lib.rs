//! Catálogo de reglas declarativo (ADR-05).
//!
//! Las reglas son TOML versionado e incrustado en el binario (D-06: no hay
//! catálogo remoto). Al cargarse se validan con los *lints* del ADR-05 §6.4:
//! raíces dentro de los prefijos permitidos incrustados, nada demasiado
//! amplio, nada protegido y solo lo «Seguro» preseleccionable.
//!
//! [`Catalog::scan`] evalúa las reglas sobre el sistema de ficheros (capa 1
//! de validación, ADR-04 §5.4) y devuelve candidatos con su identidad y su
//! huella, listos para que `ra-plan` construya el plan.

#![forbid(unsafe_code)]

mod eval;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
mod schema;

use std::collections::BTreeSet;
use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ra_safety::paths::{self, fold};
use ra_safety::{Origin, ProtectedList};

pub use eval::{Block, Candidate, RootState, RootStatus, ScanOptions, ScanResult};
pub use schema::{CategoryInfo, Localized, Requirement, Risk, Rule, RuleAction};

/// Ficheros del catálogo incrustado.
const CATALOG_TOML: &str = include_str!("../../../catalog/catalog.toml");
const RULE_FILES: &[(&str, &str)] = &[
    (
        "cat-01-user-caches.toml",
        include_str!("../../../catalog/rules/cat-01-user-caches.toml"),
    ),
    (
        "cat-03-user-logs.toml",
        include_str!("../../../catalog/rules/cat-03-user-logs.toml"),
    ),
    (
        "cat-08-browsers.toml",
        include_str!("../../../catalog/rules/cat-08-browsers.toml"),
    ),
    (
        "cat-10-saved-state.toml",
        include_str!("../../../catalog/rules/cat-10-saved-state.toml"),
    ),
];

/// Prefijos raíz permitidos, incrustados en el binario (ADR-05 §6.3). Una
/// regla no puede tener raíces fuera de ellos; un prefijo nuevo exige una
/// versión nueva de la app.
pub const ALLOWED_PREFIXES: &[&str] = &[
    "~/Library/Caches",
    "~/Library/Logs",
    "~/Library/Saved Application State",
    "~/Library/Application Support/Google/Chrome",
];

/// Raíces demasiado amplias, prohibidas aunque estén en un prefijo.
const TOO_BROAD: &[&str] = &["~", "~/Library", "~/Library/Application Support"];

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{file}: TOML no válido: {detail}")]
    Parse { file: String, detail: String },
    #[error("regla {rule}: {detail}")]
    Lint { rule: String, detail: String },
    #[error("catálogo: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub(crate) struct Compiled {
    pub include: GlobSet,
    pub exclude: GlobSet,
    pub never: BTreeSet<String>,
    pub preselect_names: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub schema: u32,
    pub catalog_version: String,
    pub categories: Vec<CategoryInfo>,
    pub rules: Vec<Rule>,
    compiled: Vec<Compiled>,
}

fn glob_set(patterns: &[String], rule: &str) -> Result<GlobSet, CatalogError> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let g = GlobBuilder::new(&fold(p))
            .case_insensitive(true)
            .literal_separator(true)
            .build()
            .map_err(|e| CatalogError::Lint {
                rule: rule.to_owned(),
                detail: format!("patrón «{p}»: {e}"),
            })?;
        b.add(g);
    }
    b.build().map_err(|e| CatalogError::Lint {
        rule: rule.to_owned(),
        detail: e.to_string(),
    })
}

impl Catalog {
    /// El catálogo incrustado en el binario.
    pub fn embedded() -> Result<Self, CatalogError> {
        Self::from_sources(CATALOG_TOML, RULE_FILES)
    }

    /// Catálogo a partir de `catalog.toml` y ficheros de reglas.
    pub fn from_sources(catalog: &str, rule_files: &[(&str, &str)]) -> Result<Self, CatalogError> {
        let head: schema::CatalogFile =
            toml::from_str(catalog).map_err(|e| CatalogError::Parse {
                file: "catalog.toml".into(),
                detail: e.to_string(),
            })?;
        let mut rules = Vec::new();
        for (name, text) in rule_files {
            let f: schema::RulesFile = toml::from_str(text).map_err(|e| CatalogError::Parse {
                file: (*name).to_owned(),
                detail: e.to_string(),
            })?;
            rules.extend(f.rule);
        }
        let mut cat = Catalog {
            schema: head.schema,
            catalog_version: head.catalog_version,
            categories: head.category,
            rules,
            compiled: Vec::new(),
        };
        cat.validate()?;
        Ok(cat)
    }

    /// Solo las reglas indicadas (para pruebas y `--category`).
    pub fn filtered(&self, keep: impl Fn(&Rule) -> bool) -> Self {
        let mut out = self.clone();
        let (rules, compiled): (Vec<_>, Vec<_>) = self
            .rules
            .iter()
            .cloned()
            .zip(self.compiled.iter().cloned())
            .filter(|(r, _)| keep(r))
            .unzip();
        out.rules = rules;
        out.compiled = compiled;
        out
    }

    pub fn category(&self, id: &str) -> Option<&CategoryInfo> {
        self.categories.iter().find(|c| c.id == id)
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }

    fn validate(&mut self) -> Result<(), CatalogError> {
        if self.schema != 1 {
            return Err(CatalogError::Invalid(format!(
                "schema {} no soportado",
                self.schema
            )));
        }
        let lint_home = Path::new("/Users/lint");
        let protected = ProtectedList::embedded(
            lint_home,
            &lint_home.join("Library/Application Support/rotten-apple.lint"),
        );
        let mut ids = BTreeSet::new();
        let mut compiled = Vec::new();
        for r in &self.rules {
            let lint = |detail: String| CatalogError::Lint {
                rule: r.id.clone(),
                detail,
            };
            if !ids.insert(r.id.clone()) {
                return Err(lint("id duplicado".into()));
            }
            if r.version == 0 {
                return Err(lint("version debe ser ≥ 1".into()));
            }
            if self.category(&r.cat).is_none() {
                return Err(lint(format!("categoría desconocida {}", r.cat)));
            }
            if (r.preselect || !r.preselect_names.is_empty()) && r.risk != Risk::Safe {
                return Err(lint("solo lo «Seguro» se puede preseleccionar".into()));
            }
            if r.roots.is_empty() || r.include.is_empty() {
                return Err(lint("roots e include no pueden estar vacíos".into()));
            }
            if !(1..=4).contains(&r.max_depth) {
                return Err(lint("max_depth debe estar entre 1 y 4".into()));
            }
            if r.bundle_id_suffix.is_some() && !r.app_not_running.iter().any(|a| a == "{bundle_id}")
            {
                return Err(lint(
                    "bundle_id_suffix sin {bundle_id} en app_not_running".into(),
                ));
            }
            for root in &r.roots {
                if !root.starts_with("~/") {
                    return Err(lint(format!("la raíz {root} debe empezar por ~/")));
                }
                if TOO_BROAD
                    .iter()
                    .any(|b| fold(b) == fold(root.trim_end_matches('/')))
                {
                    return Err(lint(format!("raíz demasiado amplia: {root}")));
                }
                let abs = paths::expand_home(root, lint_home).map_err(|e| lint(e.to_string()))?;
                let allowed = ALLOWED_PREFIXES.iter().any(|p| {
                    paths::expand_home(p, lint_home).is_ok_and(|pp| paths::is_within(&abs, &pp))
                });
                if !allowed {
                    return Err(lint(format!(
                        "la raíz {root} está fuera de los prefijos permitidos"
                    )));
                }
                if let Err(reason) = protected.check(&abs.join("x"), Origin::Rules) {
                    return Err(lint(format!("la raíz {root} está protegida: {reason}")));
                }
            }
            compiled.push(Compiled {
                include: glob_set(&r.include, &r.id)?,
                exclude: glob_set(&r.exclude, &r.id)?,
                never: r.never.iter().map(|s| fold(s)).collect(),
                preselect_names: r.preselect_names.iter().map(|s| fold(s)).collect(),
            });
        }
        self.compiled = compiled;
        Ok(())
    }

    /// Evalúa todas las reglas (capa 1).
    pub fn scan(&self, opts: &ScanOptions<'_>) -> ScanResult {
        eval::scan(self, opts)
    }

    pub(crate) fn compiled(&self) -> &[Compiled] {
        &self.compiled
    }
}

#[cfg(test)]
mod tests;
