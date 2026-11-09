//! Árboles de ficheros fixture (05 §10.2). Solo con la feature `fixtures`.
//!
//! Un fixture describe ficheros relativos a una carpeta personal de prueba,
//! con tamaño y antigüedad, más enlaces simbólicos. Al materializarlo, cada
//! carpeta recibe como `mtime` el del fichero más reciente que contiene,
//! para que las antigüedades del árbol sean coherentes.

use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Ruta relativa a la carpeta personal.
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub age_days: u32,
    /// Carpeta vacía en lugar de fichero.
    #[serde(default)]
    pub dir: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub path: String,
    /// Destino relativo a la carpeta personal.
    pub target: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Candidatos sin bloqueo (relativos a la carpeta personal).
    pub candidates: Vec<String>,
    /// Candidatos bloqueados (demasiado recientes, app abierta…).
    #[serde(default)]
    pub blocked: Vec<String>,
    /// Subconjunto de `candidates` que debe salir preseleccionado.
    #[serde(default)]
    pub preselected: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    #[serde(default)]
    pub running_apps: Vec<String>,
    #[serde(default)]
    pub entry: Vec<Entry>,
    #[serde(default)]
    pub symlink: Vec<Link>,
    #[serde(default)]
    pub expect: Expect,
}

fn set_mtime(path: &Path, t: SystemTime) -> io::Result<()> {
    File::open(path)?.set_times(FileTimes::new().set_modified(t).set_accessed(t))
}

/// Crea el árbol bajo `home`. `now` es la referencia de las antigüedades.
pub fn materialize(
    home: &Path,
    entries: &[Entry],
    links: &[Link],
    now: SystemTime,
) -> io::Result<()> {
    let day = Duration::from_secs(86_400);
    // Antigüedad mínima (más reciente) de cada carpeta, por su contenido.
    let mut dir_age: BTreeMap<PathBuf, u32> = BTreeMap::new();
    let mut note_dirs = |rel: &Path, age: u32, include_self: bool| {
        let mut cur = if include_self {
            Some(rel)
        } else {
            rel.parent()
        };
        while let Some(d) = cur {
            if d.as_os_str().is_empty() {
                break;
            }
            let e = dir_age.entry(home.join(d)).or_insert(age);
            *e = (*e).min(age);
            cur = d.parent();
        }
    };
    for e in entries {
        let rel = Path::new(&e.path);
        let abs = home.join(rel);
        if e.dir {
            fs::create_dir_all(&abs)?;
            note_dirs(rel, e.age_days, true);
        } else {
            if let Some(p) = abs.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(&abs, vec![0u8; e.size as usize])?;
            set_mtime(&abs, now - day * e.age_days)?;
            note_dirs(rel, e.age_days, false);
        }
    }
    for l in links {
        let rel = Path::new(&l.path);
        let abs = home.join(rel);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p)?;
        }
        std::os::unix::fs::symlink(home.join(&l.target), &abs)?;
        note_dirs(rel, 0, false);
    }
    // De la más profunda a la menos: fijar el mtime de una carpeta no cambia
    // el de su padre, pero crear hijos sí, así que se hace al final.
    let mut dirs: Vec<_> = dir_age.into_iter().collect();
    dirs.sort_by_key(|(p, _)| std::cmp::Reverse(p.components().count()));
    for (dir, age) in dirs {
        set_mtime(&dir, now - day * age)?;
    }
    Ok(())
}

impl Fixture {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn materialize(&self, home: &Path, now: SystemTime) -> io::Result<()> {
        materialize(home, &self.entry, &self.symlink, now)
    }
}
