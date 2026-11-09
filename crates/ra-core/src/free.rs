//! «Liberar ahora» (`rotten free <op-id…>`, T-11, CU-08): borra para
//! siempre de la Papelera los elementos que mandaron allí esas operaciones.
//!
//! - El plan sale **solo del journal**: cada elemento que la operación dejó
//!   en la Papelera y cuya ruta está en el primer nivel de la Papelera que
//!   anotó. Nada que no esté registrado para esas operaciones puede entrar.
//! - Justo antes de borrar cada elemento se revalida su identidad con
//!   `same_after_move` contra la anotada: lo modificado o sustituido dentro
//!   de la Papelera se omite.
//! - Al ejecutar, el plan se vuelve a preparar desde el journal (con el
//!   cerrojo tomado) y su hash debe ser el revisado.
//! - Se mide el espacio libre de cada volumen con `statfs` antes y después
//!   (RF-SEG-12 e, M-5) y se informa de lo estimado frente a lo medido. Los
//!   enlaces duros (`nlink > 1`), los clones APFS y las instantáneas hacen
//!   que se libere menos de lo estimado; su política está sin decidir
//!   (08 §2.2) y la diferencia queda a la vista.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ra_exec::{ExecContext, ExecReport};
use ra_history::{FreePlan, NotRecoverableReason, Record};
use ra_plan::PlanError;
use ra_safety::{AppProbe, Config, Exclusions};
use serde::Serialize;

use crate::{Core, CoreError};

/// Plan de liberación con su simulación (lo que muestra la revisión).
#[derive(Debug, Clone)]
pub struct FreePreview {
    pub plan: FreePlan,
    pub dry_run: ExecReport,
}

/// Espacio libre de un volumen antes y después de liberar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VolumeDelta {
    /// Carpeta de la Papelera medida (identifica el volumen).
    pub path: PathBuf,
    pub free_before: u64,
    pub free_after: u64,
    /// `free_after − free_before`. Puede ser negativo si otra cosa escribió
    /// en el disco a la vez.
    pub freed_bytes: i64,
}

/// Lo estimado frente a lo liberado de verdad (RF-SEG-12 e).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpaceReport {
    /// Suma del tamaño anotado (bloques asignados) de lo borrado.
    pub estimated_bytes: u64,
    /// Suma de lo medido en todos los volumenes; `None` si alguno no se
    /// pudo medir.
    pub measured_bytes: Option<i64>,
    pub volumes: Vec<VolumeDelta>,
}

#[derive(Debug, Clone)]
pub struct FreeResult {
    pub report: ExecReport,
    pub space: SpaceReport,
}

/// Una carpeta por volumen: la de la Papelera del primer elemento de cada
/// volumen.
pub(crate) fn volume_dirs<'a>(
    items: impl IntoIterator<Item = (u64, &'a Path)>,
) -> BTreeMap<u64, PathBuf> {
    let mut out = BTreeMap::new();
    for (dev, path) in items {
        if let Some(parent) = path.parent() {
            out.entry(dev).or_insert_with(|| parent.to_path_buf());
        }
    }
    out
}

fn free_space(dirs: &BTreeMap<u64, PathBuf>) -> BTreeMap<u64, Option<u64>> {
    dirs.iter()
        .map(|(dev, p)| {
            (
                *dev,
                ra_platform::attrs::volume_space(p).ok().map(|v| v.free),
            )
        })
        .collect()
}

/// Ejecuta `exec` midiendo con `statfs` el espacio libre de cada volumen de
/// `dirs` antes y después (RF-SEG-12 e, M-5).
pub(crate) fn measured(
    dirs: BTreeMap<u64, PathBuf>,
    exec: impl FnOnce() -> Result<ExecReport, CoreError>,
) -> Result<FreeResult, CoreError> {
    let before = free_space(&dirs);
    let report = exec()?;
    let after = free_space(&dirs);
    let mut measured = Some(0i64);
    let mut deltas = Vec::new();
    for (dev, path) in dirs {
        match (
            before.get(&dev).copied().flatten(),
            after.get(&dev).copied().flatten(),
        ) {
            (Some(b), Some(a)) => {
                let freed =
                    i64::try_from(a).unwrap_or(i64::MAX) - i64::try_from(b).unwrap_or(i64::MAX);
                measured = measured.map(|m| m + freed);
                deltas.push(VolumeDelta {
                    path,
                    free_before: b,
                    free_after: a,
                    freed_bytes: freed,
                });
            }
            _ => measured = None,
        }
    }
    let space = SpaceReport {
        estimated_bytes: report.bytes_done,
        measured_bytes: measured,
        volumes: deltas,
    };
    Ok(FreeResult { report, space })
}

