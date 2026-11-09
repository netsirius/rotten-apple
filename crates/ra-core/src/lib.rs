//! Fachada del núcleo (ADR-03 §4.1): la única API que usan el CLI y, más
//! adelante, la GUI. Flujo: escaneo → reglas → plan → revisión → ejecución
//! → journal → deshacer.
//!
//! El núcleo solo ejecuta planes revisados: [`Core::execute`] recibe el plan
//! y el hash que el usuario vio, y lo rechaza si no coinciden, si la revisión
//! ha caducado o si la configuración no se puede validar.

#![forbid(unsafe_code)]

mod doctor;
mod env;
mod error;
mod scan;
mod selection;

use std::path::Path;
use std::time::SystemTime;

pub use doctor::{AppsProbe, DoctorReport, FdaState, TrashInfo};
pub use env::{BUNDLE_ID, Env, EnvError};
pub use error::{CoreError, ErrorClass};
pub use ra_catalog::{Catalog, Localized, Risk};
pub use ra_exec::{ExecReport, ItemReport, Outcome, SkipCause, TrashBackend};
pub use ra_history::{
    HistoryKind, NotRecoverable, NotRecoverableReason, OpSummary, UndoError, UndoPlan, UndoState,
};
pub use ra_plan::{Action, Plan, PlanError, Selection, SkipReason};
pub use ra_safety::{ConfigState, Exclusion, FileKind, Origin, RunningApps, SystemApps};
pub use scan::{CategorySummary, ScanReport, ScanStatus};

use ra_exec::{ExecContext, ExecLock};
use ra_history::HistoryPaths;
use ra_plan::PlanInputs;
use ra_safety::{AppProbe, Config, Exclusions, ProtectedList};

/// Plan con su simulación (lo que muestra la revisión).
#[derive(Debug, Clone)]
pub struct Preview {
    pub plan: Plan,
    pub dry_run: ExecReport,
}

#[derive(Debug, Clone)]
pub struct UndoPreview {
    pub plan: UndoPlan,
    pub dry_run: ExecReport,
}

pub struct Core {
    env: Env,
    catalog: Catalog,
    apps: Box<dyn RunningApps>,
}

impl Core {
    pub fn new(env: Env) -> Result<Self, CoreError> {
        Self::with_apps(env, Box::new(SystemApps))
    }

