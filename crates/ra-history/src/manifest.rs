//! Manifiesto de deshacer, historial y preparación del deshacer.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use ra_safety::fingerprint::stat_nofollow_any;
use ra_safety::{Identity, Origin};
use serde::{Deserialize, Serialize};

use crate::HistoryPaths;
use crate::journal::{Event, ItemRef, OpKind, OpMode, OpStatus, Record, StepAction};

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ItemState {
    /// Hay intención pero no resultado (operación interrumpida).
    Pending,
    Trashed,
    Deleted,
    Skipped {
        reason: String,
    },
    Failed {
        error: String,
    },
    Restored {
        to: PathBuf,
        by: String,
    },
    /// Reconciliado tras un cierre abrupto sin poder saber dónde está.
    Unknown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestItem {
    pub item_id: u32,
    pub action: StepAction,
    pub original_path: PathBuf,
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub regenerable: bool,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_mode: Option<u32>,
    /// Origen del elemento (reglas o selección explícita). Falta en los
    /// journals anteriores a T-10 y en las restauraciones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Cuándo quedó en la Papelera, según el journal (RFC 3339, ms): el
    /// `ts` del `item_done` que lo movió o, si se reconcilió tras un corte,
    /// el de su intención (antes del movimiento: del lado seguro).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_at: Option<String>,
    #[serde(flatten)]
    pub state: ItemState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub op_id: String,
    pub kind: OpKind,
    pub mode: OpMode,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<OpStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<String>,
    pub by: String,
    pub trash: String,
    pub items: Vec<ManifestItem>,
}

/// Estado de deshacer de una operación (RF-HIS-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoState {
    /// Todo lo movido sigue en la Papelera.
    Restorable,
    /// Una parte aún se puede restaurar.
    Partial,
    /// Nada se puede restaurar ni se restauró.
    Irreversible,
    /// Todo se restauró.
    Restored,
    /// Se restauró una parte y el resto ya no está (CU-05: «restaurada
    /// total o parcialmente»).
    PartlyRestored,
    NotApplicable,
}

/// Qué fue cada operación del historial. El journal distingue limpieza,
/// deshacer y liberar (`OpKind`); un `trash put` se reconoce por el origen
/// de sus elementos (selección explícita).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    /// Limpieza con las reglas del catálogo (`rotten clean`).
    Clean,
    /// Rutas elegidas a mano (`rotten trash put`).
    TrashPut,
    /// Deshacer de otra operación.
    Undo,
    /// «Liberar ahora» (`rotten free`): borrado permanente de la Papelera.
    Free,
    /// Vaciar la Papelera (`rotten trash empty`).
    TrashEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpSummary {
    pub op_id: String,
    pub kind: HistoryKind,
    pub mode: OpMode,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<OpStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<String>,
    pub by: String,
    pub items_done: u64,
    pub items_skipped: u64,
    pub items_failed: u64,
    /// Bytes movidos a la Papelera (o borrados). Movido ≠ liberado.
    pub bytes_done: u64,
    pub restorable_items: u64,
    pub undo_state: UndoState,
}

impl Manifest {
    /// Escribe `manifests/<op>.json` de forma atómica.
    pub fn save(&self, paths: &HistoryPaths) -> io::Result<()> {
        fs::create_dir_all(&paths.manifests_dir)?;
        let path = paths.manifests_dir.join(format!("{}.json", self.op_id));
        let tmp = paths
            .manifests_dir
            .join(format!(".{}.json.tmp", self.op_id));
        let text = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }

    /// ¿Sigue el elemento en la Papelera tal cual se movió?
    pub fn item_restorable(item: &ManifestItem) -> bool {
        let (ItemState::Trashed, Some(trashed)) = (&item.state, &item.trashed_path) else {
            return false;
        };
        stat_nofollow_any(trashed).is_ok_and(|(id, _)| id.same_after_move(&item.identity))
    }

    pub fn undo_state(&self) -> UndoState {
        if self.kind != OpKind::Clean {
            return UndoState::NotApplicable;
        }
        if self.mode == OpMode::Permanent {
            return if self.items.iter().any(|i| i.state == ItemState::Deleted) {
                UndoState::Irreversible
            } else {
                UndoState::NotApplicable
            };
        }
        let mut restorable = 0;
        let mut gone = 0;
        let mut restored = 0;
        for i in &self.items {
            match &i.state {
                ItemState::Trashed if Self::item_restorable(i) => restorable += 1,
                // Borrado de la Papelera después (`rotten free`): ya no
                // se puede restaurar.
                ItemState::Trashed | ItemState::Unknown { .. } | ItemState::Deleted => gone += 1,
                ItemState::Restored { .. } => restored += 1,
                _ => {}
            }
        }
        match (restorable, gone, restored) {
            (0, 0, 0) => UndoState::NotApplicable,
            (r, 0, 0) if r > 0 => UndoState::Restorable,
            (0, 0, _) => UndoState::Restored,
            (r, _, _) if r > 0 => UndoState::Partial,
            (0, _, x) if x > 0 => UndoState::PartlyRestored,
            _ => UndoState::Irreversible,
        }
    }

