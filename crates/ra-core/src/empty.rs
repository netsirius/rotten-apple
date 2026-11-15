//! La Papelera entera (T-12, CU-08, RF-PAN-07).
//!
//! - [`Core::trash_view`] (`rotten trash`): tamaño por volumen y, dentro,
//!   lo que mandó Rotten Apple por operación (reconocido por inode contra
//!   el journal) y lo demás. Sin Acceso total al disco no se adivina: el
//!   volumen sale como no legible y solo con lo que anotó el journal.
//! - [`Core::prepare_empty`] y [`Core::empty_trash`] (`rotten trash
//!   empty`): plan con la identidad de cada entrada de primer nivel; al
//!   ejecutar, el plan debe ser el revisado y cada entrada se revalida con
//!   su identidad exacta. Lo que llega a la Papelera después de revisar no
//!   está en el plan. Si alguna carpeta no se puede leer no se vacía nada.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ra_exec::{EmptyPlan, ExecReport};
use ra_history::{FreeTotals, TrashedItem};
use ra_plan::PlanError;
use serde::Serialize;

use crate::free::{FreeResult, measured, volume_dirs};
use crate::{Core, CoreError};

/// Lo que mandó a la Papelera una operación de Rotten Apple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpInTrash {
    pub op_id: String,
    pub items: u64,
    pub bytes: u64,
}

/// Una carpeta de la Papelera (una por volumen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrashVolume {
    pub path: PathBuf,
    /// `false` sin Acceso total al disco: no se sabe qué hay; solo se
    /// muestra lo que anotó el journal.
    pub readable: bool,
    /// Todo lo que hay en el primer nivel (`None` si no se puede leer).
    pub total: Option<FreeTotals>,
    /// Lo que mandó Rotten Apple, por operación. Con la carpeta legible,
    /// lo que sigue allí (medido); sin ella, lo que dice el journal.
    pub by_operation: Vec<OpInTrash>,
    /// Lo demás (`None` si no se puede leer).
    pub others: Option<FreeTotals>,
    /// Alguna entrada no se pudo recorrer entera: los bytes se quedan
    /// cortos y esa entrada no se vaciará.
    pub partial: bool,
}

/// `rotten trash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrashView {
    /// Todas las carpetas se han podido leer.
    pub readable: bool,
    pub volumes: Vec<TrashVolume>,
}

/// Plan para vaciar la Papelera con su simulación.
#[derive(Debug, Clone)]
pub struct EmptyPreview {
    pub plan: EmptyPlan,
    pub dry_run: ExecReport,
}

/// Agrupa por operación, en el orden del journal (`known`).
fn by_operation<'a>(
    known: &[TrashedItem],
    items: impl IntoIterator<Item = (&'a TrashedItem, u64)>,
) -> Vec<OpInTrash> {
    let mut out: Vec<OpInTrash> = known
        .iter()
        .fold(Vec::<&str>::new(), |mut ops, k| {
            if !ops.contains(&k.of.op.as_str()) {
                ops.push(&k.of.op);
            }
            ops
        })
        .into_iter()
        .map(|op| OpInTrash {
            op_id: op.to_owned(),
            items: 0,
            bytes: 0,
        })
        .collect();
    for (k, bytes) in items {
        match out.iter_mut().find(|o| o.op_id == k.of.op) {
            Some(o) => {
                o.items += 1;
                o.bytes += bytes;
            }
            None => out.push(OpInTrash {
                op_id: k.of.op.clone(),
                items: 1,
                bytes,
            }),
        }
    }
    out.retain(|o| o.items > 0);
    out
}

impl Core {
    fn trash_roots(&self) -> Vec<PathBuf> {
        ra_exec::trash_roots(&self.env.trash, &self.env.home)
    }

    fn trashed_items(&self) -> Result<Vec<TrashedItem>, CoreError> {
        let (records, _) = ra_history::read_journal(&self.history_paths())?;
        Ok(ra_history::trashed_items(&records))
    }

