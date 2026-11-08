//! Resumen del escaneo por categoría (`rotten scan`).
//!
//! Tamaños honestos (RF-SEG-12, RNF-ARQ-28, RF-PAN-04):
//! - `bytes_allocated` y `preselected_bytes` suman `st_blocks × 512`, cada
//!   inode una sola vez también **entre** elementos (un enlace duro presente
//!   en dos elementos cuenta una vez).
//! - `reclaimable_bytes` y `preselected_reclaimable_bytes` estiman lo que se
//!   liberaría: `ATTR_CMNEXT_PRIVATESIZE`, que no cuenta lo atrapado en
//!   clones APFS ni en instantáneas. Una familia de clones completos suma su
//!   tamaño una vez si todos sus miembros están en la selección, y nada si
//!   alguno queda fuera (sus bloques siguen ocupados).
//!
//! El estado del volumen (RF-PAN-01) va aparte, en [`VolumeState`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use ra_catalog::{Catalog, Requirement, RootState, ScanResult};
use ra_safety::{ConfigState, Exclusions, Origin, ProtectedList, TreeFingerprint};
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
    /// Lo que se liberaría al borrar los candidatos (tamaño privado).
    pub reclaimable_bytes: u64,
    /// Lo mismo, solo de los preseleccionados.
    pub preselected_reclaimable_bytes: u64,
    /// Ficheros de los candidatos que pueden compartir bloques con un clon.
    pub cloned_entries: u64,
    /// Ficheros de los candidatos con más de un enlace duro.
    pub hardlinked_entries: u64,
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
    /// Estado del volumen de la carpeta personal; `None` si no se pudo leer.
    pub volume: Option<VolumeState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Espacio del volumen en bytes (RF-PAN-01, 05 §1.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VolumeState {
    pub capacity_bytes: u64,
    /// `capacity − free`: incluye lo purgable.
    pub used_bytes: u64,
    /// Libre sin contar lo purgable (`statfs`), como «libre» en `diskutil`.
    pub free_bytes: u64,
    /// Lo que macOS puede liberar por su cuenta: `available − free`.
    pub purgeable_bytes: Option<u64>,
    /// Disponible para uso importante, la cifra de Finder (libre más
    /// purgable). `None` si el sistema no la da o no es coherente.
    pub available_bytes: Option<u64>,
}

impl VolumeState {
    fn from_space(v: ra_platform::attrs::VolumeSpace) -> Self {
        // Lo disponible nunca es menor que lo libre; si lo es (p. ej. en un
        // sandbox sin acceso al servicio que lo calcula, que da 0), no vale.
        let available = v.available_important.filter(|&a| a >= v.free);
        VolumeState {
            capacity_bytes: v.capacity,
            used_bytes: v.capacity.saturating_sub(v.free),
            free_bytes: v.free,
            purgeable_bytes: available.map(|a| a - v.free),
            available_bytes: available,
        }
    }
}

/// Añade al informe el estado del volumen que contiene `home`.
pub(crate) fn add_volume(report: &mut ScanReport, home: &Path) {
    match ra_platform::attrs::volume_space(home) {
        Ok(v) => report.volume = Some(VolumeState::from_space(v)),
        Err(e) => report
            .warnings
            .push(format!("no se pudo leer el estado del volumen: {e}")),
    }
}

/// Suma bytes una vez por inode entre elementos y cada familia de clones
/// completos una vez, solo si todos sus miembros están dentro.
#[derive(Default)]
struct Tally {
    seen: HashSet<(u64, u64)>,
    /// Por `(dev, clone_id)`.
    families: HashMap<(u64, u64), Family>,
}

struct Family {
    /// Categoría del primer miembro visto: ahí se suma.
    cat: String,
    members: u32,
    seen: u32,
    allocated: u64,
    private: u64,
}

impl Tally {
    /// `(asignado, liberable)` que aporta el elemento sin repetir inodes.
    fn add(&mut self, cat: &str, fp: &TreeFingerprint) -> (u64, u64) {
        let (mut allocated, mut private) = (fp.bytes_allocated, fp.size.bytes_private);
        for s in &fp.size.shared {
            if !self.seen.insert((s.dev, s.ino)) {
                allocated = allocated.saturating_sub(s.allocated);
                private = private.saturating_sub(s.private);
                continue;
            }
            if let Some(c) = s.clone {
                let f = self
                    .families
                    .entry((s.dev, c.id))
                    .or_insert_with(|| Family {
                        cat: cat.to_owned(),
                        members: c.members,
                        seen: 0,
                        allocated: 0,
                        private: 0,
                    });
                f.seen += 1;
                f.allocated = f.allocated.max(s.allocated);
                f.private += s.private;
            }
        }
        (allocated, private)
    }

    /// `(categoría, bytes)` de cada familia de clones completa.
    fn complete_families(self) -> impl Iterator<Item = (String, u64)> {
        self.families
            .into_values()
            .filter(|f| f.seen >= f.members)
            .map(|f| (f.cat, f.allocated.saturating_sub(f.private)))
    }
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
                reclaimable_bytes: 0,
                preselected_reclaimable_bytes: 0,
                cloned_entries: 0,
                hardlinked_entries: 0,
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

    let (mut all, mut pre) = (Tally::default(), Tally::default());
    for c in &res.candidates {
        let Some(sum) = cats.get_mut(&c.cat) else {
            continue;
        };
        let excluded = exclusions.is_some_and(|ex| ex.check(&c.path, &c.rule_id, &c.cat).is_some());
        if c.block.is_some() || protected.check(&c.path, Origin::Rules).is_err() || excluded {
            sum.skipped_items += 1;
            continue;
        }
        sum.items += 1;
        if c.preselected {
            sum.preselected_items += 1;
        }
        let Some(fp) = &c.fingerprint else {
            continue;
        };
        let (allocated, private) = all.add(&c.cat, fp);
        sum.bytes_allocated += allocated;
        sum.reclaimable_bytes += private;
        sum.cloned_entries += fp.size.cloned_entries;
        sum.hardlinked_entries += fp.size.hardlinked_entries;
        if c.preselected {
            let (allocated, private) = pre.add(&c.cat, fp);
            sum.preselected_bytes += allocated;
            sum.preselected_reclaimable_bytes += private;
        }
    }
    for (cat, bytes) in all.complete_families() {
        if let Some(sum) = cats.get_mut(&cat) {
            sum.reclaimable_bytes += bytes;
        }
    }
    for (cat, bytes) in pre.complete_families() {
        if let Some(sum) = cats.get_mut(&cat) {
            sum.preselected_reclaimable_bytes += bytes;
        }
    }
    for sum in cats.values_mut().filter(|s| s.cloned_entries > 0) {
        sum.notes.push(format!(
            "{} ficheros comparten bloques con clones APFS: borrarlos libera menos de lo que ocupan",
            sum.cloned_entries
        ));
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
        volume: None,
        warnings,
    }
}