    fn summary(&self) -> OpSummary {
        let mut done = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut bytes = 0;
        for i in &self.items {
            match &i.state {
                ItemState::Trashed | ItemState::Deleted | ItemState::Restored { .. } => {
                    done += 1;
                    bytes += i.bytes;
                }
                ItemState::Skipped { .. } => skipped += 1,
                ItemState::Failed { .. } => failed += 1,
                ItemState::Pending | ItemState::Unknown { .. } => {}
            }
        }
        let kind = match self.kind {
            OpKind::Undo => HistoryKind::Undo,
            OpKind::Free => HistoryKind::Free,
            OpKind::TrashEmpty => HistoryKind::TrashEmpty,
            OpKind::Clean
                if self
                    .items
                    .iter()
                    .any(|i| i.origin == Some(Origin::ExplicitSelection)) =>
            {
                HistoryKind::TrashPut
            }
            OpKind::Clean => HistoryKind::Clean,
        };
        OpSummary {
            op_id: self.op_id.clone(),
            kind,
            mode: self.mode,
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.clone(),
            status: self.status,
            undo_of: self.undo_of.clone(),
            by: self.by.clone(),
            items_done: done,
            items_skipped: skipped,
            items_failed: failed,
            bytes_done: bytes,
            restorable_items: self
                .items
                .iter()
                .filter(|i| Self::item_restorable(i))
                .count() as u64,
            undo_state: self.undo_state(),
        }
    }
}