/// El contexto del ejecutor para borrar de la Papelera, con el cerrojo
/// tomado y la recuperación hecha.
pub(crate) struct Ctx {
    pub(crate) lock: ra_exec::ExecLock,
    pub(crate) protected: ra_safety::ProtectedList,
    pub(crate) exclusions: Exclusions,
    pub(crate) apps: AppProbe,
    pub(crate) history: ra_history::HistoryPaths,
}

impl Ctx {
    pub(crate) fn exec<'a>(&'a self, core: &'a Core, by: &'a str) -> ExecContext<'a> {
        ExecContext {
            history: &self.history,
            protected: &self.protected,
            exclusions: &self.exclusions,
            apps: &self.apps,
            trash: &core.env.trash,
            home: &core.env.home,
            by,
        }
    }
}

impl Core {
    /// Plan de liberación desde el journal. Un elemento entra solo si:
    ///
    /// - la Papelera que anotó su operación es la que se usa ahora (`dir:X`
    ///   solo con `ROTTEN_TRASH_DIR=X`, `system` solo con la del sistema):
    ///   el descriptor del journal no basta para decidir dónde borrar;
    /// - la carpeta donde quedó existe (si no, el volumen no está montado);
    /// - y su ruta está en el primer nivel de esa Papelera.
    fn free_plan(&self, records: &[Record], ops: &[String]) -> Result<FreePlan, CoreError> {
        let active = self.env.trash.describe();
        Ok(ra_history::prepare_free(records, ops, |it| {
            if it.trash != active {
                return Err(NotRecoverableReason::OtherTrash);
            }
            if let Some(parent) = it.path.parent()
                && fs::symlink_metadata(parent).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                return Err(NotRecoverableReason::VolumeNotMounted);
            }
            if ra_exec::in_trash_top_level(
                &it.trash,
                &it.path,
                &it.original_path,
                it.identity.dev,
                &self.env.home,
            ) {
                Ok(())
            } else {
                Err(NotRecoverableReason::NotInTrash)
            }
        })?)
    }

    /// Sin Acceso total al disco, `~/.Trash` no se puede leer: no se
    /// adivina, se dice (CU-08 A1) y no se borra nada.
    fn trash_readable(plan: &FreePlan) -> Result<(), CoreError> {
        for it in &plan.ready {
            if let Err(e) = fs::symlink_metadata(&it.path)
                && e.kind() == io::ErrorKind::PermissionDenied
            {
                return Err(CoreError::TrashUnreadable {
                    path: it.path.parent().unwrap_or(&it.path).to_path_buf(),
                });
            }
        }
        Ok(())
    }

    /// Prepara la liberación de `ops` y la simula (la revisión).
    pub fn prepare_free(&self, ops: &[String]) -> Result<FreePreview, CoreError> {
        let (records, _) = ra_history::read_journal(&self.history_paths())?;
        let plan = self.free_plan(&records, ops)?;
        Self::trash_readable(&plan)?;
        let dry_run = ra_exec::dry_run_free(&plan, &self.env.trash, &self.env.home);
        Ok(FreePreview { plan, dry_run })
    }

    /// Ejecuta una liberación revisada. `reviewed_hash` es el `plan_hash`
    /// que vio el usuario: con el cerrojo tomado, el plan se vuelve a
    /// preparar desde el journal y, si ha cambiado, no se borra nada.
    pub fn free(
        &self,
        plan: &FreePlan,
        reviewed_hash: &str,
        by: &str,
    ) -> Result<FreeResult, CoreError> {
        let ctx = self.trash_ctx(by)?;
        let exec = ctx.exec(self, by);
        let (records, _) = ra_history::read_journal(&ctx.history)?;
        let fresh = self.free_plan(&records, &plan.ops)?;
        if fresh.plan_hash != reviewed_hash {
            return Err(CoreError::Plan(PlanError::HashMismatch {
                expected: reviewed_hash.to_owned(),
                actual: fresh.plan_hash,
            }));
        }
        Self::trash_readable(&fresh)?;
        let dirs = volume_dirs(
            fresh
                .ready
                .iter()
                .map(|it| (it.identity.dev, it.path.as_path())),
        );
        measured(dirs, || {
            Ok(ra_exec::execute_free(&fresh, &exec, &ctx.lock)?)
        })
    }

    /// Toma el cerrojo, prepara el contexto del ejecutor y completa
    /// cualquier operación interrumpida antes de borrar nada.
    pub(crate) fn trash_ctx(&self, by: &str) -> Result<Ctx, CoreError> {
        let ctx = Ctx {
            lock: self.lock()?,
            protected: self.protected(),
            exclusions: Exclusions::compile(&Config::default(), &self.env.home)
                .map_err(CoreError::Other)?,
            apps: AppProbe::snapshot(self.apps.as_ref()),
            history: self.history_paths(),
        };
        ra_exec::recover_incomplete(&ctx.exec(self, by), &ctx.lock)?;
        Ok(ctx)
    }
}
