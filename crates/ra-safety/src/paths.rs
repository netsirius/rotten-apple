//! Operaciones léxicas sobre rutas.
//!
//! Las comparaciones de protección y exclusión se hacen sobre componentes
//! «plegados»: normalización Unicode NFD + minúsculas. APFS suele ser
//! insensible a mayúsculas y conserva la normalización, así que plegar es la
//! opción conservadora (RF-SEG-06): puede proteger o excluir de más, nunca de
//! menos.

use std::path::{Component, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("la ruta no es absoluta: {0}")]
    NotAbsolute(String),
    #[error("la ruta contiene componentes «.» o «..»: {0}")]
    NotNormal(String),
    #[error("la ruta está vacía")]
    Empty,
}

/// Pliega un texto para compararlo: NFD + minúsculas.
pub fn fold(s: &str) -> String {
    s.nfd().collect::<String>().to_lowercase()
}

/// Componentes normales de una ruta, plegados. La raíz no aparece.
pub fn folded_components(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(fold(&s.to_string_lossy())),
            _ => None,
        })
        .collect()
}

/// `true` si `path` es `ancestor` o está dentro de él (comparación plegada).
pub fn is_within(path: &Path, ancestor: &Path) -> bool {
    let p = folded_components(path);
    let a = folded_components(ancestor);
    p.len() >= a.len() && p[..a.len()] == a[..]
}

/// Exige una ruta absoluta sin componentes `.` ni `..`.
///
/// No resuelve enlaces simbólicos: eso se hace al ejecutar, con operaciones
/// relativas a descriptor que fallan si algún componente es un enlace.
pub fn check_absolute_normal(p: &Path) -> Result<(), PathError> {
    if p.as_os_str().is_empty() {
        return Err(PathError::Empty);
    }
    if !p.is_absolute() {
        return Err(PathError::NotAbsolute(p.display().to_string()));
    }
    for c in p.components() {
        match c {
            Component::RootDir | Component::Normal(_) => {}
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(PathError::NotNormal(p.display().to_string()));
            }
        }
    }
    Ok(())
}

/// Expande `~` y `~/…` con la carpeta personal dada. Otras rutas deben ser
/// absolutas y normales.
pub fn expand_home(s: &str, home: &Path) -> Result<PathBuf, PathError> {
    let path = if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(s)
    };
    check_absolute_normal(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_ignores_case_and_normalization() {
        // «é» precompuesta (NFC) frente a «e» + acento combinante (NFD).
        assert_eq!(fold("Caf\u{e9}"), fold("CAFE\u{301}"));
    }

    #[test]
    fn within_is_component_wise() {
        assert!(is_within(Path::new("/a/b/c"), Path::new("/a/b")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(is_within(Path::new("/A/B/c"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a/bc"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a"), Path::new("/a/b")));
    }

    #[test]
    fn rejects_relative_and_dot_dot() {
        assert!(check_absolute_normal(Path::new("a/b")).is_err());
        assert!(check_absolute_normal(Path::new("/a/../b")).is_err());
        // `Path::components` ya descarta los «.» intermedios.
        assert!(check_absolute_normal(Path::new("/a/./b")).is_ok());
        assert!(check_absolute_normal(Path::new("/a/b")).is_ok());
    }

    #[test]
    fn expands_tilde() {
        let home = Path::new("/Users/ana");
        assert_eq!(
            expand_home("~/Library/Caches", home).ok(),
            Some(PathBuf::from("/Users/ana/Library/Caches"))
        );
        assert!(expand_home("Library", home).is_err());
    }
}