/// Proyecta el manifiesto de `op` a partir del journal, incluidos los
/// deshaceres posteriores que la referencian.
pub fn manifest_for(records: &[Record], op: &str) -> Option<Manifest> {
    let mut m: Option<Manifest> = None;
    let mut items: BTreeMap<u32, ManifestItem> = BTreeMap::new();
    for r in records.iter().filter(|r| r.op == op) {
        match &r.event {
            Event::OpStarted {
                kind,
                mode,
                plan_hash,
                undo_of,
                by,
                trash,
                ..
            } => {
                m = Some(Manifest {
                    schema_version: MANIFEST_SCHEMA_VERSION,
                    op_id: op.to_owned(),
                    kind: *kind,
                    mode: *mode,
                    started_at: r.ts.clone(),
                    finished_at: None,
                    status: None,
                    plan_hash: plan_hash.clone(),
                    undo_of: undo_of.clone(),
                    by: by.clone(),
                    trash: trash.clone(),
                    items: vec![],
                });
            }
            Event::ItemIntent {
                item,
                action,
                path,
                identity,
                category,
                regenerable,
                bytes,
                parent_mode,
                origin,
                ..
            } => {
                items.insert(
                    *item,
                    ManifestItem {
                        item_id: *item,
                        action: *action,
                        original_path: path.clone(),
                        identity: *identity,
                        trashed_path: None,
                        category: category.clone(),
                        regenerable: *regenerable,
                        bytes: *bytes,
                        parent_mode: *parent_mode,
                        origin: *origin,
                        moved_at: Some(r.ts.clone()),
                        state: ItemState::Pending,
                    },
                );
            }
            Event::ItemDone {
                item, result_path, ..
            } => {
                if let Some(i) = items.get_mut(item) {
                    i.state = match i.action {
                        StepAction::Trash => {
                            i.trashed_path = result_path.clone();
                            i.moved_at = Some(r.ts.clone());
                            ItemState::Trashed
                        }
                        StepAction::Delete => ItemState::Deleted,
                        StepAction::Restore => ItemState::Restored {
                            to: result_path.clone().unwrap_or_default(),
                            by: op.to_owned(),
                        },
                    };
                }
            }
            Event::ItemSkipped { item, reason, .. } => {
                if let Some(i) = items.get_mut(item) {
                    i.state = ItemState::Skipped {
                        reason: reason.clone(),
                    };
                }
            }
            Event::ItemFailed { item, error } => {
                if let Some(i) = items.get_mut(item) {
                    i.state = ItemState::Failed {
                        error: error.clone(),
                    };
                }
            }
            Event::ItemReconciled {
                item,
                state,
                result_path,
                detail,
            } => {
                if let Some(i) = items.get_mut(item) {
                    i.state = match (state.as_str(), result_path) {
                        ("not_moved", _) => ItemState::Skipped {
                            reason: "interrupted_before_action".into(),
                        },
                        ("trashed", _) => {
                            i.trashed_path = result_path.clone();
                            ItemState::Trashed
                        }
                        ("deleted", _) => ItemState::Deleted,
                        ("restored", Some(to)) => ItemState::Restored {
                            to: to.clone(),
                            by: op.to_owned(),
                        },
                        _ => ItemState::Unknown {
                            detail: detail.clone(),
                        },
                    };
                }
            }
            Event::OpFinished { status, .. } => {
                if let Some(m) = m.as_mut() {
                    m.finished_at = Some(r.ts.clone());
                    m.status = Some(*status);
                }
            }
        }
    }
    let mut m = m?;
    if m.kind == OpKind::Clean {
        // Deshaceres posteriores de esta operación.
        let undo_ops: Vec<&str> = records
            .iter()
            .filter_map(|r| match &r.event {
                Event::OpStarted {
                    kind: OpKind::Undo,
                    undo_of: Some(of),
                    ..
                } if of == op => Some(r.op.as_str()),
                _ => None,
            })
            .collect();
        // Liberaciones posteriores (`rotten free`): cada intención dice qué
        // elemento de esta operación borra de la Papelera.
        let freed_by: BTreeMap<(&str, u32), u32> = records
            .iter()
            .filter_map(|r| match &r.event {
                Event::ItemIntent {
                    item, of: Some(of), ..
                } if of.op == op => Some(((r.op.as_str(), *item), of.item)),
                _ => None,
            })
            .collect();
        for r in records.iter() {
            // Lo borrado de la Papelera queda borrado; una carpeta cuyo
            // borrado se cortó a medias, en paradero incierto (le falta
            // parte). Si la recuperación no pudo comprobarlo, no cambia.
            let freed = match &r.event {
                Event::ItemDone { item, .. } => freed_by
                    .get(&(r.op.as_str(), *item))
                    .map(|of| (of, ItemState::Deleted)),
                Event::ItemReconciled {
                    item,
                    state,
                    detail,
                    ..
                } => freed_by
                    .get(&(r.op.as_str(), *item))
                    .and_then(|of| match state.as_str() {
                        "deleted" => Some((of, ItemState::Deleted)),
                        "partial" => Some((
                            of,
                            ItemState::Unknown {
                                detail: detail.clone(),
                            },
                        )),
                        _ => None,
                    }),
                _ => None,
            };
            if let Some((of, state)) = freed
                && let Some(i) = items.get_mut(of)
            {
                i.state = state;
                continue;
            }
            if !undo_ops.contains(&r.op.as_str()) {
                continue;
            }
            let restored = match &r.event {
                Event::ItemDone {
                    item,
                    result_path: Some(to),
                    ..
                } => Some((item, to)),
                Event::ItemReconciled {
                    item,
                    state,
                    result_path: Some(to),
                    ..
                } if state == "restored" => Some((item, to)),
                _ => None,
            };
            if let Some((item, to)) = restored
                && let Some(i) = items.get_mut(item)
            {
                i.state = ItemState::Restored {
                    to: to.clone(),
                    by: r.op.clone(),
                };
            }
        }
    }
    m.items = items.into_values().collect();
    Some(m)
}

/// Operaciones del journal, de la más reciente a la más antigua.
pub fn list_ops(records: &[Record]) -> Vec<OpSummary> {
    let mut ops: Vec<&str> = Vec::new();
    for r in records {
        if matches!(r.event, Event::OpStarted { .. }) && !ops.contains(&r.op.as_str()) {
            ops.push(&r.op);
        }
    }
    ops.iter()
        .rev()
        .filter_map(|op| manifest_for(records, op).map(|m| m.summary()))
        .collect()
}

/// Un elemento listo para restaurar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoItem {
    pub item_id: u32,
    /// Dónde está ahora (en la Papelera).
    pub from: PathBuf,
    /// Ruta original.
    pub to: PathBuf,
    pub identity: Identity,
    pub regenerable: bool,
    pub bytes: u64,
    pub parent_mode: Option<u32>,
}

