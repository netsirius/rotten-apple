//! Enlace `~/.config/rotten/config.toml` → configuración canónica (07 §2,
//! T-13).
//!
//! - Solo se crea si en esa ruta no hay nada: `symlink(2)` nunca sustituye
//!   lo que haya, así que tampoco cuando algo aparece entre la comprobación y
//!   la creación.
//! - Si hay otra cosa (un fichero, una carpeta, un enlace a otro sitio), no
//!   se toca: alguien podría editarlo creyendo que es la configuración, y sus
//!   exclusiones no se aplicarían. Quien llama trata ese estado como una
//!   configuración que no se puede validar (RF-SEG-05 c).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Lo que hay en la ruta del enlace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LinkState {
    /// No hay nada.
    Missing,
    /// El enlace a la configuración canónica.
    Linked,
    /// Otra cosa: no se toca y no se actúa.
    Conflict { found: Found },
}

/// Qué hay en la ruta del enlace cuando no es el enlace esperado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Found {
    File,
    Directory,
    /// Un enlace a otro sitio.
    Symlink {
        target: PathBuf,
    },
    Other,
    /// No se puede saber (p. ej. sin permiso para mirar).
    Unreadable {
        detail: String,
    },
}

impl std::fmt::Display for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Found::File => f.write_str("a file"),
            Found::Directory => f.write_str("a folder"),
            Found::Symlink { target } => write!(f, "a link to {}", target.display()),
            Found::Other => f.write_str("something that is not a file"),
            Found::Unreadable { detail } => write!(f, "it cannot be inspected: {detail}"),
        }
    }
}

/// ¿Apunta el enlace `link`, cuyo contenido es `points_to`, a `target`?
/// Vale el destino exacto, relativo a la carpeta del enlace, o cualquier
/// camino que lleve al mismo fichero.
fn points_at(link: &Path, points_to: &Path, target: &Path) -> bool {
    if points_to == target {
        return true;
    }
    if points_to.is_relative()
        && let Some(dir) = link.parent()
        && dir.join(points_to) == target
    {
        return true;
    }
    matches!(
        (fs::canonicalize(link), fs::canonicalize(target)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// Qué hay en `link` respecto a la configuración `target`. No cambia nada.
pub fn inspect(link: &Path, target: &Path) -> LinkState {
    let meta = match fs::symlink_metadata(link) {
        Ok(m) => m,
        // Nada, o una carpeta intermedia que no es carpeta: ahí no puede
        // haber ningún fichero que se confunda con la configuración.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return LinkState::Missing;
        }
        Err(e) => {
            return LinkState::Conflict {
                found: Found::Unreadable {
                    detail: e.to_string(),
                },
            };
        }
    };
    let ft = meta.file_type();
    let found = if ft.is_symlink() {
        match fs::read_link(link) {
            Ok(to) if points_at(link, &to, target) => return LinkState::Linked,
            Ok(to) => Found::Symlink { target: to },
            Err(e) => Found::Unreadable {
                detail: e.to_string(),
            },
        }
    } else if ft.is_file() {
        Found::File
    } else if ft.is_dir() {
        Found::Directory
    } else {
        Found::Other
    };
    LinkState::Conflict { found }
}

/// Crea el enlace si no hay nada en `link` (con sus carpetas). Nunca
/// sustituye lo que haya: si algo aparece mientras tanto, devuelve lo que
/// encuentra. Devuelve el estado final.
pub fn create(link: &Path, target: &Path) -> io::Result<LinkState> {
    match inspect(link, target) {
        LinkState::Missing => {}
        other => return Ok(other),
    }
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir)?;
    }
    match std::os::unix::fs::symlink(target, link) {
        Ok(()) => Ok(LinkState::Linked),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(inspect(link, target)),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fx {
        _tmp: tempfile::TempDir,
        link: PathBuf,
        target: PathBuf,
    }

    fn fx() -> Fx {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canon");
        let state = root.join("state");
        fs::create_dir_all(&state).expect("mkdir");
        let target = state.join("config.toml");
        fs::write(&target, "schema_version = 1\n").expect("write");
        Fx {
            _tmp: tmp,
            link: root.join("home/.config/rotten/config.toml"),
            target,
        }
    }

    #[test]
    fn creates_the_link_and_its_folders_when_nothing_is_there() {
        let f = fx();
        assert_eq!(inspect(&f.link, &f.target), LinkState::Missing);
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Linked
        );
        assert_eq!(fs::read_link(&f.link).expect("readlink"), f.target);
        assert_eq!(inspect(&f.link, &f.target), LinkState::Linked);
        // Otra vez: ya está, no cambia nada.
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Linked
        );
    }

    #[test]
    fn a_relative_link_to_the_same_file_counts() {
        let f = fx();
        fs::create_dir_all(f.link.parent().expect("padre")).expect("mkdir");
        std::os::unix::fs::symlink("../../../state/config.toml", &f.link).expect("ln");
        assert_eq!(inspect(&f.link, &f.target), LinkState::Linked);
    }

    /// Nunca sustituye un fichero, una carpeta ni un enlace que ya estén.
    #[test]
    fn never_replaces_what_is_already_there() {
        let f = fx();
        let dir = f.link.parent().expect("padre").to_path_buf();
        fs::create_dir_all(&dir).expect("mkdir");
        let other = dir.join("otra.toml");
        fs::write(&other, "x").expect("write");

        fs::write(&f.link, "mía").expect("write");
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Conflict { found: Found::File }
        );
        assert_eq!(fs::read_to_string(&f.link).expect("read"), "mía");
        fs::remove_file(&f.link).expect("rm");

        fs::create_dir(&f.link).expect("mkdir");
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Conflict {
                found: Found::Directory
            }
        );
        fs::remove_dir(&f.link).expect("rmdir");

        std::os::unix::fs::symlink(&other, &f.link).expect("ln");
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Conflict {
                found: Found::Symlink {
                    target: other.clone()
                }
            }
        );
        assert_eq!(fs::read_link(&f.link).expect("readlink"), other);
        fs::remove_file(&f.link).expect("rm");

        // Un enlace roto a otro sitio también es otra cosa.
        std::os::unix::fs::symlink(dir.join("no-existe"), &f.link).expect("ln");
        assert!(matches!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Conflict {
                found: Found::Symlink { .. }
            }
        ));
    }

    /// Un enlace roto que apunta a la configuración canónica es el enlace
    /// (la configuración aún no existe).
    #[test]
    fn a_dangling_link_to_the_canonical_path_is_the_link() {
        let f = fx();
        fs::remove_file(&f.target).expect("rm");
        assert_eq!(
            create(&f.link, &f.target).expect("create"),
            LinkState::Linked
        );
        assert_eq!(inspect(&f.link, &f.target), LinkState::Linked);
    }

    #[test]
    fn a_file_where_a_folder_should_be_is_nothing_there() {
        let f = fx();
        let config_dir = f.link.parent().expect("rotten").to_path_buf();
        fs::create_dir_all(config_dir.parent().expect(".config")).expect("mkdir");
        fs::write(&config_dir, "no soy una carpeta").expect("write");
        assert_eq!(inspect(&f.link, &f.target), LinkState::Missing);
        assert!(create(&f.link, &f.target).is_err());
        assert_eq!(
            fs::read_to_string(&config_dir).expect("read"),
            "no soy una carpeta"
        );
    }
}