    /// Con una fuente de apps en ejecución inyectada (pruebas).
    pub fn with_apps(env: Env, apps: Box<dyn RunningApps>) -> Result<Self, CoreError> {
        let catalog = Catalog::embedded().map_err(|e| CoreError::Other(e.to_string()))?;
        Ok(Self { env, catalog, apps })
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn history_paths(&self) -> HistoryPaths {
        HistoryPaths::new(&self.env.state_dir)
    }

    fn protected(&self) -> ProtectedList {
        ProtectedList::embedded(&self.env.home, &self.env.state_dir)
    }

    pub fn config_state(&self) -> ConfigState {
        ConfigState::load(&self.env.config_path, &self.env.home)
    }

    /// Exclusiones válidas o aborto por seguridad (RF-SEG-05).
    fn exclusions_or_abort(&self) -> Result<Exclusions, CoreError> {
        let state = self.config_state();
        if let ConfigState::Corrupt { path, reason } = state {
            return Err(CoreError::ConfigCorrupt { path, reason });
        }
        state
            .exclusions(&self.env.home)
            .map_err(|reason| CoreError::ConfigCorrupt {
                path: self.env.config_path.clone(),
                reason,
            })
    }

    fn catalog_for(&self, categories: Option<&[String]>) -> Result<Catalog, CoreError> {
        let Some(cats) = categories else {
            return Ok(self.catalog.clone());
        };
        for c in cats {
            if self.catalog.category(&c.to_uppercase()).is_none() {
                return Err(CoreError::UnknownCategory(c.clone()));
            }
        }
        Ok(self
            .catalog
            .filtered(|r| cats.iter().any(|c| c.eq_ignore_ascii_case(&r.cat))))
    }

    fn raw_scan(&self, catalog: &Catalog) -> (ra_catalog::ScanResult, AppProbe) {
        let apps = AppProbe::snapshot(self.apps.as_ref());
        let res = catalog.scan(&ra_catalog::ScanOptions {
            home: &self.env.home,
            apps: &apps,
            now: SystemTime::now(),
        });
        (res, apps)
    }

    /// Escaneo inteligente: resumen por categoría. Con la configuración
    /// corrupta sigue funcionando, con un aviso.
    pub fn scan(&self, categories: Option<&[String]>) -> Result<ScanReport, CoreError> {
        let catalog = self.catalog_for(categories)?;
        let (res, _) = self.raw_scan(&catalog);
        let config = self.config_state();
        let exclusions = config.exclusions(&self.env.home).ok();
        Ok(scan::summarize(
            &catalog,
            &res,
            &self.protected(),
            exclusions.as_ref(),
            &config,
        ))
    }

    /// Construye el plan y lo simula (la revisión). Aborta si las
    /// exclusiones no se pueden validar.
    pub fn plan(&self, selection: &Selection, action: Action) -> Result<Preview, CoreError> {
        let exclusions = self.exclusions_or_abort()?;
        let catalog = self.catalog_for(selection.categories.as_deref())?;
        let (res, apps) = self.raw_scan(&catalog);
        let protected = self.protected();
        let plan = Plan::build(&PlanInputs {
            scan: &res,
            home: &self.env.home,
            protected: &protected,
            exclusions: &exclusions,
            selection,
            action,
            origin: Origin::Rules,
            now: SystemTime::now(),
        });
        let history = self.history_paths();
        let dry_run = ra_exec::dry_run(
            &plan,
            &ExecContext {
                history: &history,
                protected: &protected,
                exclusions: &exclusions,
                apps: &apps,
                trash: &self.env.trash,
                by: "cli",
            },
        );
        Ok(Preview { plan, dry_run })
    }

    fn lock(&self) -> Result<ExecLock, CoreError> {
        ExecLock::try_acquire(&self.env.state_dir)?.ok_or(CoreError::Locked)
    }

    /// Ejecuta un plan revisado. `reviewed_hash` es el `plan_hash` que vio
    /// el usuario.
    pub fn execute(
        &self,
        plan: Plan,
        reviewed_hash: &str,
        by: &str,
    ) -> Result<ExecReport, CoreError> {
        if plan.home() != self.env.home {
            return Err(CoreError::WrongHome(plan.home().to_path_buf()));
        }
        let origin = plan.origin();
        let approved = plan.approve(reviewed_hash, SystemTime::now())?;
        // La configuración se vuelve a validar justo antes de actuar.
        let exclusions = self.exclusions_or_abort()?;
        let lock = self.lock()?;
        let protected = self.protected().for_origin(origin);
        let apps = AppProbe::snapshot(self.apps.as_ref());
        let history = self.history_paths();
        let ctx = ExecContext {
            history: &history,
            protected: &protected,
            exclusions: &exclusions,
            apps: &apps,
            trash: &self.env.trash,
            by,
        };
        ra_exec::recover_incomplete(&ctx, &lock)?;
        Ok(ra_exec::execute(&approved, &ctx, &lock)?)
    }

    /// Operaciones del journal (la más reciente primero) y líneas ilegibles.
    pub fn history(&self) -> Result<(Vec<OpSummary>, Vec<String>), CoreError> {
        let (records, problems) = ra_history::read_journal(&self.history_paths())?;
        Ok((ra_history::list_ops(&records), problems))
    }

    /// Prepara el deshacer de `op` (o de la última operación restaurable) y
    /// lo simula.
    pub fn prepare_undo(
        &self,
        op: Option<&str>,
        items: Option<&[u32]>,
    ) -> Result<UndoPreview, CoreError> {
        let (records, _) = ra_history::read_journal(&self.history_paths())?;
        let op = match op {
            Some(o) => o.to_owned(),
            None => ra_history::list_ops(&records)
                .into_iter()
                .find(|o| matches!(o.undo_state, UndoState::Restorable | UndoState::Partial))
                .map(|o| o.op_id)
                .ok_or(CoreError::NothingToUndo)?,
        };
        let plan = ra_history::prepare_undo(&records, &op, items)?;
        let dry_run = ra_exec::dry_run_undo(&plan);
        Ok(UndoPreview { plan, dry_run })
    }

    /// Restaura los elementos del plan de deshacer.
    pub fn undo(&self, plan: &UndoPlan, by: &str) -> Result<ExecReport, CoreError> {
        let lock = self.lock()?;
        let protected = self.protected();
        let exclusions =
            Exclusions::compile(&Config::default(), &self.env.home).map_err(CoreError::Other)?;
        let apps = AppProbe::snapshot(self.apps.as_ref());
        let history = self.history_paths();
        let ctx = ExecContext {
            history: &history,
            protected: &protected,
            exclusions: &exclusions,
            apps: &apps,
            trash: &self.env.trash,
            by,
        };
        ra_exec::recover_incomplete(&ctx, &lock)?;
        Ok(ra_exec::execute_undo(plan, &ctx, &lock)?)
    }

    /// Reconciliación al arrancar (05 §5.6): cierra las operaciones que un
    /// cierre abrupto dejó a medias. `execute` y `undo` ya la hacen antes de
    /// actuar. Devuelve las operaciones reconciliadas.
    pub fn recover(&self) -> Result<Vec<String>, CoreError> {
        let lock = self.lock()?;
        let protected = self.protected();
        let exclusions =
            Exclusions::compile(&Config::default(), &self.env.home).map_err(CoreError::Other)?;
        let apps = AppProbe::snapshot(self.apps.as_ref());
        let history = self.history_paths();
        let ctx = ExecContext {
            history: &history,
            protected: &protected,
            exclusions: &exclusions,
            apps: &apps,
            trash: &self.env.trash,
            by: "recovery",
        };
        Ok(ra_exec::recover_incomplete(&ctx, &lock)?)
    }

    pub fn doctor(&self) -> DoctorReport {
        doctor::run(self)
    }

    /// Configuración válida para modificarla; nunca sobrescribe un fichero
    /// corrupto (CU-07 A3).
    fn editable_config(&self) -> Result<Config, CoreError> {
        match self.config_state() {
            ConfigState::Missing => Ok(Config::default()),
            ConfigState::Loaded(c) => Ok(c),
            ConfigState::Corrupt { path, reason } => Err(CoreError::ConfigCorrupt { path, reason }),
        }
    }

    pub fn exclusions(&self) -> Result<Vec<Exclusion>, CoreError> {
        Ok(self.editable_config()?.all())
    }

    /// Añade o quita una exclusión. Devuelve si cambió algo. Con `dry_run` no
    /// escribe.
    pub fn edit_exclusion(
        &self,
        raw: &str,
        add: bool,
        dry_run: bool,
    ) -> Result<(Exclusion, bool), CoreError> {
        let ex = Exclusion::parse(raw).map_err(CoreError::InvalidExclusion)?;
        let mut config = self.editable_config()?;
        let changed = if add {
            config.add(&ex)
        } else {
            config.remove(&ex)
        };
        if changed && !dry_run {
            // Se valida contra la carpeta personal antes de guardar.
            Exclusions::compile(&config, &self.env.home).map_err(CoreError::InvalidExclusion)?;
            config.save(&self.env.config_path)?;
        }
        Ok((ex, changed))
    }
}

/// `true` si el proceso corre como root (RF-CLI-06).
pub fn running_as_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// Ruta mostrable relativa a la carpeta personal (`~/…`).
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