/// Por qué un elemento ya no se puede restaurar. El JSON lleva el código
/// (`reason`), no un texto: el CLI lo traduce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum NotRecoverableReason {
    /// Ya no está en la Papelera, o ha cambiado.
    #[error("it is no longer in the Trash or it has changed")]
    NotInTrash,
    #[error("it was deleted permanently")]
    Deleted,
    #[error("it was already restored")]
    AlreadyRestored,
    /// Paradero desconocido tras un cierre abrupto (con el motivo, si se
    /// sabe: p. ej. la Papelera no se pudo leer).
    #[error("whereabouts unknown after an abrupt shutdown{}", detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
    UnknownWhereabouts {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// No se llegó a mover (omitido, fallido o interrumpido antes).
    #[error("it was never moved")]
    NeverMoved,
    /// La operación lo mandó a otra Papelera que la que se usa ahora
    /// (otro `ROTTEN_TRASH_DIR`, o un journal manipulado): no se toca.
    #[error("it was sent to a different Trash than the one in use")]
    OtherTrash,
    /// La carpeta de la Papelera donde quedó no existe (volumen sin montar).
    #[error("its Trash is not available (is the volume mounted?)")]
    VolumeNotMounted,
}

impl NotRecoverableReason {
    /// Para liberar: ya no queda nada que hacer con él (se borró, se
    /// restauró o nunca llegó a la Papelera). Los demás motivos dejan algo
    /// sin liberar (código 4).
    pub fn is_settled(&self) -> bool {
        matches!(
            self,
            Self::Deleted | Self::AlreadyRestored | Self::NeverMoved
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotRecoverable {
    pub item_id: u32,
    #[serde(flatten)]
    pub reason: NotRecoverableReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoPlan {
    pub undo_of: String,
    pub ready: Vec<UndoItem>,
    /// Elementos pedidos que ya no se pueden restaurar, con el motivo.
    pub not_recoverable: Vec<NotRecoverable>,
}

/// Por qué no se puede preparar un deshacer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UndoError {
    #[error("operation {op} does not exist")]
    UnknownOperation { op: String },
    /// Un deshacer no se deshace.
    #[error("operation {op} cannot be undone")]
    NotUndoable { op: String },
    #[error("operation {op} has no item {item}")]
    UnknownItem { op: String, item: u32 },
}

/// Prepara el deshacer de `op`, entero (`items = None`) o por elementos.
pub fn prepare_undo(
    records: &[Record],
    op: &str,
    items: Option<&[u32]>,
) -> Result<UndoPlan, UndoError> {
    let m =
        manifest_for(records, op).ok_or_else(|| UndoError::UnknownOperation { op: op.into() })?;
    // Un deshacer no se deshace, y una operación de la que todo lo que se
    // movió se borró después (de la Papelera, o era un borrado permanente)
    // tampoco. Si algo está en paradero incierto o ya no está en la
    // Papelera, se prepara el plan para decir el motivo de cada elemento.
    let all_deleted = m.undo_state() == UndoState::Irreversible
        && m.items
            .iter()
            .all(|i| !matches!(i.state, ItemState::Trashed | ItemState::Unknown { .. }));
    if m.kind != OpKind::Clean || all_deleted {
        return Err(UndoError::NotUndoable { op: op.into() });
    }
    if let Some(ids) = items {
        for id in ids {
            if !m.items.iter().any(|i| i.item_id == *id) {
                return Err(UndoError::UnknownItem {
                    op: op.into(),
                    item: *id,
                });
            }
        }
    }
    let mut plan = UndoPlan {
        undo_of: op.to_owned(),
        ready: vec![],
        not_recoverable: vec![],
    };
    for i in &m.items {
        if items.is_some_and(|ids| !ids.contains(&i.item_id)) {
            continue;
        }
        match &i.state {
            ItemState::Trashed if Manifest::item_restorable(i) => {
                if let Some(from) = &i.trashed_path {
                    plan.ready.push(UndoItem {
                        item_id: i.item_id,
                        from: from.clone(),
                        to: i.original_path.clone(),
                        identity: i.identity,
                        regenerable: i.regenerable,
                        bytes: i.bytes,
                        parent_mode: i.parent_mode,
                    });
                }
            }
            ItemState::Trashed => plan.not_recoverable.push(NotRecoverable {
                item_id: i.item_id,
                reason: NotRecoverableReason::NotInTrash,
            }),
            ItemState::Deleted => plan.not_recoverable.push(NotRecoverable {
                item_id: i.item_id,
                reason: NotRecoverableReason::Deleted,
            }),
            ItemState::Restored { .. } => plan.not_recoverable.push(NotRecoverable {
                item_id: i.item_id,
                reason: NotRecoverableReason::AlreadyRestored,
            }),
            ItemState::Unknown { detail } => plan.not_recoverable.push(NotRecoverable {
                item_id: i.item_id,
                reason: NotRecoverableReason::UnknownWhereabouts {
                    detail: detail.clone(),
                },
            }),
            ItemState::Pending | ItemState::Skipped { .. } | ItemState::Failed { .. } => {
                // No se movió: no hay nada que deshacer. Solo se informa si se pidió.
                if items.is_some() {
                    plan.not_recoverable.push(NotRecoverable {
                        item_id: i.item_id,
                        reason: NotRecoverableReason::NeverMoved,
                    });
                }
            }
        }
    }
    Ok(plan)
}

/// Un elemento que `rotten free` borraría de la Papelera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeItem {
    /// Número del elemento en la operación de liberar (1, 2…).
    pub item_id: u32,
    /// La operación y el elemento que lo mandaron a la Papelera.
    pub of: ItemRef,
    /// Dónde está en la Papelera, según el journal.
    pub path: PathBuf,
    pub original_path: PathBuf,
    /// La identidad anotada al mandarlo a la Papelera: se revalida con
    /// `same_after_move` justo antes de borrarlo.
    pub identity: Identity,
    /// Tamaño estimado (asignado) anotado en el journal.
    pub bytes: u64,
    /// Papelera que usó la operación: `system` o `dir:<ruta>`.
    pub trash: String,
    /// Cuándo quedó en la Papelera ([`ManifestItem::moved_at`]). Si es una
    /// carpeta, nada de dentro puede haber cambiado después.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_at: Option<String>,
}

impl FreeItem {
    /// [`FreeItem::moved_at`] como instante; `None` si falta o no se
    /// entiende (quien lo use debe omitir el elemento si es una carpeta).
    pub fn moved_at_time(&self) -> Option<std::time::SystemTime> {
        humantime::parse_rfc3339(self.moved_at.as_deref()?).ok()
    }
}

/// Elemento de una operación pedida que no se puede liberar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotFreeable {
    pub op_id: String,
    pub item_id: u32,
    #[serde(flatten)]
    pub reason: NotRecoverableReason,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeTotals {
    pub items: u64,
    pub bytes: u64,
}

/// Plan de `rotten free`: sale solo del journal. Su hash cubre las
/// operaciones, los elementos listos y los que no se pueden liberar; al
/// ejecutar se vuelve a preparar desde el journal y se compara.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreePlan {
    pub ops: Vec<String>,
    pub ready: Vec<FreeItem>,
    pub not_freeable: Vec<NotFreeable>,
    pub totals: FreeTotals,
    pub plan_hash: String,
}

impl FreePlan {
    /// Hash BLAKE3 del plan sin el campo `plan_hash`.
    pub fn compute_hash(&self) -> String {
        let body = (&self.ops, &self.ready, &self.not_freeable, &self.totals);
        let text = serde_json::to_string(&body).unwrap_or_default();
        format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex())
    }
}

