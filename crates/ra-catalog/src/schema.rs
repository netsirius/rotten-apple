//! Esquema de las reglas (ADR-05 §6.2, subconjunto del MVP).

use serde::{Deserialize, Serialize};

/// Nivel de riesgo (02 §0): solo lo «Seguro» se puede preseleccionar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Safe,
    Caution,
    Review,
}

/// Acción de una regla. Las reglas solo pueden mandar a la Papelera: el
/// borrado permanente nunca sale de una regla (RF-SEG-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Trash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    /// Acceso total al disco.
    Fda,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Localized {
    pub es: String,
    pub en: String,
}

impl Localized {
    pub fn get(&self, lang: &str) -> &str {
        if lang.starts_with("en") {
            &self.en
        } else {
            &self.es
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryInfo {
    pub id: String,
    pub title: Localized,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub version: u32,
    /// Categoría del catálogo (`CAT-01`…).
    pub cat: String,
    pub risk: Risk,
    pub regenerable: bool,
    /// Preseleccionar todos los candidatos de la regla.
    pub preselect: bool,
    /// Preseleccionar solo estos nombres (lista permitida, CAT-01).
    #[serde(default)]
    pub preselect_names: Vec<String>,
    pub action: RuleAction,
    #[serde(default)]
    pub requires: Vec<Requirement>,
    /// Raíces, siempre bajo `~/` y dentro de los prefijos permitidos.
    pub roots: Vec<String>,
    /// Patrones glob relativos a la raíz: lo que coincide es candidato (y no
    /// se desciende dentro).
    pub include: Vec<String>,
    /// Patrones glob relativos a la raíz que nunca son candidatos.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Profundidad máxima del recorrido (1 = hijos directos de la raíz).
    #[serde(default = "one")]
    pub max_depth: usize,
    /// Antigüedad mínima del elemento y de todo su contenido.
    #[serde(default)]
    pub min_age_days: u32,
    /// Bundle IDs que deben estar cerrados; `{bundle_id}` es el nombre del
    /// candidato sin `bundle_id_suffix`.
    #[serde(default)]
    pub app_not_running: Vec<String>,
    #[serde(default)]
    pub bundle_id_suffix: Option<String>,
    /// Nombres que nunca pueden aparecer en el candidato ni dentro de él.
    #[serde(default)]
    pub never: Vec<String>,
    pub title: Localized,
    pub explain: Localized,
    /// Justificación de que es seguro (documento, enlace…).
    pub evidence: String,
}

/// `catalog.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogFile {
    pub schema: u32,
    pub catalog_version: String,
    #[serde(default)]
    pub category: Vec<CategoryInfo>,
}

/// `rules/*.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RulesFile {
    #[serde(default)]
    pub rule: Vec<Rule>,
}
