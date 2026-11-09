//! Evaluación de reglas sobre el sistema de ficheros (capa 1, ADR-04 §5.4).
//!
//! - Nunca sigue enlaces simbólicos: un enlace que coincide es candidato
//!   como enlace (se mueve el enlace, no su destino).
//! - Si algún componente de la raíz es un enlace, la raíz no se recorre.
//! - No cruza puntos de montaje.
//! - No lee contenido de ficheros: solo metadatos.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ra_safety::apps::AppBlock;
use ra_safety::fingerprint::{fingerprint_path, stat_nofollow_any};
use ra_safety::identity::{check_flags, flags_of};
use ra_safety::paths::{self, fold};
use ra_safety::{AppProbe, FileKind, FlagViolation, Identity, TreeFingerprint};
use serde::{Deserialize, Serialize};

use crate::schema::{Risk, Rule, RuleAction};
use crate::{Catalog, Compiled};

pub struct ScanOptions<'a> {
    /// Carpeta personal (absoluta y canónica).
    pub home: &'a Path,
    pub apps: &'a AppProbe,
    pub now: SystemTime,
}

/// Por qué un candidato no puede entrar en un plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    /// Modificado hace menos de `min_age_days`.
    TooRecent {
        min_age_days: u32,
        age_days: i64,
    },
    /// Una app que debe estar cerrada está abierta, o no se puede saber.
    /// Con campo propio: `AppBlock` también lleva `kind` y, aplanado, la
    /// clave se repetía y se perdía `"app"`.
    App {
        detail: AppBlock,
    },
    Flags {
        flag: FlagViolation,
    },
    /// Contiene un nombre que la regla prohíbe (cookies, historial…).
    ContainsNeverName {
        name: String,
    },
    CrossesDevice,
    Unreadable {
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub path: PathBuf,
    pub rule_id: String,
    pub rule_version: u32,
    pub cat: String,
    pub risk: Risk,
    pub regenerable: bool,
    pub preselected: bool,
    pub action: RuleAction,
    pub identity: Identity,
    pub fingerprint: Option<TreeFingerprint>,
    /// Apps que deben estar cerradas (se revalida al ejecutar).
    pub apps: Vec<String>,
    pub block: Option<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RootState {
    /// Recorrida entera.
    Complete,
    /// No existe: nada que limpiar.
    Absent,
    /// Recorrida en parte (alguna subcarpeta no se pudo leer).
    Partial { detail: String },
    /// No se pudo recorrer (permiso, enlace simbólico en la ruta…).
    Unavailable { detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootStatus {
    pub rule_id: String,
    pub cat: String,
    pub root: PathBuf,
    #[serde(flatten)]
    pub state: RootState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub catalog_version: String,
    pub candidates: Vec<Candidate>,
    pub roots: Vec<RootStatus>,
}

pub(crate) fn scan(catalog: &Catalog, opts: &ScanOptions<'_>) -> ScanResult {
    let mut candidates = Vec::new();
    let mut roots = Vec::new();
    for (rule, compiled) in catalog.rules.iter().zip(catalog.compiled()) {
        for root in &rule.roots {
            let Ok(abs) = paths::expand_home(root, opts.home) else {
                continue;
            };
            let state = walk_root(rule, compiled, &abs, opts, &mut candidates);
            roots.push(RootStatus {
                rule_id: rule.id.clone(),
                cat: rule.cat.clone(),
                root: abs,
                state,
            });
        }
    }
    ScanResult {
        catalog_version: catalog.catalog_version.clone(),
        candidates,
        roots,
    }
}

fn is_permission(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied
}

fn walk_root(
    rule: &Rule,
    compiled: &Compiled,
    root: &Path,
    opts: &ScanOptions<'_>,
    out: &mut Vec<Candidate>,
) -> RootState {
    let root_id = match stat_nofollow_any(root) {
        Ok((id, _)) if id.kind == FileKind::Dir => id,
        Ok(_) => {
            return RootState::Unavailable {
                detail: "the root is not a folder".into(),
            };
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return RootState::Absent,
        Err(e) if is_permission(&e) => {
            return RootState::Unavailable {
                detail: format!("no permission: {e}"),
            };
        }
        Err(e) => {
            return RootState::Unavailable {
                detail: format!("cannot be opened without following links: {e}"),
            };
        }
    };

    let mut partial: Option<String> = None;
    let mut queue: VecDeque<(PathBuf, String, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), String::new(), 0));
    while let Some((dir, rel, depth)) = queue.pop_front() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if depth == 0 => {
                return RootState::Unavailable {
                    detail: format!("cannot be read: {e}"),
                };
            }
            Err(e) => {
                partial.get_or_insert_with(|| format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                partial.get_or_insert_with(|| format!("{}: cannot be read", path.display()));
                continue;
            };
            let identity = Identity::from_metadata(&meta);
            if identity.dev != root_id.dev {
                continue;
            }
            let folded_rel = fold(&child_rel);
            if compiled.exclude.is_match(&folded_rel) {
                continue;
            }
            if folded_rel.split('/').any(|c| compiled.never.contains(c)) {
                continue;
            }
            if compiled.include.is_match(&folded_rel) {
                out.push(candidate(
                    rule, compiled, path, &name, identity, &meta, opts,
                ));
            } else if identity.kind == FileKind::Dir && depth + 1 < rule.max_depth {
                queue.push_back((path, child_rel, depth + 1));
            }
        }
    }
    match partial {
        Some(detail) => RootState::Partial { detail },
        None => RootState::Complete,
    }
}

fn candidate(
    rule: &Rule,
    compiled: &Compiled,
    path: PathBuf,
    name: &str,
    identity: Identity,
    meta: &fs::Metadata,
    opts: &ScanOptions<'_>,
) -> Candidate {
    let apps = resolve_apps(rule, name);
    let preselected = rule.preselect || compiled.preselect_names.contains(&fold(name));
    let mut c = Candidate {
        path,
        rule_id: rule.id.clone(),
        rule_version: rule.version,
        cat: rule.cat.clone(),
        risk: rule.risk,
        regenerable: rule.regenerable,
        preselected,
        action: rule.action,
        identity,
        fingerprint: None,
        apps,
        block: None,
    };
    c.block = evaluate(rule, compiled, &mut c, meta, opts).err();
    c
}

fn evaluate(
    rule: &Rule,
    compiled: &Compiled,
    c: &mut Candidate,
    meta: &fs::Metadata,
    opts: &ScanOptions<'_>,
) -> Result<(), Block> {
    check_flags(flags_of(meta)).map_err(|flag| Block::Flags { flag })?;
    let fp = fingerprint_path(&c.path).map_err(|e| Block::Unreadable {
        detail: e.to_string(),
    })?;
    let crosses = fp.crosses_device;
    let newest = fp.newest_mtime;
    c.fingerprint = Some(fp);
    if crosses {
        return Err(Block::CrossesDevice);
    }
    if c.identity.kind == FileKind::Dir
        && !compiled.never.is_empty()
        && let Some(name) = find_never_name(&c.path, &compiled.never)
    {
        return Err(Block::ContainsNeverName { name });
    }
    if rule.min_age_days > 0 {
        let now = opts
            .now
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let age_days = (now - newest).div_euclid(86_400);
        if age_days < i64::from(rule.min_age_days) {
            return Err(Block::TooRecent {
                min_age_days: rule.min_age_days,
                age_days,
            });
        }
    }
    opts.apps.check(&c.apps)
        .map_err(|detail| Block::App { detail })?;
    Ok(())
}

/// Resuelve `{bundle_id}`: el nombre del candidato sin el sufijo de la regla,
/// solo si parece un identificador inverso (`com.empresa.app`).
fn resolve_apps(rule: &Rule, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for a in &rule.app_not_running {
        if a == "{bundle_id}" {
            let base = match &rule.bundle_id_suffix {
                Some(sfx) => name.strip_suffix(sfx.as_str()).unwrap_or(name),
                None => name,
            };
            if looks_like_bundle_id(base) {
                out.push(base.to_owned());
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn looks_like_bundle_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        })
}

/// Busca entre los descendientes (sin seguir enlaces) algún nombre prohibido.
fn find_never_name(dir: &Path, never: &std::collections::BTreeSet<String>) -> Option<String> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if never.contains(&fold(&name)) {
                return Some(name);
            }
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    None
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn bundle_id_heuristic() {
        assert!(looks_like_bundle_id("com.example.Editor"));
        assert!(looks_like_bundle_id("org.mozilla.firefox"));
        assert!(!looks_like_bundle_id("Google"));
        assert!(!looks_like_bundle_id("com..x"));
        assert!(!looks_like_bundle_id("my cache.dir"));
    }
}
