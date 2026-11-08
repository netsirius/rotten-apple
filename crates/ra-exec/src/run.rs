//! Ejecución de planes, deshacer y recuperación tras un cierre abrupto.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ra_history::{
    Event, HistoryPaths, Journal, JournalError, OpKind, OpMode, OpStatus, StepAction, UndoPlan,
    manifest_for, read_journal,
};
use ra_plan::{Action, ApprovedPlan, Plan, PlanItem};
use ra_safety::fingerprint::{open_parent_nofollow, stat_nofollow_any};
use ra_safety::{AppProbe, Exclusions, FileKind, ProtectedList};
use rustix::fs::{AtFlags, Dir, Mode, OFlags, RenameFlags};
use serde::{Deserialize, Serialize};

use crate::lock::ExecLock;
use crate::trash::TrashBackend;
use crate::verify::{Guards, SkipCause, open_error, verify};

/// Todo lo que el ejecutor necesita además del plan.
pub struct ExecContext<'a> {
    pub history: &'a HistoryPaths,
    pub protected: &'a ProtectedList,
    pub exclusions: &'a Exclusions,
    pub apps: &'a AppProbe,
    pub trash: &'a TrashBackend,
    /// Origen para el journal: `cli`, `gui`…
    pub by: &'a str,
}

impl ExecContext<'_> {
    fn guards(&self) -> Guards<'_> {
        Guards {
            protected: self.protected,
            exclusions: self.exclusions,
            apps: self.apps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Hecho; `result_path` es dónde quedó (Papelera o ruta restaurada).
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        result_path: Option<PathBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        post_check: Option<String>,
    },
    /// `--dry-run`: se haría (`would_trash`, `would_delete`, `would_restore`).
    Would {
        action: String,
    },
    Skipped {
        #[serde(flatten)]
        cause: SkipCause,
        message: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemReport {
    pub item_id: u32,
    pub path: PathBuf,
    pub bytes: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_id: Option<String>,
    pub dry_run: bool,
    pub items: Vec<ItemReport>,
    pub done: u64,
    pub would: u64,
    pub skipped: u64,
    pub failed: u64,
    /// Bytes movidos a la Papelera, borrados o restaurados (según la
    /// operación). Movido ≠ liberado.
    pub bytes_done: u64,
}

impl ExecReport {
    fn push(&mut self, r: ItemReport) {
        match &r.outcome {
            Outcome::Done { .. } => {
                self.done += 1;
                self.bytes_done += r.bytes;
            }
            Outcome::Would { .. } => self.would += 1,
            Outcome::Skipped { .. } => self.skipped += 1,
            Outcome::Failed { .. } => self.failed += 1,
        }
        self.items.push(r);
    }

    fn status(&self) -> OpStatus {
        if self.skipped + self.failed == 0 {
            OpStatus::Completed
        } else {
            OpStatus::Partial
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("E/S: {0}")]
    Io(#[from] io::Error),
    #[error("el plan es de otra carpeta personal ({0})")]
    WrongHome(PathBuf),
}

fn skipped(item_id: u32, path: &Path, bytes: u64, cause: SkipCause) -> ItemReport {
    ItemReport {
        item_id,
        path: path.to_path_buf(),
        bytes,
        outcome: Outcome::Skipped {
            message: cause.to_string(),
            cause,
        },
    }
}

fn would(action: Action) -> String {
    match action {
        Action::Trash => "would_trash".into(),
        Action::Delete => "would_delete".into(),
    }
}

/// Simulación: la misma validación de la capa 3 que [`execute`], sin actuar
/// ni escribir el journal.
pub fn dry_run(plan: &Plan, ctx: &ExecContext<'_>) -> ExecReport {
    let mut report = ExecReport {
        dry_run: true,
        ..Default::default()
    };
    let g = ctx.guards();
    for item in plan.items() {
        let r = match verify(item, &g) {
            Ok(_) => ItemReport {
                item_id: item.item_id,
                path: item.path.clone(),
                bytes: item.bytes_allocated,
                outcome: Outcome::Would {
                    action: would(item.action),
                },
            },
            Err(cause) => skipped(item.item_id, &item.path, item.bytes_allocated, cause),
        };
        report.push(r);
    }
    report
}

fn step(action: Action) -> StepAction {
    match action {
        Action::Trash => StepAction::Trash,
        Action::Delete => StepAction::Delete,
    }
}

/// Ejecuta un plan aprobado. Exige el cerrojo de ejecución.
pub fn execute(
    approved: &ApprovedPlan,
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
) -> Result<ExecReport, ExecError> {
    let plan = approved.plan();
    let now = SystemTime::now();
    let op = ra_plan::new_id(now);
    let mut j = Journal::open(ctx.history, &op, now)?;
    j.write(
        Event::OpStarted {
            kind: OpKind::Clean,
            mode: match plan.action() {
                Action::Trash => OpMode::Trash,
                Action::Delete => OpMode::Permanent,
            },
            plan_hash: Some(plan.plan_hash().to_owned()),
            undo_of: None,
            items: plan.totals().items,
            bytes: plan.totals().bytes_allocated,
            by: ctx.by.to_owned(),
            trash: ctx.trash.describe(),
        },
        true,
    )?;

    let mut report = ExecReport {
        op_id: Some(op.clone()),
        ..Default::default()
    };
    let g = ctx.guards();
    for item in plan.items() {
        // Escritura anticipada: la intención llega al disco antes de actuar.
        let parent_mode = item
            .path
            .parent()
            .and_then(|p| fs::symlink_metadata(p).ok())
            .map(|m| m.permissions().mode() & 0o7777);
        j.write(
            Event::ItemIntent {
                item: item.item_id,
                action: step(item.action),
                path: item.path.clone(),
                identity: item.identity,
                target: None,
                category: Some(item.category.clone()),
                regenerable: item.regenerable,
                bytes: item.bytes_allocated,
                parent_mode,
                origin: Some(item.origin),
            },
            true,
        )?;
        let r = run_item(item, ctx, &g);
        match &r.outcome {
            Outcome::Done {
                result_path,
                post_check,
            } => j.write(
                Event::ItemDone {
                    item: item.item_id,
                    result_path: result_path.clone(),
                    post_check: post_check.clone(),
                },
                true,
            )?,
            Outcome::Skipped { cause, message } => j.write(
                Event::ItemSkipped {
                    item: item.item_id,
                    reason: cause.code().into(),
                    detail: Some(message.clone()),
                },
                true,
            )?,
            Outcome::Failed { error } => j.write(
                Event::ItemFailed {
                    item: item.item_id,
                    error: error.clone(),
                },
                true,
            )?,
            Outcome::Would { .. } => {}
        }
        report.push(r);
    }
    j.write(
        Event::OpFinished {
            status: report.status(),
            done: report.done,
            skipped: report.skipped,
            failed: report.failed,
        },
        true,
    )?;
    save_manifests(ctx.history, &[&op])?;
    Ok(report)
}

fn run_item(item: &PlanItem, ctx: &ExecContext<'_>, g: &Guards<'_>) -> ItemReport {
    let v = match verify(item, g) {
        Ok(v) => v,
        Err(cause) => return skipped(item.item_id, &item.path, item.bytes_allocated, cause),
    };
    let outcome = match item.action {
        Action::Trash => match ctx.trash.trash(&item.path, v.parent.as_fd(), v.name) {
            Ok(to) => {
                // Mover a la Papelera es un `rename`: el inode debe ser el mismo.
                let post_check = match stat_nofollow_any(&to) {
                    Ok((id, _)) if id.same_object(&item.identity) => None,
                    Ok(_) => Some("el elemento en la Papelera tiene otro inode".into()),
                    Err(e) => Some(format!("no se pudo comprobar en la Papelera: {e}")),
                };
                Outcome::Done {
                    result_path: Some(to),
                    post_check,
                }
            }
            Err(e) => Outcome::Failed {
                error: e.to_string(),
            },
        },
        Action::Delete => match remove_at(v.parent.as_fd(), v.name, item.identity.kind) {
            Ok(()) => Outcome::Done {
                result_path: None,
                post_check: None,
            },
            Err(e) => Outcome::Failed {
                error: e.to_string(),
            },
        },
    };
    ItemReport {
        item_id: item.item_id,
        path: item.path.clone(),
        bytes: item.bytes_allocated,
        outcome,
    }
}

/// Borrado permanente relativo a descriptor; no sigue enlaces ni cruza
/// volúmenes (la huella ya lo ha comprobado).
fn remove_at(parent: BorrowedFd<'_>, name: &OsStr, kind: FileKind) -> io::Result<()> {
    if kind == FileKind::Dir {
        let dir = rustix::fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let dev = rustix::fs::fstat(&dir)?.st_dev;
        let mut children = Vec::new();
        for e in Dir::read_from(&dir)? {
            let e = e?;
            let n = e.file_name().to_bytes();
            if n != b"." && n != b".." {
                children.push(OsString::from(OsStr::from_bytes(n)));
            }
        }
        for c in children {
            let st = rustix::fs::statat(&dir, c.as_os_str(), AtFlags::SYMLINK_NOFOLLOW)?;
            let ck = ra_safety::Identity::from_stat(&st).kind;
            if ck == FileKind::Dir && st.st_dev != dev {
                return Err(io::Error::new(
                    io::ErrorKind::CrossesDevices,
                    "punto de montaje",
                ));
            }
            remove_at(dir.as_fd(), &c, ck)?;
        }
        rustix::fs::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    } else {
        rustix::fs::unlinkat(parent, name, AtFlags::empty())?;
    }
    Ok(())
}

fn save_manifests(history: &HistoryPaths, ops: &[&str]) -> Result<(), ExecError> {
    let (records, _) = read_journal(history)?;
    for op in ops {
        if let Some(m) = manifest_for(&records, op) {
            m.save(history)?;
        }
    }
    Ok(())
}

/// `nombre (restaurado)`, `nombre (restaurado 2)`… conservando la extensión.
fn restored_name(name: &OsStr, n: u32) -> OsString {
    let bytes = name.as_bytes();
    let (stem, ext) = match bytes.iter().rposition(|&b| b == b'.') {
        Some(i) if i > 0 => (&bytes[..i], &bytes[i..]),
        _ => (bytes, &b""[..]),
    };
    let mut out = stem.to_vec();
    if n <= 1 {
        out.extend_from_slice(" (restaurado)".as_bytes());
    } else {
        out.extend_from_slice(format!(" (restaurado {n})").as_bytes());
    }
    out.extend_from_slice(ext);
    OsString::from(OsStr::from_bytes(&out))
}

/// Simulación del deshacer: mismas comprobaciones, sin mover nada.
pub fn dry_run_undo(plan: &UndoPlan) -> ExecReport {
    let mut report = ExecReport {
        dry_run: true,
        ..Default::default()
    };
    for u in &plan.ready {
        let r = match check_restorable(u) {
            Ok(()) => ItemReport {
                item_id: u.item_id,
                path: u.to.clone(),
                bytes: u.bytes,
                outcome: Outcome::Would {
                    action: "would_restore".into(),
                },
            },
            Err(cause) => skipped(u.item_id, &u.to, u.bytes, cause),
        };
        report.push(r);
    }
    report
}

fn check_restorable(u: &ra_history::UndoItem) -> Result<(), SkipCause> {
    match stat_nofollow_any(&u.from) {
        Ok((id, _)) if id.same_after_move(&u.identity) => {}
        Ok(_) => return Err(SkipCause::IdentityChanged),
        Err(e) => return Err(open_error(e)),
    }
    if fs::symlink_metadata(&u.to).is_ok() && u.regenerable {
        return Err(SkipCause::ConflictExists);
    }
    Ok(())
}

/// Restaura los elementos de un plan de deshacer (CU-05). Nunca sobrescribe:
/// si la ruta original existe, omite lo regenerable y conserva ambos en el
/// resto (`nombre (restaurado)`).
pub fn execute_undo(
    plan: &UndoPlan,
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
) -> Result<ExecReport, ExecError> {
    let now = SystemTime::now();
    let op = ra_plan::new_id(now);
    let mut j = Journal::open(ctx.history, &op, now)?;
    j.write(
        Event::OpStarted {
            kind: OpKind::Undo,
            mode: OpMode::Restore,
            plan_hash: None,
            undo_of: Some(plan.undo_of.clone()),
            items: plan.ready.len() as u64,
            bytes: plan.ready.iter().map(|u| u.bytes).sum(),
            by: ctx.by.to_owned(),
            trash: ctx.trash.describe(),
        },
        true,
    )?;
    let mut report = ExecReport {
        op_id: Some(op.clone()),
        ..Default::default()
    };
    for u in &plan.ready {
        j.write(
            Event::ItemIntent {
                item: u.item_id,
                action: StepAction::Restore,
                path: u.from.clone(),
                identity: u.identity,
                target: Some(u.to.clone()),
                category: None,
                regenerable: u.regenerable,
                bytes: u.bytes,
                parent_mode: u.parent_mode,
                origin: None,
            },
            true,
        )?;
        let outcome = match restore_one(u) {
            Ok(to) => Outcome::Done {
                result_path: Some(to),
                post_check: None,
            },
            Err(Ok(cause)) => Outcome::Skipped {
                message: cause.to_string(),
                cause,
            },
            Err(Err(e)) => Outcome::Failed {
                error: e.to_string(),
            },
        };
        match &outcome {
            Outcome::Done { result_path, .. } => j.write(
                Event::ItemDone {
                    item: u.item_id,
                    result_path: result_path.clone(),
                    post_check: None,
                },
                true,
            )?,
            Outcome::Skipped { cause, message } => j.write(
                Event::ItemSkipped {
                    item: u.item_id,
                    reason: cause.code().into(),
                    detail: Some(message.clone()),
                },
                true,
            )?,
            Outcome::Failed { error } => j.write(
                Event::ItemFailed {
                    item: u.item_id,
                    error: error.clone(),
                },
                true,
            )?,
            Outcome::Would { .. } => {}
        }
        report.push(ItemReport {
            item_id: u.item_id,
            path: u.to.clone(),
            bytes: u.bytes,
            outcome,
        });
    }
    j.write(
        Event::OpFinished {
            status: report.status(),
            done: report.done,
            skipped: report.skipped,
            failed: report.failed,
        },
        true,
    )?;
    save_manifests(ctx.history, &[&op, &plan.undo_of])?;
    Ok(report)
}

type RestoreResult = Result<PathBuf, Result<SkipCause, io::Error>>;

fn restore_one(u: &ra_history::UndoItem) -> RestoreResult {
    // El elemento en la Papelera debe ser el mismo que se movió.
    let (from_parent, from_name) = open_parent_nofollow(&u.from).map_err(|e| Ok(open_error(e)))?;
    let st = rustix::fs::statat(from_parent.as_fd(), from_name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|e| Ok(open_error(e.into())))?;
    if !ra_safety::Identity::from_stat(&st).same_after_move(&u.identity) {
        return Err(Ok(SkipCause::IdentityChanged));
    }
    // Recrear las carpetas padre que falten con los permisos registrados.
    let to_parent_path = u.to.parent().ok_or(Ok(SkipCause::Missing))?;
    if fs::symlink_metadata(to_parent_path).is_err() {
        fs::create_dir_all(to_parent_path).map_err(Err)?;
        if let Some(mode) = u.parent_mode {
            fs::set_permissions(to_parent_path, fs::Permissions::from_mode(mode)).map_err(Err)?;
        }
    }
    // La carpeta de destino se abre sin seguir enlaces: si alguien ha
    // cambiado un componente por un enlace, no se restaura en otro sitio.
    let (to_parent, to_name) = open_parent_nofollow(&u.to).map_err(|e| Ok(open_error(e)))?;
    for n in 1..1000u32 {
        let candidate = if n == 1 {
            to_name.to_owned()
        } else {
            restored_name(to_name, n - 1)
        };
        if n == 1
            && u.regenerable
            && rustix::fs::statat(to_parent.as_fd(), to_name, AtFlags::SYMLINK_NOFOLLOW).is_ok()
        {
            // La app ya regeneró la caché: por defecto se omite (CU-05 A2).
            return Err(Ok(SkipCause::ConflictExists));
        }
        match rustix::fs::renameat_with(
            from_parent.as_fd(),
            from_name,
            to_parent.as_fd(),
            candidate.as_os_str(),
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => return Ok(to_parent_path.join(candidate)),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => return Err(Err(e.into())),
        }
    }
    Err(Err(io::Error::other("no hay nombre libre para restaurar")))
}

/// Reconciliación al arrancar (05 §5.6): cierra las operaciones que no
/// llegaron a `op_finished`. Para cada intención sin resultado, mira si el
/// original sigue en su sitio.
pub fn recover_incomplete(
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
) -> Result<Vec<String>, ExecError> {
    let (records, _) = read_journal(ctx.history)?;
    let mut started: Vec<(&str, Option<&str>)> = Vec::new();
    let mut finished: Vec<&str> = Vec::new();
    for r in &records {
        match &r.event {
            Event::OpStarted { undo_of, .. } => started.push((&r.op, undo_of.as_deref())),
            Event::OpFinished { .. } => finished.push(&r.op),
            _ => {}
        }
    }
    let mut touched: Vec<String> = Vec::new();
    let mut recovered = Vec::new();
    for (op, undo_of) in started.into_iter().filter(|(op, _)| !finished.contains(op)) {
        let mut j = Journal::open(ctx.history, op, SystemTime::now())?;
        let mut resolved: Vec<u32> = Vec::new();
        for r in records.iter().filter(|r| r.op == op) {
            match &r.event {
                Event::ItemDone { item, .. }
                | Event::ItemSkipped { item, .. }
                | Event::ItemFailed { item, .. }
                | Event::ItemReconciled { item, .. } => resolved.push(*item),
                _ => {}
            }
        }
        let (mut done, mut skipped) = (0, 0);
        for r in records.iter().filter(|r| r.op == op) {
            let Event::ItemIntent {
                item,
                action,
                path,
                identity,
                target,
                ..
            } = &r.event
            else {
                continue;
            };
            if resolved.contains(item) {
                continue;
            }
            let (state, result_path) = match stat_nofollow_any(path) {
                Ok((id, _)) if id.same_after_move(identity) => {
                    skipped += 1;
                    ("not_moved", None)
                }
                _ => {
                    done += 1;
                    // Un deshacer interrumpido puede haber llegado a su destino
                    // (con o sin sufijo « (restaurado)»).
                    let restored = match (action, target) {
                        (StepAction::Restore, Some(t)) => find_restored(t, identity),
                        _ => None,
                    };
                    match restored {
                        Some(p) => ("restored", Some(p)),
                        None => ("unknown", None),
                    }
                }
            };
            j.write(
                Event::ItemReconciled {
                    item: *item,
                    state: state.into(),
                    result_path,
                },
                true,
            )?;
        }
        j.write(
            Event::OpFinished {
                status: OpStatus::Recovered,
                done,
                skipped,
                failed: 0,
            },
            true,
        )?;
        recovered.push(op.to_owned());
        touched.push(op.to_owned());
        if let Some(of) = undo_of {
            touched.push(of.to_owned());
        }
    }
    let ops: Vec<&str> = touched.iter().map(String::as_str).collect();
    save_manifests(ctx.history, &ops)?;
    Ok(recovered)
}

/// Busca en la ruta original (y en sus variantes « (restaurado N)») el
/// elemento que un deshacer interrumpido pudo llegar a mover.
fn find_restored(target: &Path, identity: &ra_safety::Identity) -> Option<PathBuf> {
    let parent = target.parent()?;
    let name = target.file_name()?;
    (0..20u32).find_map(|n| {
        let candidate = if n == 0 {
            target.to_path_buf()
        } else {
            parent.join(restored_name(name, n))
        };
        stat_nofollow_any(&candidate)
            .ok()
            .filter(|(id, _)| id.same_after_move(identity))
            .map(|_| candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_names() {
        assert_eq!(restored_name(OsStr::new("a.log"), 1), "a (restaurado).log");
        assert_eq!(restored_name(OsStr::new("dir"), 2), "dir (restaurado 2)");
    }

    #[test]
    fn skipped_items_serialize_with_reason_and_message() {
        let causes = [
            SkipCause::IdentityChanged,
            SkipCause::Flags {
                flag: ra_safety::FlagViolation::Immutable,
            },
            SkipCause::Protected {
                detail: ra_safety::ProtectionReason::Home,
            },
        ];
        for cause in causes {
            let code = cause.code();
            let r = skipped(1, Path::new("/x"), 0, cause);
            let v = serde_json::to_value(&r).expect("json");
            assert_eq!(v["outcome"], "skipped");
            assert_eq!(v["reason"], code);
            assert!(v["message"].as_str().is_some_and(|m| !m.is_empty()));
        }
    }
}
