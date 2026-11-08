//! Manifiesto de deshacer, historial y preparación del deshacer.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use ra_safety::Identity;
use ra_safety::fingerprint::stat_nofollow_any;
use serde::{Deserialize, Serialize};

use crate::HistoryPaths;
use crate::journal::{Event, OpKind, OpMode, OpStatus, Record, StepAction};

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
    Unknown,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpSummary {
    pub op_id: String,
    pub kind: OpKind,
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
        if self.kind == OpKind::Undo {
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
                ItemState::Trashed | ItemState::Unknown => gone += 1,
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
                ItemState::Pending | ItemState::Unknown => {}
            }
        }
        OpSummary {
            op_id: self.op_id.clone(),
            kind: self.kind,
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
                        _ => ItemState::Unknown,
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
        for r in records.iter().filter(|r| undo_ops.contains(&r.op.as_str())) {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoPlan {
    pub undo_of: String,
    pub ready: Vec<UndoItem>,
    /// Elementos pedidos que ya no se pueden restaurar, con el motivo.
    pub not_recoverable: Vec<(u32, String)>,
}

/// Prepara el deshacer de `op`, entero (`items = None`) o por elementos.
pub fn prepare_undo(
    records: &[Record],
    op: &str,
    items: Option<&[u32]>,
) -> Result<UndoPlan, String> {
    let m = manifest_for(records, op).ok_or_else(|| format!("no existe la operación {op}"))?;
    if m.kind != OpKind::Clean {
        return Err(format!(
            "la operación {op} no se puede deshacer ({:?})",
            m.kind
        ));
    }
    if let Some(ids) = items {
        for id in ids {
            if !m.items.iter().any(|i| i.item_id == *id) {
                return Err(format!("la operación {op} no tiene el elemento {id}"));
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
            ItemState::Trashed => plan
                .not_recoverable
                .push((i.item_id, "ya no está en la Papelera o ha cambiado".into())),
            ItemState::Deleted => plan
                .not_recoverable
                .push((i.item_id, "borrado permanentemente".into())),
            ItemState::Restored { .. } => plan
                .not_recoverable
                .push((i.item_id, "ya se restauró".into())),
            ItemState::Unknown => plan.not_recoverable.push((
                i.item_id,
                "paradero desconocido tras un cierre abrupto".into(),
            )),
            ItemState::Pending | ItemState::Skipped { .. } | ItemState::Failed { .. } => {
                // No se movió: no hay nada que deshacer. Solo se informa si se pidió.
                if items.is_some() {
                    plan.not_recoverable
                        .push((i.item_id, "no se llegó a mover".into()));
                }
            }
        }
    }
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
    }
}
