//! Plan inmutable con hash (ADR-04 §5.2, §5.3).
//!
//! Un [`Plan`] se construye una vez a partir de unos candidatos, aplicando la
//! capa 2 de validación: lista protegida incrustada, exclusiones del usuario
//! y elementos bloqueados por la capa 1. Después no se puede modificar: sus
//! campos son privados y solo tiene lectores.
//!
//! Los candidatos pueden venir del escaneo del catálogo ([`Origin::Rules`])
//! o de una selección explícita del usuario ([`Origin::ExplicitSelection`],
//! `rotten trash put`). Todos pasan por las mismas comprobaciones, y cada
//! elemento lleva anotado su origen.
//!
//! `plan_hash` es el BLAKE3 de la serialización canónica (claves ordenadas)
//! del plan sin el propio hash. La aprobación se liga a ese hash y caduca a
//! los 30 minutos (README §8): el ejecutor solo acepta un [`ApprovedPlan`].

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ra_catalog::{Block, Candidate, Risk, ScanResult};
use ra_safety::paths;
use ra_safety::{
    ExclusionMatch, Exclusions, FileKind, Identity, Origin, ProtectedList, ProtectionReason,
    TreeFingerprint,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// v2: el motivo `app` de un elemento bloqueado lleva su detalle en
/// `detail` (en v1 la clave `kind` salía repetida).
pub const PLAN_SCHEMA: &str = "rotten.plan.v2";
pub const SCHEMA_VERSION: u32 = 2;
/// Una revisión más antigua no se ejecuta sin regenerarla (README §8).
pub const PREVIEW_TTL: Duration = Duration::from_secs(30 * 60);

/// Qué se hace con cada elemento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// A la Papelera (por defecto, reversible).
    Trash,
    /// Borrado permanente; solo con `--permanent`.
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRef {
    pub id: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub item_id: u32,
    pub path: PathBuf,
    pub kind: FileKind,
    pub identity: Identity,
    pub fingerprint: TreeFingerprint,
    pub bytes_allocated: u64,
    pub rule: RuleRef,
    pub category: String,
    pub risk: Risk,
    pub regenerable: bool,
    pub preselected: bool,
    pub action: Action,
    pub reversible: bool,
    /// Apps que deben seguir cerradas al ejecutar.
    pub apps: Vec<String>,
    /// De dónde sale el elemento. No se serializa cuando es
    /// [`Origin::Rules`]: los planes del catálogo quedan igual que antes
    /// (mismo JSON, mismo `plan_hash`) y la regla ya está en `rule`.
    #[serde(default = "rules_origin", skip_serializing_if = "is_rules_origin")]
    pub origin: Origin,
}

fn rules_origin() -> Origin {
    Origin::Rules
}

fn is_rules_origin(o: &Origin) -> bool {
    *o == Origin::Rules
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum SkipReason {
    Blocked(Block),
    Protected(ProtectionReason),
    Excluded(ExclusionMatch),
    /// Está dentro de otro elemento del plan.
    Overlaps(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub rule: RuleRef,
    pub category: String,
    pub reason: SkipReason,
}

/// Candidatos válidos que no se seleccionaron (p. ej. CAT-01 sin marcar).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotSelected {
    pub items: u64,
    pub bytes_allocated: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub items: u64,
    pub bytes_allocated: u64,
    pub irreversible_items: u64,
}

/// Qué candidatos entran en el plan.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// `None`: lo preseleccionado. `Some`: todo lo «Seguro» de esas
    /// categorías (y lo demás solo con `include_review`).
    pub categories: Option<Vec<String>>,
    pub include_review: bool,
}

impl Selection {
    fn wants(&self, c: &Candidate) -> bool {
        match &self.categories {
            None => c.preselected,
            Some(cats) => {
                cats.iter().any(|k| k.eq_ignore_ascii_case(&c.cat))
                    && (c.risk == Risk::Safe || self.include_review)
            }
        }
    }
}

pub struct PlanInputs<'a> {
    /// Candidatos: del escaneo del catálogo o de una selección explícita.
    pub scan: &'a ScanResult,
    pub home: &'a Path,
    pub protected: &'a ProtectedList,
    pub exclusions: &'a Exclusions,
    /// Solo con [`Origin::Rules`]: con una selección explícita entran todos
    /// los candidatos (los ha elegido el usuario uno a uno).
    pub selection: &'a Selection,
    pub action: Action,
    /// Origen de todos los candidatos de `scan`.
    pub origin: Origin,
    pub now: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    schema: String,
    schema_version: u32,
    plan_id: String,
    created_at: String,
    catalog_version: String,
    origin: Origin,
    home: PathBuf,
    action: Action,
    items: Vec<PlanItem>,
    skipped: Vec<Skipped>,
    not_selected: BTreeMap<String, NotSelected>,
    totals: Totals,
    plan_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("the plan hash does not match (expected {expected}, got {actual})")]
    HashMismatch { expected: String, actual: String },
    #[error("the review has expired ({age_secs} s): generate the plan again")]
    Expired { age_secs: u64 },
    #[error("invalid plan: {0}")]
    Invalid(String),
}