/// Elemento que, según el journal, una operación dejó en la Papelera y
/// sigue allí (estado `Trashed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashedItem {
    pub of: ItemRef,
    /// Dónde quedó en la Papelera.
    pub path: PathBuf,
    pub original_path: PathBuf,
    pub identity: Identity,
    /// Tamaño anotado en el journal.
    pub bytes: u64,
    /// Papelera que usó la operación: `system` o `dir:<ruta>`.
    pub trash: String,
}

/// Todo lo que el journal da por movido a la Papelera y aún allí, de todas
/// las operaciones que mandaron cosas a la Papelera, en orden del journal.
pub fn trashed_items(records: &[Record]) -> Vec<TrashedItem> {
    let mut ops: Vec<&str> = Vec::new();
    for r in records {
        if let Event::OpStarted {
            kind: OpKind::Clean,
            mode: OpMode::Trash,
            ..
        } = &r.event
            && !ops.contains(&r.op.as_str())
        {
            ops.push(&r.op);
        }
    }
    let mut out = Vec::new();
    for op in ops {
        let Some(m) = manifest_for(records, op) else {
            continue;
        };
        for i in m.items {
            if let (ItemState::Trashed, Some(path)) = (&i.state, i.trashed_path) {
                out.push(TrashedItem {
                    of: ItemRef {
                        op: op.to_owned(),
                        item: i.item_id,
                    },
                    path,
                    original_path: i.original_path,
                    identity: i.identity,
                    bytes: i.bytes,
                    trash: m.trash.clone(),
                });
            }
        }
    }
    out
}