    /// Lo que hay en la Papelera, por volumen.
    pub fn trash_view(&self) -> Result<TrashView, CoreError> {
        let known = self.trashed_items()?;
        let mut view = TrashView {
            readable: true,
            volumes: vec![],
        };
        for root in self.trash_roots() {
            let vol = match ra_exec::read_trash_dir(&root) {
                Ok(entries) => {
                    let (mut total, mut others) = (FreeTotals::default(), FreeTotals::default());
                    let mut mine = Vec::new();
                    let partial = entries.iter().any(|e| e.partial);
                    for e in &entries {
                        total.items += 1;
                        total.bytes += e.bytes;
                        match ra_exec::attribute(e, &known) {
                            Some(k) => mine.push((k, e.bytes)),
                            None => {
                                others.items += 1;
                                others.bytes += e.bytes;
                            }
                        }
                    }
                    TrashVolume {
                        path: root,
                        readable: true,
                        total: Some(total),
                        by_operation: by_operation(&known, mine),
                        others: Some(others),
                        partial,
                    }
                }
                Err(_) => {
                    view.readable = false;
                    let mine = known
                        .iter()
                        .filter(|k| k.path.parent() == Some(root.as_path()))
                        .map(|k| (k, k.bytes));
                    TrashVolume {
                        by_operation: by_operation(&known, mine),
                        path: root,
                        readable: false,
                        total: None,
                        others: None,
                        partial: false,
                    }
                }
            };
            view.volumes.push(vol);
        }
        Ok(view)
    }

    /// Sin poder leer cada carpeta no se vacía nada.
    fn empty_plan(&self) -> Result<EmptyPlan, CoreError> {
        let known = self.trashed_items()?;
        ra_exec::plan_empty(&self.trash_roots(), &known)
            .map_err(|u| CoreError::EmptyUnreadable { path: u.path })
    }

    /// Prepara el vaciado de la Papelera y lo simula (la revisión).
    pub fn prepare_empty(&self) -> Result<EmptyPreview, CoreError> {
        // RF-SEG-05 c: con la configuración corrupta o con otra cosa en la
        // ruta de su enlace no se borra nada.
        self.config_ok_or_abort()?;
        let plan = self.empty_plan()?;
        let dry_run = ra_exec::dry_run_empty(&plan, &self.env.trash, &self.env.home);
        Ok(EmptyPreview { plan, dry_run })
    }

    /// Vacía la Papelera según el plan revisado (`reviewed_hash`). No se
    /// vuelve a listar la Papelera: lo que llegó después no se borra, y lo
    /// que cambió se omite al revalidar su identidad exacta.
    pub fn empty_trash(
        &self,
        plan: &EmptyPlan,
        reviewed_hash: &str,
        by: &str,
    ) -> Result<FreeResult, CoreError> {
        // La configuración se vuelve a validar justo antes de borrar.
        self.config_ok_or_abort()?;
        let actual = plan.compute_hash();
        if actual != reviewed_hash || plan.plan_hash != reviewed_hash {
            return Err(CoreError::Plan(PlanError::HashMismatch {
                expected: reviewed_hash.to_owned(),
                actual,
            }));
        }
        // Una carpeta revisada que ya no existe (volumen desmontado): no es
        // el plan revisado, y se dice cuál.
        for d in &plan.trash_dirs {
            if fs::symlink_metadata(d).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
                return Err(CoreError::TrashGone { path: d.clone() });
            }
        }
        if !plan.within(&self.trash_roots()) {
            return Err(CoreError::Plan(PlanError::Invalid(
                "the plan has entries outside the Trash".into(),
            )));
        }
        let ctx = self.trash_ctx(by)?;
        let exec = ctx.exec(self, by);
        for d in &plan.trash_dirs {
            readable(d)?;
        }
        let dirs = volume_dirs(
            plan.items
                .iter()
                .map(|it| (it.identity.dev, it.path.as_path())),
        );
        measured(dirs, || Ok(ra_exec::execute_empty(plan, &exec, &ctx.lock)?))
    }
}

/// La carpeta se puede listar (o no existe: nada que borrar).
fn readable(dir: &Path) -> Result<(), CoreError> {
    match fs::read_dir(dir) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CoreError::EmptyUnreadable {
            path: dir.to_path_buf(),
        }),
    }
}