impl Plan {
    /// Construye el plan aplicando la capa 2 de validación.
    pub fn build(inp: &PlanInputs<'_>) -> Plan {
        let mut kept: Vec<&Candidate> = Vec::new();
        let mut skipped = Vec::new();
        let mut not_selected: BTreeMap<String, NotSelected> = BTreeMap::new();

        for c in &inp.scan.candidates {
            let rule = RuleRef {
                id: c.rule_id.clone(),
                version: c.rule_version,
            };
            let skip = |reason| Skipped {
                path: c.path.clone(),
                rule: rule.clone(),
                category: c.cat.clone(),
                reason,
            };
            let wanted = inp.origin == Origin::ExplicitSelection || inp.selection.wants(c);
            if !wanted {
                if c.block.is_none() {
                    let e = not_selected.entry(c.cat.clone()).or_default();
                    e.items += 1;
                    e.bytes_allocated += c.fingerprint.as_ref().map_or(0, |f| f.bytes_allocated);
                }
                continue;
            }
            if let Some(b) = &c.block {
                skipped.push(skip(SkipReason::Blocked(b.clone())));
                continue;
            }
            if let Err(r) = inp.protected.check(&c.path, inp.origin) {
                skipped.push(skip(SkipReason::Protected(r)));
                continue;
            }
            if let Some(m) = inp.exclusions.check(&c.path, &c.rule_id, &c.cat) {
                skipped.push(skip(SkipReason::Excluded(m)));
                continue;
            }
            if c.fingerprint.is_none() {
                skipped.push(skip(SkipReason::Blocked(Block::Unreadable {
                    detail: "no fingerprint".into(),
                })));
                continue;
            }
            kept.push(c);
        }

        // Orden determinista y sin solapes: si un elemento está dentro de
        // otro, se omite el interior (el exterior ya lo incluye).
        kept.sort_by(|a, b| a.path.cmp(&b.path));
        let mut items: Vec<PlanItem> = Vec::new();
        for c in kept {
            if let Some(outer) = items.iter().find(|i| paths::is_within(&c.path, &i.path)) {
                skipped.push(Skipped {
                    path: c.path.clone(),
                    rule: RuleRef {
                        id: c.rule_id.clone(),
                        version: c.rule_version,
                    },
                    category: c.cat.clone(),
                    reason: SkipReason::Overlaps(outer.path.clone()),
                });
                continue;
            }
            let Some(fp) = c.fingerprint.clone() else {
                continue;
            };
            items.push(PlanItem {
                item_id: items.len() as u32 + 1,
                path: c.path.clone(),
                kind: c.identity.kind,
                identity: c.identity,
                bytes_allocated: fp.bytes_allocated,
                fingerprint: fp,
                rule: RuleRef {
                    id: c.rule_id.clone(),
                    version: c.rule_version,
                },
                category: c.cat.clone(),
                risk: c.risk,
                regenerable: c.regenerable,
                preselected: c.preselected,
                action: inp.action,
                reversible: inp.action == Action::Trash,
                apps: c.apps.clone(),
                origin: inp.origin,
            });
        }
        skipped.sort_by(|a, b| a.path.cmp(&b.path));

        let totals = Totals {
            items: items.len() as u64,
            bytes_allocated: items.iter().map(|i| i.bytes_allocated).sum(),
            irreversible_items: items.iter().filter(|i| !i.reversible).count() as u64,
        };
        let mut plan = Plan {
            schema: PLAN_SCHEMA.into(),
            schema_version: SCHEMA_VERSION,
            plan_id: new_id(inp.now),
            created_at: humantime::format_rfc3339_millis(inp.now).to_string(),
            catalog_version: inp.scan.catalog_version.clone(),
            origin: inp.origin,
            home: inp.home.to_path_buf(),
            action: inp.action,
            items,
            skipped,
            not_selected,
            totals,
            plan_hash: String::new(),
        };
        plan.plan_hash = plan.compute_hash();
        plan
    }

