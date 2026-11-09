//! Ejecución de planes, deshacer y recuperación tras un cierre abrupto.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use ra_history::{
    Event, HistoryPaths, Journal, JournalError, OpKind, OpMode, OpStatus, StepAction, UndoPlan,
    manifest_for, read_journal,
};
use ra_plan::{Action, ApprovedPlan, Plan, PlanItem};
use ra_platform::files::OpenFiles;
use ra_safety::fingerprint::{open_parent_nofollow, stat_nofollow_any};
use ra_safety::{AppProbe, Exclusions, FileKind, ProtectedList};
use rustix::fs::{AtFlags, Dir, Mode, OFlags, RenameFlags};
use serde::{Deserialize, Serialize};

use crate::lock::ExecLock;
use crate::trash::{TrashBackend, find_trashed};
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
            open: OpenFiles::snapshot().map_err(|e| e.to_string()),
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

/// Intenciones por `fsync` (RNF-06, 05 §5.6). Con `F_FULLFSYNC` cada
/// sincronización cuesta varios milisegundos: con un `fsync` por evento,
/// 10 000 elementos tardaban casi dos minutos. Con lotes de 128 son unas 80
/// sincronizaciones y el coste pasa a ser el de verificar y mover. Más grande
/// apenas gana y deja más intenciones sin resultado tras un corte de
/// corriente.
pub const DEFAULT_FSYNC_BATCH: usize = 128;

/// Variable que cambia el tamaño del lote (entero ≥ 1).
pub const FSYNC_BATCH_VAR: &str = "ROTTEN_FSYNC_BATCH";

/// En un lote lento (p. ej. con la Papelera del sistema), los resultados se
/// sincronizan como mucho cada este tiempo: así un corte de corriente pierde
/// pocos resultados que haya que deducir al recuperar.
const RESULTS_SYNC_EVERY: Duration = Duration::from_millis(250);

/// Tamaño del lote: [`FSYNC_BATCH_VAR`] si es un entero ≥ 1 o, si no,
/// [`DEFAULT_FSYNC_BATCH`].
pub fn fsync_batch() -> usize {
    std::env::var(FSYNC_BATCH_VAR)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_FSYNC_BATCH)
}

/// Gancho de prueba: orden en que ocurren las intenciones, los `fsync` de
/// los lotes y las acciones sobre el sistema de ficheros.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    Intent(u32),
    Synced,
    /// Justo antes del `rename` o el `unlink` del elemento.
    Act(u32),
}

