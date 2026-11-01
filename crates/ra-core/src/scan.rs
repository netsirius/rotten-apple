//! Resumen del escaneo por categoría (`rotten scan`).

use std::collections::BTreeMap;

use ra_catalog::{Catalog, Requirement, RootState, ScanResult};
use ra_safety::{ConfigState, Exclusions, Origin, ProtectedList};
use serde::Serialize;

/// `complete | partial | unavailable` (07 §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategorySummary {
    pub id: String,
    pub title_es: String,
    pub title_en: String,
    pub scan_status: ScanStatus,
    /// Candidatos que podrían entrar en un plan.
    pub items: u64,
    pub bytes_allocated: u64,
    pub preselected_items: u64,
    pub preselected_bytes: u64,
    /// Omitidos: bloqueados (app abierta, recientes…), protegidos o excluidos.
    pub skipped_items: u64,
    /// Permisos que necesita alguna raíz no disponible.
    pub requires: Vec<Requirement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub scan_status: ScanStatus,
    pub catalog_version: String,
    pub categories: Vec<CategorySummary>,
    /// `ok`, `missing` o `corrupt`.
    pub config: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

pub(crate) fn summarize(
    catalog: &Catalog,
    res: &ScanResult,
    protected: &ProtectedList,
    exclusions: Option<&Exclusions>,
    config: &ConfigState,
) -> ScanReport {
    let mut cats: BTreeMap<String, CategorySummary> = BTreeMap::new();
    for rule in &catalog.rules {
        cats.entry(rule.cat.clone()).or_insert_with(|| {
            let info = catalog.category(&rule.cat);
            CategorySummary {
                id: rule.cat.clone(),
                title_es: info.map(|i| i.title.es.clone()).unwrap_or_default(),
                title_en: info.map(|i| i.title.en.clone()).unwrap_or_default(),
                scan_status: ScanStatus::Complete,
                items: 0,
                bytes_allocated: 0,
                preselected_items: 0,
                preselected_bytes: 0,
                skipped_items: 0,
                requires: vec![],
                notes: vec![],
            }
        });
    }

    // Estado por categoría a partir del estado de sus raíces.
    let mut unavailable: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for r in &res.roots {
        let e = unavailable.entry(r.cat.as_str()).or_default();
        e.1 += 1;
        let Some(sum) = cats.get_mut(&r.cat) else {
            continue;
        };
        match &r.state {
            RootState::Complete | RootState::Absent => {}
            RootState::Partial { detail } => {
                sum.scan_status = ScanStatus::Partial;
                sum.notes.push(detail.clone());
            }
            RootState::Unavailable { detail } => {
                e.0 += 1;
                sum.scan_status = ScanStatus::Partial;
                sum.notes.push(format!("{}: {detail}", r.root.display()));
                if let Some(rule) = catalog.rule(&r.rule_id) {
                    for req in &rule.requires {
                        if !sum.requires.contains(req) {
                            sum.requires.push(*req);
                        }
                    }
                }
            }
        }
    }
    for (cat, (bad, total)) in unavailable {
        if bad > 0
            && bad == total
            && let Some(s) = cats.get_mut(cat)
        {
            s.scan_status = ScanStatus::Unavailable;
        }
    }

    for c in &res.candidates {
        let Some(sum) = cats.get_mut(&c.cat) else {
            continue;
        };
        let excluded = exclusions.is_some_and(|ex| ex.check(&c.path, &c.rule_id, &c.cat).is_some());
        if c.block.is_some() || protected.check(&c.path, Origin::Rules).is_err() || excluded {
            sum.skipped_items += 1;
            continue;
        }
        let bytes = c.fingerprint.as_ref().map_or(0, |f| f.bytes_allocated);
        sum.items += 1;
        sum.bytes_allocated += bytes;
        if c.preselected {
            sum.preselected_items += 1;
            sum.preselected_bytes += bytes;
        }
    }

    let categories: Vec<CategorySummary> = cats.into_values().collect();
    let scan_status = if categories
        .iter()
        .all(|c| c.scan_status == ScanStatus::Complete)
    {
        ScanStatus::Complete
    } else if categories
        .iter()
        .all(|c| c.scan_status == ScanStatus::Unavailable)
    {
        ScanStatus::Unavailable
    } else {
        ScanStatus::Partial
    };
    let mut warnings = Vec::new();
    let config_label = match config {
        ConfigState::Missing => "missing",
        ConfigState::Loaded(_) => "ok",
        ConfigState::Corrupt { path, reason } => {
            warnings.push(format!(
                "{} está corrupto ({reason}): no se aplican exclusiones y no se podrá limpiar",
                path.display()
            ));
            "corrupt"
        }
    };
    ScanReport {
        scan_status,
        catalog_version: res.catalog_version.clone(),
        categories,
        config: config_label.into(),
        warnings,
    }
}