    /// Hash canónico del plan sin el campo `plan_hash`.
    pub fn compute_hash(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.remove("plan_hash");
        }
        let mut out = String::new();
        canonical_json(&v, &mut out);
        format!("blake3:{}", blake3::hash(out.as_bytes()).to_hex())
    }

    /// Comprueba que `plan_hash` corresponde al contenido.
    pub fn verify(&self) -> Result<(), PlanError> {
        let actual = self.compute_hash();
        if actual != self.plan_hash {
            return Err(PlanError::HashMismatch {
                expected: self.plan_hash.clone(),
                actual,
            });
        }
        Ok(())
    }

    /// Carga un plan serializado y verifica su hash.
    pub fn from_json(text: &str) -> Result<Plan, PlanError> {
        let p: Plan = serde_json::from_str(text).map_err(|e| PlanError::Invalid(e.to_string()))?;
        if p.schema != PLAN_SCHEMA || p.schema_version != SCHEMA_VERSION {
            return Err(PlanError::Invalid(format!(
                "unsupported schema {}",
                p.schema
            )));
        }
        p.verify()?;
        Ok(p)
    }

    /// Aprueba el plan para ejecutarlo. Exige que `plan_hash` sea el que el
    /// usuario revisó y que la revisión no haya caducado.
    pub fn approve(self, reviewed_hash: &str, now: SystemTime) -> Result<ApprovedPlan, PlanError> {
        self.verify()?;
        if reviewed_hash != self.plan_hash {
            return Err(PlanError::HashMismatch {
                expected: reviewed_hash.to_owned(),
                actual: self.plan_hash.clone(),
            });
        }
        let created = humantime::parse_rfc3339(&self.created_at)
            .map_err(|e| PlanError::Invalid(e.to_string()))?;
        let age = now.duration_since(created).unwrap_or_default();
        if age > PREVIEW_TTL {
            return Err(PlanError::Expired {
                age_secs: age.as_secs(),
            });
        }
        Ok(ApprovedPlan { plan: self })
    }

    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }
    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }
    pub fn created_at(&self) -> &str {
        &self.created_at
    }
    pub fn catalog_version(&self) -> &str {
        &self.catalog_version
    }
    pub fn home(&self) -> &Path {
        &self.home
    }
    /// Origen de los elementos del plan.
    pub fn origin(&self) -> Origin {
        self.origin
    }
    pub fn action(&self) -> Action {
        self.action
    }
    pub fn items(&self) -> &[PlanItem] {
        &self.items
    }
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }
    pub fn not_selected(&self) -> &BTreeMap<String, NotSelected> {
        &self.not_selected
    }
    pub fn totals(&self) -> &Totals {
        &self.totals
    }
}

/// Plan aprobado: lo único que acepta el ejecutor.
#[derive(Debug, Clone)]
pub struct ApprovedPlan {
    plan: Plan,
}

impl ApprovedPlan {
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

/// JSON canónico: claves ordenadas, sin espacios. No depende de que
/// `serde_json` conserve o no el orden de inserción.
fn canonical_json(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                canonical_json(&m[*k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_json(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Identificador ordenable por tiempo: milisegundos en hexadecimal y un
/// sufijo derivado del reloj, el PID y un contador.
pub fn new_id(now: SystemTime) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut h = blake3::Hasher::new();
    h.update(&d.as_nanos().to_le_bytes());
    h.update(&std::process::id().to_le_bytes());
    h.update(&n.to_le_bytes());
    let suffix = &h.finalize().to_hex()[..6];
    format!("{:011x}-{suffix}", d.as_millis())
}

#[cfg(test)]
mod tests;