#[cfg(test)]
thread_local! {
    static PROBES: std::cell::RefCell<Vec<Probe>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn probe(p: Probe) {
    PROBES.with(|v| v.borrow_mut().push(p));
}

#[cfg(not(test))]
#[inline(always)]
fn probe(_: Probe) {}

/// Anota el resultado de un elemento. No espera al disco: lo lleva el
/// `fsync` del lote siguiente o el de `op_finished`.
fn write_outcome(j: &mut Journal, item: u32, outcome: &Outcome) -> Result<(), JournalError> {
    let ev = match outcome {
        Outcome::Done {
            result_path,
            post_check,
        } => Event::ItemDone {
            item,
            result_path: result_path.clone(),
            post_check: post_check.clone(),
        },
        Outcome::Skipped { cause, message } => Event::ItemSkipped {
            item,
            reason: cause.code().into(),
            detail: Some(message.clone()),
        },
        Outcome::Failed { error } => Event::ItemFailed {
            item,
            error: error.clone(),
        },
        Outcome::Would { .. } => return Ok(()),
    };
    j.write(ev, false)?;
    Ok(())
}

/// `fsync` del journal. El gancho de prueba va pegado a la llamada real:
/// la traza no puede decir que hubo `fsync` si no lo hubo.
fn sync(j: &mut Journal) -> Result<(), JournalError> {
    j.sync()?;
    probe(Probe::Synced);
    Ok(())
}

/// Escritura anticipada por lotes: anexa las intenciones de `batch`
/// elementos, las lleva al disco con un único `fsync` y después, elemento a
/// elemento, verifica, actúa (`act`) y anota el resultado.
///
/// Nunca se actúa sobre un elemento cuya intención no es durable. La
/// verificación de cada elemento sigue siendo justo antes de actuar, con su
/// intención ya escrita: si lo omite, queda anotado como omitido.
fn run_batched<T>(
    j: &mut Journal,
    items: &[T],
    batch: usize,
    intent: impl Fn(&T) -> Event,
    mut act: impl FnMut(&T) -> ItemReport,
    report: &mut ExecReport,
) -> Result<(), ExecError> {
    for chunk in items.chunks(batch.max(1)) {
        let mut seqs = Vec::with_capacity(chunk.len());
        for it in chunk {
            let ev = intent(it);
            if let Event::ItemIntent { item, .. } = &ev {
                probe(Probe::Intent(*item));
            }
            seqs.push(j.write(ev, false)?);
        }
        sync(j)?;
        let mut synced_at = Instant::now();
        for (it, seq) in chunk.iter().zip(seqs) {
            // Invariante de la escritura anticipada; no depende del lote.
            assert!(j.is_durable(seq), "intención no durable antes de actuar");
            let r = act(it);
            write_outcome(j, r.item_id, &r.outcome)?;
            report.push(r);
            if synced_at.elapsed() >= RESULTS_SYNC_EVERY {
                sync(j)?;
                synced_at = Instant::now();
            }
        }
    }
    Ok(())
}

/// Ejecuta un plan aprobado. Exige el cerrojo de ejecución.
pub fn execute(
    approved: &ApprovedPlan,
    ctx: &ExecContext<'_>,
    lock: &ExecLock,
) -> Result<ExecReport, ExecError> {
    execute_batched(approved, ctx, lock, fsync_batch())
}

fn execute_batched(
    approved: &ApprovedPlan,
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
    batch: usize,
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
    run_batched(
        &mut j,
        plan.items(),
        batch,
        |item| {
            let parent_mode = item
                .path
                .parent()
                .and_then(|p| fs::symlink_metadata(p).ok())
                .map(|m| m.permissions().mode() & 0o7777);
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
            }
        },
        |item| run_item(item, ctx, &g),
        &mut report,
    )?;
    j.write(
        Event::OpFinished {
            status: report.status(),
            done: report.done,
            skipped: report.skipped,
            failed: report.failed,
            unknown: 0,
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
    probe(Probe::Act(item.item_id));
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
            Ok(degraded) => Outcome::Done {
                result_path: None,
                post_check: degraded.then(|| {
                    "AT_NODELETEBUSY no disponible: borrado sin la barrera del kernel".into()
                }),
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

/// `AT_NODELETEBUSY` de `<sys/fcntl.h>` (no está en `rustix`): `unlinkat`
/// falla con `EBUSY` si el fichero tiene descriptores abiertos.
const AT_NODELETEBUSY: AtFlags = AtFlags::from_bits_retain(0x4000);

/// Si el sistema admite `AT_NODELETEBUSY`. Se desactiva la primera vez que
/// devuelve `EINVAL` (RNF-ARQ-27).
static NODELETEBUSY: AtomicBool = AtomicBool::new(true);

/// `unlinkat` de un fichero con `extra` (normalmente `AT_NODELETEBUSY`) como
/// última barrera. Si el sistema no admite el flag (`EINVAL`), lo anota en
/// `supported` y borra sin él. Devuelve `true` si se degradó.
fn unlink_file(
    parent: BorrowedFd<'_>,
    name: &OsStr,
    extra: AtFlags,
    supported: &AtomicBool,
) -> io::Result<bool> {
    if supported.load(Ordering::Relaxed) {
        match rustix::fs::unlinkat(parent, name, extra) {
            Ok(()) => return Ok(false),
            Err(rustix::io::Errno::INVAL) => supported.store(false, Ordering::Relaxed),
            Err(e) => return Err(e.into()),
        }
    }
    rustix::fs::unlinkat(parent, name, AtFlags::empty())?;
    Ok(true)
}

/// Borrado permanente relativo a descriptor; no sigue enlaces ni cruza
/// volúmenes (la huella ya lo ha comprobado). Devuelve `true` si algún
/// fichero se borró sin `AT_NODELETEBUSY`.
fn remove_at(parent: BorrowedFd<'_>, name: &OsStr, kind: FileKind) -> io::Result<bool> {
    let mut degraded = false;
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
            degraded |= remove_at(dir.as_fd(), &c, ck)?;
        }
        rustix::fs::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    } else {
        degraded |= unlink_file(parent, name, AT_NODELETEBUSY, &NODELETEBUSY)?;
    }
    Ok(degraded)
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
    lock: &ExecLock,
) -> Result<ExecReport, ExecError> {
    execute_undo_batched(plan, ctx, lock, fsync_batch())
}

fn execute_undo_batched(
    plan: &UndoPlan,
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
    batch: usize,
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
    run_batched(
        &mut j,
        &plan.ready,
        batch,
        |u| Event::ItemIntent {
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
        |u| {
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
            ItemReport {
                item_id: u.item_id,
                path: u.to.clone(),
                bytes: u.bytes,
                outcome,
            }
        },
        &mut report,
    )?;
    j.write(
        Event::OpFinished {
            status: report.status(),
            done: report.done,
            skipped: report.skipped,
            failed: report.failed,
            unknown: 0,
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
    probe(Probe::Act(u.item_id));
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
/// original sigue en su sitio y, si no, si llegó a su destino (la Papelera
/// de la operación o la ruta que restauraba).
///
/// Con la escritura por lotes puede haber muchas intenciones sin resultado:
/// las del lote en curso que aún no se habían tocado (siguen en su sitio) y,
/// tras un corte de corriente, resultados que no llegaron al disco.
pub fn recover_incomplete(
    ctx: &ExecContext<'_>,
    _lock: &ExecLock,
) -> Result<Vec<String>, ExecError> {
    let (records, _) = read_journal(ctx.history)?;
    let mut started: Vec<(&str, Option<&str>, &str)> = Vec::new();
    let mut finished: Vec<&str> = Vec::new();
    for r in &records {
        match &r.event {
            Event::OpStarted { undo_of, trash, .. } => {
                started.push((&r.op, undo_of.as_deref(), trash))
            }
            Event::OpFinished { .. } => finished.push(&r.op),
            _ => {}
        }
    }
    let mut touched: Vec<String> = Vec::new();
    let mut recovered = Vec::new();
    for (op, undo_of, trash) in started
        .into_iter()
        .filter(|(op, ..)| !finished.contains(op))
    {
        let mut j = Journal::open(ctx.history, op, SystemTime::now())?;
        let mut resolved: BTreeSet<u32> = BTreeSet::new();
        for r in records.iter().filter(|r| r.op == op) {
            match &r.event {
                Event::ItemDone { item, .. }
                | Event::ItemSkipped { item, .. }
                | Event::ItemFailed { item, .. }
                | Event::ItemReconciled { item, .. } => {
                    resolved.insert(*item);
                }
                _ => {}
            }
        }
        let (mut done, mut skipped, mut unknown) = (0, 0, 0);
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
            let (state, result_path, detail) = match stat_nofollow_any(path) {
                // Mover y borrar no conservan el objeto en su ruta: si el
                // mismo objeto (volumen, inode y tipo) sigue ahí, no se tocó.
                // Puede haber cambiado (la verificación lo habría omitido),
                // y una carpeta que se borraba puede haber perdido parte de
                // su contenido.
                Ok((id, _)) if id.same_object(identity) => {
                    skipped += 1;
                    let detail = (!id.same_after_move(identity)).then(|| {
                        if *action == StepAction::Delete && identity.kind == FileKind::Dir {
                            "sigue en su sitio, pero cambió: puede faltar parte de su contenido"
                        } else {
                            "sigue en su sitio, pero cambió desde la revisión"
                        }
                        .to_owned()
                    });
                    ("not_moved", None, detail)
                }
                // Un borrado permanente cuyo original ya no está se hizo.
                _ if *action == StepAction::Delete => {
                    done += 1;
                    ("deleted", None, None)
                }
                _ => {
                    // Un deshacer interrumpido puede haber llegado a su destino
                    // (con o sin sufijo « (restaurado)»); una limpieza, a la
                    // Papelera que anotó en `op_started`.
                    let found = match (action, target) {
                        (StepAction::Restore, Some(t)) => {
                            Ok(find_restored(t, identity).map(|p| ("restored", p)))
                        }
                        (StepAction::Trash, _) => {
                            find_trashed(trash, path, identity).map(|o| o.map(|p| ("trashed", p)))
                        }
                        _ => Ok(None),
                    };
                    match found {
                        Ok(Some((state, p))) => {
                            done += 1;
                            (state, Some(p), None)
                        }
                        // Ni en su sitio ni en su destino: no cuenta como hecho.
                        // Si la Papelera no se pudo leer, se dice por qué.
                        Ok(None) => {
                            unknown += 1;
                            ("unknown", None, None)
                        }
                        Err(why) => {
                            unknown += 1;
                            ("unknown", None, Some(why))
                        }
                    }
                }
            };
            // Lo lleva al disco el `fsync` de `op_finished`.
            j.write(
                Event::ItemReconciled {
                    item: *item,
                    state: state.into(),
                    result_path,
                    detail,
                },
                false,
            )?;
        }
        j.write(
            Event::OpFinished {
                status: OpStatus::Recovered,
                done,
                skipped,
                failed: 0,
                unknown,
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

    fn tmp_file(name: &str) -> (tempfile::TempDir, std::fs::File, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = fs::File::open(tmp.path()).expect("abrir carpeta");
        let path = tmp.path().join(name);
        fs::write(&path, b"x").expect("write");
        (tmp, dir, path)
    }

    /// Con un descriptor abierto, `AT_NODELETEBUSY` no borra (`EBUSY`); si
    /// el sistema no lo admite, se degrada y se anota.
    #[test]
    fn nodeletebusy_refuses_open_files_or_degrades() {
        let (_tmp, dir, path) = tmp_file("abierto");
        let _open = fs::File::open(&path).expect("abrir");
        let supported = AtomicBool::new(true);
        match unlink_file(
            dir.as_fd(),
            OsStr::new("abierto"),
            AT_NODELETEBUSY,
            &supported,
        ) {
            Err(e) => {
                assert_eq!(
                    e.raw_os_error(),
                    Some(rustix::io::Errno::BUSY.raw_os_error())
                );
                assert!(path.exists(), "no debe borrarse un fichero abierto");
                assert!(supported.load(Ordering::Relaxed));
            }
            Ok(degraded) => {
                assert!(degraded && !supported.load(Ordering::Relaxed));
                assert!(!path.exists());
            }
        }
    }

    /// Un flag que el sistema rechaza con `EINVAL` desactiva la barrera y se
    /// borra sin ella (RNF-ARQ-27).
    #[test]
    fn unsupported_flag_degrades_on_einval() {
        let (_tmp, dir, path) = tmp_file("cerrado");
        let supported = AtomicBool::new(true);
        let bogus = AtFlags::from_bits_retain(0x4000_0000);
        let degraded =
            unlink_file(dir.as_fd(), OsStr::new("cerrado"), bogus, &supported).expect("borrar");
        assert!(degraded);
        assert!(!supported.load(Ordering::Relaxed));
        assert!(!path.exists());
        // Una vez desactivada, ni se intenta.
        let (_tmp2, dir2, path2) = tmp_file("otro");
        assert!(unlink_file(dir2.as_fd(), OsStr::new("otro"), bogus, &supported).expect("borrar"));
        assert!(!path2.exists());
    }

    #[test]
    fn closed_files_are_deleted_with_the_barrier() {
        let (_tmp, dir, path) = tmp_file("cerrado");
        let supported = AtomicBool::new(true);
        let degraded = unlink_file(
            dir.as_fd(),
            OsStr::new("cerrado"),
            AT_NODELETEBUSY,
            &supported,
        )
        .expect("borrar");
        assert!(!path.exists());
        assert_eq!(degraded, !supported.load(Ordering::Relaxed));
    }

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

/// Escritura anticipada por lotes (T-08, RNF-06).
#[cfg(test)]
mod batch_tests {
    use std::collections::BTreeMap;

    use ra_catalog::{Candidate, Risk, RuleAction, ScanResult};
    use ra_history::{ItemState, manifest_for, prepare_undo};
    use ra_plan::{PlanInputs, Selection};
    use ra_safety::fingerprint::fingerprint_path;
    use ra_safety::{Config, Exclusions, Origin, ProtectedList};

    use super::*;

    struct Fx {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        logs: PathBuf,
        history: HistoryPaths,
        state: PathBuf,
        protected: ProtectedList,
        exclusions: Exclusions,
        apps: AppProbe,
        trash: TrashBackend,
        trash_dir: PathBuf,
    }

    impl Fx {
        fn new() -> Fx {
            let tmp = tempfile::tempdir().expect("tempdir");
            // Canónica: la verificación no sigue enlaces (/var → /private/var).
            let root = fs::canonicalize(tmp.path()).expect("canon");
            let (home, state, trash_dir) = (root.join("home"), root.join("state"), root.join("t"));
            let logs = home.join("Library/Logs");
            for d in [&logs, &state, &trash_dir] {
                fs::create_dir_all(d).expect("mkdir");
            }
            Fx {
                history: HistoryPaths::new(&state),
                protected: ProtectedList::embedded(&home, &state),
                exclusions: Exclusions::compile(&Config::default(), &home).expect("exclusiones"),
                apps: AppProbe::snapshot(&BTreeSet::<String>::new()),
                trash: TrashBackend::Dir(trash_dir.clone()),
                _tmp: tmp,
                home,
                logs,
                state,
                trash_dir,
            }
        }

        fn ctx(&self) -> ExecContext<'_> {
            ExecContext {
                history: &self.history,
                protected: &self.protected,
                exclusions: &self.exclusions,
                apps: &self.apps,
                trash: &self.trash,
                by: "test",
            }
        }

        /// `n` ficheros y una carpeta con dos ficheros dentro (la última).
        fn files(&self, n: usize) -> Vec<PathBuf> {
            let mut out: Vec<PathBuf> = (0..n)
                .map(|i| {
                    let p = self.logs.join(format!("f-{i:02}.log"));
                    fs::write(&p, b"contenido").expect("write");
                    p
                })
                .collect();
            let d = self.logs.join("z-dir");
            fs::create_dir(&d).expect("mkdir");
            for f in ["a.log", "b.log"] {
                fs::write(d.join(f), b"x").expect("write");
            }
            out.push(d);
            out
        }

        fn approved(&self, paths: &[PathBuf], action: Action) -> ApprovedPlan {
            let candidates = paths
                .iter()
                .map(|p| Candidate {
                    path: p.clone(),
                    rule_id: "test".into(),
                    rule_version: 1,
                    cat: "CAT-03".into(),
                    risk: Risk::Safe,
                    regenerable: false,
                    preselected: true,
                    action: RuleAction::Trash,
                    identity: stat_nofollow_any(p).expect("lstat").0,
                    fingerprint: Some(fingerprint_path(p).expect("huella")),
                    apps: vec![],
                    block: None,
                })
                .collect();
            let scan = ScanResult {
                catalog_version: "test".into(),
                candidates,
                roots: vec![],
            };
            let plan = Plan::build(&PlanInputs {
                scan: &scan,
                home: &self.home,
                protected: &self.protected,
                exclusions: &self.exclusions,
                selection: &Selection::default(),
                action,
                origin: Origin::ExplicitSelection,
                now: SystemTime::now(),
            });
            assert_eq!(plan.items().len(), paths.len(), "{:?}", plan.skipped());
            let hash = plan.plan_hash().to_owned();
            plan.approve(&hash, SystemTime::now()).expect("aprobar")
        }

        fn lock(&self) -> ExecLock {
            ExecLock::try_acquire(&self.state)
                .expect("cerrojo")
                .expect("libre")
        }
    }

    fn take_probes() -> Vec<Probe> {
        PROBES.with(|v| std::mem::take(&mut *v.borrow_mut()))
    }

    /// Reproduce la traza: una intención es durable desde el primer `fsync`
    /// posterior, y ningún `rename` ni `unlink` puede llegar antes. Devuelve
    /// los elementos sobre los que se actuó y cuántos lotes hubo.
    fn check_write_ahead(probes: &[Probe]) -> (Vec<u32>, usize) {
        let (mut pending, mut durable) = (BTreeSet::new(), BTreeSet::new());
        let mut acted = Vec::new();
        let mut batches = 0;
        let mut prev = None;
        for p in probes {
            match *p {
                Probe::Intent(i) => {
                    if !matches!(prev, Some(Probe::Intent(_))) {
                        batches += 1;
                    }
                    pending.insert(i);
                }
                Probe::Synced => durable.append(&mut pending),
                Probe::Act(i) => {
                    assert!(
                        durable.contains(&i),
                        "se actuó sobre {i} antes del fsync de su intención: {probes:?}"
                    );
                    acted.push(i);
                }
            }
            prev = Some(*p);
        }
        (acted, batches)
    }

    /// Estado de cada elemento en el manifiesto de `op`.
    fn states(fx: &Fx, op: &str) -> BTreeMap<u32, ItemState> {
        let (records, _) = read_journal(&fx.history).expect("journal");
        let m = manifest_for(&records, op).expect("manifiesto");
        m.items.into_iter().map(|i| (i.item_id, i.state)).collect()
    }

    /// Con cualquier tamaño de lote, cada `rename` (Papelera) y cada
    /// `unlink` (borrado permanente) llega después del `fsync` de su
    /// intención; los elementos que la verificación omite, con la intención
    /// ya escrita, quedan anotados como omitidos.
    #[test]
    fn no_rename_or_unlink_before_its_intent_is_durable() {
        for action in [Action::Trash, Action::Delete] {
            for batch in [1, 2, 3, 7, 100] {
                let fx = Fx::new();
                let paths = fx.files(6);
                let approved = fx.approved(&paths, action);
                // Cambian después de la revisión: la verificación los omite.
                fs::write(&paths[1], b"otro contenido").expect("write");
                fs::remove_file(&paths[4]).expect("rm");
                take_probes();
                let r = execute_batched(&approved, &fx.ctx(), &fx.lock(), batch).expect("ejecutar");
                let (acted, batches) = check_write_ahead(&take_probes());
                let ctx = format!("{action:?}, lote {batch}");
                assert_eq!(batches, paths.len().div_ceil(batch), "{ctx}");
                assert_eq!((r.done, r.skipped, r.failed), (5, 2, 0), "{ctx}");
                assert_eq!(acted, vec![1, 3, 4, 6, 7], "{ctx}");
                let st = states(&fx, r.op_id.as_deref().expect("op"));
                assert_eq!(
                    st[&2],
                    ItemState::Skipped {
                        reason: "identity_changed".into()
                    },
                    "{ctx}"
                );
                assert_eq!(
                    st[&5],
                    ItemState::Skipped {
                        reason: "missing".into()
                    },
                    "{ctx}"
                );
                let done = match action {
                    Action::Trash => ItemState::Trashed,
                    Action::Delete => ItemState::Deleted,
                };
                for i in [1, 3, 4, 6, 7] {
                    assert_eq!(st[&i], done, "{ctx}: elemento {i}");
                }
            }
        }
    }

    /// Deshacer también restaura (`rename`) solo con la intención durable.
    #[test]
    fn undo_restores_only_after_its_intent_is_durable() {
        for batch in [1, 2, 100] {
            let fx = Fx::new();
            let paths = fx.files(4);
            let approved = fx.approved(&paths, Action::Trash);
            let r = execute_batched(&approved, &fx.ctx(), &fx.lock(), 3).expect("ejecutar");
            let op = r.op_id.expect("op");
            let (records, _) = read_journal(&fx.history).expect("journal");
            let plan = prepare_undo(&records, &op, None).expect("deshacer");
            assert_eq!(plan.ready.len(), paths.len());
            take_probes();
            let u = execute_undo_batched(&plan, &fx.ctx(), &fx.lock(), batch).expect("deshacer");
            let (acted, batches) = check_write_ahead(&take_probes());
            assert_eq!(batches, paths.len().div_ceil(batch), "lote {batch}");
            assert_eq!(acted.len(), paths.len(), "lote {batch}");
            assert_eq!(u.done, paths.len() as u64, "lote {batch}");
            for p in &paths {
                assert!(p.exists(), "lote {batch}: {p:?} no se restauró");
            }
        }
    }

    /// Un lote cortado: todas sus intenciones en el disco y ningún
    /// resultado (un corte de corriente puede perder los que no se
    /// sincronizaron).
    fn interrupted_batch(fx: &Fx, op: &str, approved: &ApprovedPlan) {
        let plan = approved.plan();
        let mut j = Journal::open(&fx.history, op, SystemTime::now()).expect("journal");
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
                by: "test".into(),
                trash: fx.trash.describe(),
            },
            true,
        )
        .expect("start");
        for item in plan.items() {
            let ev = Event::ItemIntent {
                item: item.item_id,
                action: step(item.action),
                path: item.path.clone(),
                identity: item.identity,
                target: None,
                category: Some(item.category.clone()),
                regenerable: item.regenerable,
                bytes: item.bytes_allocated,
                parent_mode: None,
                origin: Some(item.origin),
            };
            j.write(ev, false).expect("intent");
        }
        j.sync().expect("sync");
    }

    fn not_moved() -> ItemState {
        ItemState::Skipped {
            reason: "interrupted_before_action".into(),
        }
    }

    /// `recover_incomplete` resuelve a la vez todas las intenciones sin
    /// resultado de un lote, y lo que la verificación omitió (porque cambió)
    /// queda omitido, no `unknown`.
    #[test]
    fn recovery_resolves_every_pending_intent_of_a_trash_batch() {
        let fx = Fx::new();
        let paths = fx.files(3);
        let approved = fx.approved(&paths, Action::Trash);
        interrupted_batch(&fx, "op-lote", &approved);
        // 1: movido sin resultado anotado. 2: la verificación lo omitió
        // porque cambió, y su `item_skipped` no llegó al disco. 3 y 4 (la
        // carpeta): el corte llegó antes.
        fs::rename(&paths[0], fx.trash_dir.join("f-00.log")).expect("mover");
        fs::write(&paths[1], b"cambiado").expect("write");
        let recovered = recover_incomplete(&fx.ctx(), &fx.lock()).expect("recover");
        assert_eq!(recovered, vec!["op-lote"]);
        let st = states(&fx, "op-lote");
        assert_eq!(st[&1], ItemState::Trashed);
        for i in [2, 3, 4] {
            assert_eq!(st[&i], not_moved(), "elemento {i}");
        }
        assert!(
            recover_incomplete(&fx.ctx(), &fx.lock())
                .expect("recover")
                .is_empty()
        );
    }

    /// En un borrado permanente cortado, lo que sigue en su sitio (aunque
    /// sea una carpeta a medio borrar) no se anota como borrado.
    #[test]
    fn recovery_of_a_permanent_batch_never_calls_deleted_what_is_still_there() {
        let fx = Fx::new();
        let paths = fx.files(2);
        let approved = fx.approved(&paths, Action::Delete);
        interrupted_batch(&fx, "op-borrado", &approved);
        // 1: borrado. 3: carpeta a medio borrar. 2: no se llegó a tocar.
        fs::remove_file(&paths[0]).expect("rm");
        fs::remove_file(paths[2].join("a.log")).expect("rm");
        recover_incomplete(&fx.ctx(), &fx.lock()).expect("recover");
        let st = states(&fx, "op-borrado");
        assert_eq!(st[&1], ItemState::Deleted);
        assert_eq!(st[&2], not_moved());
        assert_eq!(st[&3], not_moved());
        let (records, _) = read_journal(&fx.history).expect("journal");
        let detail = records.iter().find_map(|r| match &r.event {
            Event::ItemReconciled {
                item: 3, detail, ..
            } => detail.clone(),
            _ => None,
        });
        assert!(
            detail.is_some_and(|d| d.contains("puede faltar parte de su contenido")),
            "la carpeta a medio borrar debe decirlo"
        );
    }
}
