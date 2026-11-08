//! Capa de plataforma de macOS.
//!
//! Según el ADR-03 (§4.1, punto 3) todo el `unsafe` y el FFI de macOS viven
//! aquí; el resto de crates del workspace usan `#![forbid(unsafe_code)]`.
//! De momento solo contiene la enumeración de procesos en ejecución, que
//! `ra-safety` usa para no tocar datos de apps abiertas (RF-SEG-08).

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub mod files;
pub mod iopolicy;

/// Rutas de los ejecutables de todos los procesos visibles para el usuario.
///
/// Devuelve error si la lista no se puede obtener o sale vacía: quien llame
/// debe tratarlo como «no determinable», que cuenta como «en uso».
pub fn running_executable_paths() -> io::Result<Vec<PathBuf>> {
    let pids = list_all_pids()?;
    let mut paths = Vec::with_capacity(pids.len());
    for pid in pids {
        // Los procesos de otros usuarios o los que ya han terminado fallan
        // aquí; no son relevantes para los datos del usuario actual.
        if let Some(path) = pid_path(pid) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(io::Error::other("no se pudo leer ningún proceso"));
    }
    Ok(paths)
}

/// Bundle IDs de las apps (`.app`) que tienen algún proceso en ejecución.
///
/// Para un ejecutable dentro de varios bundles anidados (p. ej. los helpers
/// de Chrome) se añaden todos: el helper y la app que lo contiene.
pub fn running_app_bundle_ids() -> io::Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut seen_bundles = BTreeSet::new();
    for exe in running_executable_paths()? {
        for bundle in enclosing_app_bundles(&exe) {
            if seen_bundles.insert(bundle.clone())
                && let Some(id) = bundle_identifier(&bundle)
            {
                ids.insert(id);
            }
        }
    }
    Ok(ids)
}

/// Bundles `.app` que contienen a `exe`, del más externo al más interno.
pub fn enclosing_app_bundles(exe: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for comp in exe.components() {
        acc.push(comp);
        let name = comp.as_os_str().as_bytes();
        if name.len() > 4 && name.ends_with(b".app") && acc != exe {
            out.push(acc.clone());
        }
    }
    out
}

/// `CFBundleIdentifier` del `Info.plist` de un bundle, si se puede leer.
pub fn bundle_identifier(bundle: &Path) -> Option<String> {
    let info = bundle.join("Contents").join("Info.plist");
    let value = plist::Value::from_file(info).ok()?;
    value
        .as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(str::to_owned)
}

#[allow(unsafe_code)]
fn list_all_pids() -> io::Result<Vec<libc::pid_t>> {
    // Primera llamada con búfer nulo: devuelve el número de procesos.
    // SAFETY: con un búfer nulo y tamaño 0 la función solo cuenta.
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return Err(io::Error::last_os_error());
    }
    // Margen por si aparecen procesos entre las dos llamadas.
    let capacity = n as usize + 64;
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    let bytes = (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `pids` tiene `capacity` elementos y le pasamos su tamaño exacto
    // en bytes; la función escribe como mucho ese número de bytes.
    let got = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if got <= 0 {
        return Err(io::Error::last_os_error());
    }
    pids.truncate((got as usize).min(capacity));
    pids.retain(|&p| p > 0);
    Ok(pids)
}

#[allow(unsafe_code)]
fn pid_path(pid: libc::pid_t) -> Option<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buf` mide exactamente `PROC_PIDPATHINFO_MAXSIZE` bytes, el
    // tamaño que se declara; la función escribe una cadena terminada en NUL.
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if len <= 0 {
        return None;
    }
    buf.truncate(len as usize);
    Some(PathBuf::from(OsStr::from_bytes(&buf)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_bundles_are_listed_outermost_first() {
        let exe = Path::new(
            "/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper",
        );
        let bundles = enclosing_app_bundles(exe);
        assert_eq!(bundles.len(), 2);
        assert!(bundles[0].ends_with("Google Chrome.app"));
        assert!(bundles[1].ends_with("Google Chrome Helper.app"));
    }

    #[test]
    fn plain_binaries_have_no_bundle() {
        assert!(enclosing_app_bundles(Path::new("/usr/bin/true")).is_empty());
    }

    #[test]
    fn current_process_is_listed() {
        let me = std::env::current_exe().expect("ruta del ejecutable de la prueba");
        let me = std::fs::canonicalize(me).expect("ruta canónica");
        let paths = running_executable_paths().expect("lista de procesos");
        assert!(paths.iter().any(|p| p == &me), "{me:?} no está en la lista");
    }
}