/// Por qué no se puede preparar una liberación.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FreeError {
    #[error("give at least one operation")]
    NoOperations,
    #[error("operation {op} does not exist")]
    UnknownOperation { op: String },
    /// Solo se liberan operaciones que mandaron cosas a la Papelera.
    #[error("operation {op} did not move anything to the Trash")]
    NotFreeable { op: String },
}

/// Prepara la liberación de `ops`: cada elemento que el journal da por
/// movido a la Papelera y que acepta `in_trash` (que su Papelera es la que
/// se usa y que está en su primer nivel); si no lo acepta, devuelve el
/// motivo. Nunca entra nada que no esté registrado para esas operaciones.
pub fn prepare_free(
    records: &[Record],
    ops: &[String],
    in_trash: impl Fn(&FreeItem) -> Result<(), NotRecoverableReason>,
) -> Result<FreePlan, FreeError> {
    let mut unique: Vec<String> = Vec::new();
    for op in ops {
        if !unique.contains(op) {
            unique.push(op.clone());
        }
    }
    if unique.is_empty() {
        return Err(FreeError::NoOperations);
    }
    let mut plan = FreePlan {
        ops: unique.clone(),
        ready: vec![],
        not_freeable: vec![],
        totals: FreeTotals::default(),
        plan_hash: String::new(),
    };
    for op in &unique {
        let m = manifest_for(records, op)
            .ok_or_else(|| FreeError::UnknownOperation { op: op.clone() })?;
        if m.kind != OpKind::Clean || m.mode != OpMode::Trash {
            return Err(FreeError::NotFreeable { op: op.clone() });
        }
        for i in &m.items {
            let reason = match (&i.state, &i.trashed_path) {
                (ItemState::Trashed, Some(path)) => {
                    let item = FreeItem {
                        item_id: plan.ready.len() as u32 + 1,
                        of: ItemRef {
                            op: op.clone(),
                            item: i.item_id,
                        },
                        path: path.clone(),
                        original_path: i.original_path.clone(),
                        identity: i.identity,
                        bytes: i.bytes,
                        trash: m.trash.clone(),
                        moved_at: i.moved_at.clone(),
                    };
                    match in_trash(&item) {
                        Ok(()) => {
                            plan.totals.items += 1;
                            plan.totals.bytes += item.bytes;
                            plan.ready.push(item);
                            continue;
                        }
                        Err(reason) => reason,
                    }
                }
                (ItemState::Trashed, None) => NotRecoverableReason::NotInTrash,
                (ItemState::Deleted, _) => NotRecoverableReason::Deleted,
                (ItemState::Restored { .. }, _) => NotRecoverableReason::AlreadyRestored,
                (ItemState::Unknown { detail }, _) => NotRecoverableReason::UnknownWhereabouts {
                    detail: detail.clone(),
                },
                // No llegó a la Papelera: no hay nada que liberar ni que contar.
                (ItemState::Pending | ItemState::Skipped { .. } | ItemState::Failed { .. }, _) => {
                    continue;
                }
            };
            plan.not_freeable.push(NotFreeable {
                op_id: op.clone(),
                item_id: i.item_id,
                reason,
            });
        }
    }
    plan.plan_hash = plan.compute_hash();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Journal;
    use std::time::SystemTime;

    fn ident(p: &std::path::Path) -> Identity {
        Identity::from_metadata(&fs::symlink_metadata(p).expect("stat"))
    }

    fn intent(j: &mut Journal, item: u32, path: &std::path::Path) {
        j.write(
            Event::ItemIntent {
                item,
                action: StepAction::Trash,
                path: path.to_path_buf(),
                identity: ident(path),
                target: None,
                category: Some("CAT-03".into()),
                regenerable: false,
                bytes: 1,
                parent_mode: None,
                origin: None,
                of: None,
            },
            true,
        )
        .expect("intent");
    }

    #[test]
    fn projection_tracks_trash_undo_and_trash_emptying() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(dir.path()).expect("canon");
        let paths = HistoryPaths::new(&base.join("state"));
        let trash = base.join("trash");
        fs::create_dir_all(&trash).expect("mkdir");
        let a = base.join("a.log");
        let b = base.join("b.log");
        fs::write(&a, b"a").expect("write");
        fs::write(&b, b"b").expect("write");

        let mut j = Journal::open(&paths, "op-clean", SystemTime::now()).expect("open");
        j.write(
            Event::OpStarted {
                kind: OpKind::Clean,
                mode: OpMode::Trash,
                plan_hash: Some("blake3:x".into()),
                undo_of: None,
                items: 2,
                bytes: 2,
                by: "test".into(),
                trash: "dir".into(),
            },
            true,
        )
        .expect("start");
        for (n, p) in [(1, &a), (2, &b)] {
            intent(&mut j, n, p);
            let to = trash.join(p.file_name().expect("nombre"));
            fs::rename(p, &to).expect("rename");
            j.write(
                Event::ItemDone {
                    item: n,
                    result_path: Some(to),
                    post_check: None,
                },
                true,
            )
            .expect("done");
        }
        j.write(
            Event::OpFinished {
                status: OpStatus::Completed,
                done: 2,
                skipped: 0,
                failed: 0,
                unknown: 0,
            },
            true,
        )
        .expect("finish");

        let (recs, _) = crate::read_journal(&paths).expect("read");
        let ops = list_ops(&recs);
        assert_eq!(ops[0].undo_state, UndoState::Restorable);
        assert_eq!(ops[0].bytes_done, 2);

        // Deshacer solo el elemento 1.
        let plan = prepare_undo(&recs, "op-clean", Some(&[1])).expect("plan");
        assert_eq!(plan.ready.len(), 1);
        let mut u = Journal::open(&paths, "op-undo", SystemTime::now()).expect("open");
        u.write(
            Event::OpStarted {
                kind: OpKind::Undo,
                mode: OpMode::Restore,
                plan_hash: None,
                undo_of: Some("op-clean".into()),
                items: 1,
                bytes: 1,
                by: "test".into(),
                trash: "dir".into(),
            },
            true,
        )
        .expect("start");
        fs::rename(&plan.ready[0].from, &plan.ready[0].to).expect("restore");
        u.write(
            Event::ItemDone {
                item: 1,
                result_path: Some(a.clone()),
                post_check: None,
            },
            true,
        )
        .expect("done");
        let (recs, _) = crate::read_journal(&paths).expect("read");
        let ops = list_ops(&recs);
        let clean = ops.iter().find(|o| o.op_id == "op-clean").expect("op");
        assert_eq!(clean.undo_state, UndoState::Partial);

        // «Vaciar la Papelera»: el elemento 2 ya no es recuperable.
        fs::remove_file(trash.join("b.log")).expect("rm");
        let ops = list_ops(&recs);
        let clean = ops.iter().find(|o| o.op_id == "op-clean").expect("op");
        assert_eq!(clean.undo_state, UndoState::PartlyRestored);
        let plan = prepare_undo(&recs, "op-clean", None).expect("plan");
        assert!(plan.ready.is_empty());
        assert_eq!(plan.not_recoverable.len(), 2);

        // El manifiesto se guarda y se relee igual.
        let m = manifest_for(&recs, "op-clean").expect("manifest");
        m.save(&paths).expect("save");
        let text = fs::read_to_string(paths.manifests_dir.join("op-clean.json")).expect("read");
        let back: Manifest = serde_json::from_str(&text).expect("json");
        assert_eq!(back, m);
    }

    #[test]
    fn unknown_ops_and_items_are_errors() {
        assert!(prepare_undo(&[], "nada", None).is_err());
        assert_eq!(
            prepare_free(&[], &["nada".into()], |_| Ok(())),
            Err(FreeError::UnknownOperation { op: "nada".into() })
        );
        assert_eq!(
            prepare_free(&[], &[], |_| Ok(())),
            Err(FreeError::NoOperations)
        );
    }

    fn start(j: &mut Journal, kind: OpKind, mode: OpMode, items: u64) {
        j.write(
            Event::OpStarted {
                kind,
                mode,
                plan_hash: None,
                undo_of: None,
                items,
                bytes: items,
                by: "test".into(),
                trash: "dir:/t".into(),
            },
            true,
        )
        .expect("start");
    }

    /// T-11: una liberación marca como borrados los elementos de la
    /// operación original, que pasa a «irreversible» (o a «restaurada en
    /// parte» si antes se restauró algo), y `prepare_free` solo trae lo que
    /// sigue anotado en la Papelera.
    #[test]
    fn free_projection_marks_source_items_deleted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(dir.path()).expect("canon");
        let paths = HistoryPaths::new(&base.join("state"));
        let trash = base.join("trash");
        fs::create_dir_all(&trash).expect("mkdir");
        let files: Vec<_> = ["a.log", "b.log", "c.log"]
            .iter()
            .map(|n| {
                let p = base.join(n);
                fs::write(&p, n.as_bytes()).expect("write");
                p
            })
            .collect();

        let mut j = Journal::open(&paths, "op-clean", SystemTime::now()).expect("open");
        start(&mut j, OpKind::Clean, OpMode::Trash, 3);
        for (n, p) in files.iter().enumerate() {
            let n = n as u32 + 1;
            intent(&mut j, n, p);
            let to = trash.join(p.file_name().expect("nombre"));
            fs::rename(p, &to).expect("rename");
            j.write(
                Event::ItemDone {
                    item: n,
                    result_path: Some(to),
                    post_check: None,
                },
                true,
            )
            .expect("done");
        }
        // El elemento 3 se restaura.
        let mut u = Journal::open(&paths, "op-undo", SystemTime::now()).expect("open");
        u.write(
            Event::OpStarted {
                kind: OpKind::Undo,
                mode: OpMode::Restore,
                plan_hash: None,
                undo_of: Some("op-clean".into()),
                items: 1,
                bytes: 1,
                by: "test".into(),
                trash: "dir:/t".into(),
            },
            true,
        )
        .expect("start");
        u.write(
            Event::ItemDone {
                item: 3,
                result_path: Some(files[2].clone()),
                post_check: None,
            },
            true,
        )
        .expect("done");

        let (recs, _) = crate::read_journal(&paths).expect("read");
        // Solo lo que acepta el filtro (la Papelera de la operación) entra.
        let plan = prepare_free(&recs, &["op-clean".into(), "op-clean".into()], |i| {
            if i.item_id == 1 || i.of.item == 2 {
                Ok(())
            } else {
                Err(NotRecoverableReason::NotInTrash)
            }
        })
        .expect("plan");
        assert_eq!(plan.ops, vec!["op-clean"]);
        assert_eq!(plan.ready.len(), 2);
        assert_eq!(plan.ready[1].item_id, 2);
        assert_eq!(plan.ready[1].of.item, 2);
        assert_eq!(plan.totals.items, 2);
        assert_eq!(plan.plan_hash, plan.compute_hash());
        assert_eq!(
            plan.not_freeable,
            vec![NotFreeable {
                op_id: "op-clean".into(),
                item_id: 3,
                reason: NotRecoverableReason::AlreadyRestored
            }]
        );
        // Un deshacer no se libera.
        assert_eq!(
            prepare_free(&recs, &["op-undo".into()], |_| Ok(())),
            Err(FreeError::NotFreeable {
                op: "op-undo".into()
            })
        );

        // Liberar 1 (hecho) y 2 (reconciliado como borrado tras un corte).
        let mut f = Journal::open(&paths, "op-free", SystemTime::now()).expect("open");
        start(&mut f, OpKind::Free, OpMode::Permanent, 2);
        for it in &plan.ready {
            f.write(
                Event::ItemIntent {
                    item: it.item_id,
                    action: StepAction::Delete,
                    path: it.path.clone(),
                    identity: it.identity,
                    target: None,
                    category: None,
                    regenerable: false,
                    bytes: it.bytes,
                    parent_mode: None,
                    origin: None,
                    of: Some(it.of.clone()),
                },
                true,
            )
            .expect("intent");
            fs::remove_file(&it.path).expect("rm");
        }
        f.write(
            Event::ItemDone {
                item: 1,
                result_path: None,
                post_check: None,
            },
            true,
        )
        .expect("done");
        f.write(
            Event::ItemReconciled {
                item: 2,
                state: "deleted".into(),
                result_path: None,
                detail: None,
            },
            true,
        )
        .expect("reconciled");
        let (recs, _) = crate::read_journal(&paths).expect("read");
        let m = manifest_for(&recs, "op-clean").expect("manifest");
        assert_eq!(m.items[0].state, ItemState::Deleted);
        assert_eq!(m.items[1].state, ItemState::Deleted);
        assert!(matches!(m.items[2].state, ItemState::Restored { .. }));
        let ops = list_ops(&recs);
        let by_id = |id: &str| ops.iter().find(|o| o.op_id == id).expect("op").clone();
        assert_eq!(by_id("op-clean").undo_state, UndoState::PartlyRestored);
        assert_eq!(by_id("op-free").kind, HistoryKind::Free);
        assert_eq!(by_id("op-free").undo_state, UndoState::NotApplicable);
        // Ya no queda nada que liberar ni que deshacer.
        let again = prepare_free(&recs, &["op-clean".into()], |_| Ok(())).expect("plan");
        assert!(again.ready.is_empty());
        assert_eq!(again.not_freeable.len(), 3);
        assert!(
            prepare_undo(&recs, "op-clean", None)
                .expect("undo")
                .ready
                .is_empty()
        );
    }
}
