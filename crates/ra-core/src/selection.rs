//! Planes por selección explícita (`rotten trash put`, T-10).
//!
//! El usuario elige las rutas una a una. Pasan por las mismas capas que un
//! plan del catálogo (lista protegida, exclusiones, identidad, huella y
//! journal) con el origen [`Origin::ExplicitSelection`]: pueden estar dentro
//! de Documentos, Escritorio o Descargas, pero nunca salen preseleccionadas
//! (RF-SEG-09).
//!
//! Diferencias con el plan del catálogo (RF-SEG-05):
//! - una ruta protegida, o ancestro de una protegida, **aborta todo** con
//!   [`CoreError::SelectionProtected`] en lugar de omitirse;
//! - también aborta una ruta que, ya resuelta, queda fuera de la carpeta
//!   personal (RF-SEG-19);
//! - y una de las seis carpetas de documentos del usuario en sí, o un enlace
//!   que apunta a una de ellas: su contenido sí se puede elegir;
//! - una ruta que no se puede resolver ni clasificar también aborta;
//! - una ruta excluida por el usuario no entra en el plan (queda en
//!   `skipped`).

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ra_catalog::{Block, Candidate, Risk, RuleAction, ScanResult};
use ra_exec::ExecContext;
use ra_plan::PlanInputs;
use ra_safety::fingerprint::{fingerprint_path, stat_nofollow_any};
use ra_safety::identity::check_flags;
use ra_safety::{AppProbe, Origin};
use rustix::fs::{AtFlags, Dir, Mode, OFlags};

use crate::{Action, Core, CoreError, Plan, Preview, Selection, tilde};

/// Regla y categoría con las que se anotan los elementos elegidos a mano.
/// No es una categoría del catálogo: `exclude add cat:selection` impediría
/// cualquier selección explícita.
pub const SELECTION_ID: &str = "selection";

impl Core {
    /// Construye el plan de una selección explícita y lo simula (la
    /// revisión). Siempre manda a la Papelera.
    ///
    /// Cada ruta debe ser absoluta; su carpeta padre se resuelve (sin
    /// seguir el último componente: un enlace se mueve como enlace).
    pub fn plan_selection(&self, paths: &[PathBuf]) -> Result<Preview, CoreError> {
        let exclusions = self.exclusions_or_abort()?;
        if paths.is_empty() {
            return Err(CoreError::NoPaths);
        }
        let protected = self.protected().for_origin(Origin::ExplicitSelection);
        let mut candidates = Vec::with_capacity(paths.len());
        for raw in paths {
            let path = self.resolve(raw)?;
            if let Err(reason) = protected.check(&path, Origin::ExplicitSelection) {
                return Err(CoreError::SelectionProtected {
                    path: tilde(&path, &self.env.home),
                    reason,
                });
            }
            // Un enlace a ~/Downloads (o a otra carpeta del usuario) cuenta
            // como la carpeta.
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
                && let Ok(target) = fs::canonicalize(&path)
                && let Some(root) = protected.user_folder_root(&target)
            {
                return Err(CoreError::SelectionUserFolderLink {
                    path: tilde(&path, &self.env.home),
                    root: root.to_path_buf(),
                });
            }
            candidates.push(self.selected(path)?);
        }
        let scan = ScanResult {
            catalog_version: self.catalog.catalog_version.clone(),
            candidates,
            roots: vec![],
        };
        let plan = Plan::build(&PlanInputs {
            scan: &scan,
            home: &self.env.home,
            protected: &protected,
            exclusions: &exclusions,
            selection: &Selection::default(),
            action: Action::Trash,
            origin: Origin::ExplicitSelection,
            now: SystemTime::now(),
        });
        let apps = AppProbe::snapshot(self.apps.as_ref());
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

    /// Ruta absoluta con la carpeta padre canónica y el último componente
    /// con su nombre real en disco (I-1). Si no se puede resolver, se aborta
    /// (RF-SEG-05 c).
    fn resolve(&self, raw: &Path) -> Result<PathBuf, CoreError> {
        if !raw.is_absolute() {
            return Err(CoreError::RelativePath(raw.to_path_buf()));
        }
        let unresolvable = |e: std::io::Error| CoreError::Unresolvable {
            path: raw.display().to_string(),
            detail: e.to_string(),
        };
        match (raw.parent(), raw.file_name()) {
            (Some(parent), Some(name)) => {
                let parent = fs::canonicalize(parent).map_err(unresolvable)?;
                let name = real_name(&parent, name).unwrap_or_else(|| name.to_owned());
                Ok(parent.join(name))
            }
            // `/` o una ruta que acaba en `..`: se resuelve entera.
            _ => fs::canonicalize(raw).map_err(unresolvable),
        }
    }

    /// Candidato para una ruta elegida a mano: nunca preseleccionado,
    /// nunca regenerable (al deshacer no se omite si ya existe algo).
    fn selected(&self, path: PathBuf) -> Result<Candidate, CoreError> {
        let (identity, flags) =
            stat_nofollow_any(&path).map_err(|e| CoreError::Unclassifiable {
                path: tilde(&path, &self.env.home),
                detail: e.to_string(),
            })?;
        let mut c = Candidate {
            path,
            rule_id: SELECTION_ID.into(),
            rule_version: 1,
            cat: SELECTION_ID.into(),
            risk: Risk::Review,
            regenerable: false,
            preselected: false,
            action: RuleAction::Trash,
            identity,
            fingerprint: None,
            apps: vec![],
            block: None,
        };
        if let Err(flag) = check_flags(flags) {
            c.block = Some(Block::Flags { flag });
            return Ok(c);
        }
        match fingerprint_path(&c.path) {
            Ok(fp) => {
                if fp.crosses_device {
                    c.block = Some(Block::CrossesDevice);
                }
                c.fingerprint = Some(fp);
            }
            Err(e) => {
                c.block = Some(Block::Unreadable {
                    detail: e.to_string(),
                });
            }
        }
        Ok(c)
    }
}

/// Nombre con el que está guardada en `parent` la entrada que el usuario
/// escribió como `name`. En APFS sin distinción de mayúsculas,
/// `~/Desktop/CARPETA` abre `~/Desktop/carpeta`: el plan, la Papelera y el
/// deshacer deben usar el nombre real, y la detección de ficheros en uso
/// compara rutas por prefijo.
///
/// Sin seguir enlaces: se busca en `parent` la entrada con el mismo
/// `(dev, ino)` que `name`. Si la que se escribió existe tal cual (o es un
/// enlace duro con otro nombre en la misma carpeta), se queda esa. `None`
/// si `name` no existe: lo rechaza después la clasificación.
fn real_name(parent: &Path, name: &OsStr) -> Option<OsString> {
    let dir = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    let want = rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW).ok()?;
    let mut found = None;
    for entry in Dir::read_from(&dir).ok()? {
        let Ok(entry) = entry else { continue };
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let entry_name = OsStr::from_bytes(bytes);
        if entry_name == name {
            return Some(name.to_owned());
        }
        if found.is_none()
            && let Ok(st) = rustix::fs::statat(&dir, entry_name, AtFlags::SYMLINK_NOFOLLOW)
            && st.st_dev == want.st_dev
            && st.st_ino == want.st_ino
        {
            found = Some(entry_name.to_owned());
        }
    }
    found
}
