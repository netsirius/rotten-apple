//! Ficheros abiertos por procesos visibles para el usuario (RF-SEG-08).
//!
//! Recorre los descriptores de cada proceso con `proc_pidinfo`
//! (`PROC_PIDLISTFDS`) y `proc_pidfdinfo` (`PROC_PIDFDVNODEPATHINFO`). Los
//! procesos de otros usuarios fallan con `EPERM` y se ignoran: solo cuentan
//! los que el usuario puede ver. Se descartan los descriptores abiertos con
//! `O_EVTONLY` (Finder, FSEvents y similares solo vigilan cambios) y los del
//! propio proceso, que es quien va a actuar.
//!
//! Recorrer todos los procesos cuesta decenas de milisegundos, así que se
//! toma una instantánea por pasada (como `AppProbe`) y cada elemento se
//! comprueba en memoria.

use std::ffi::{CStr, OsStr};
use std::io;
use std::mem::size_of;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::{list_all_pids, pid_path};

/// Proceso que tiene abierto un fichero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: i32,
    /// Nombre del ejecutable, o `?` si no se puede leer.
    pub process: String,
}

#[derive(Debug, Clone)]
struct OpenVnode {
    pid: i32,
    dev: u32,
    ino: u64,
    path: PathBuf,
}

/// Instantánea de los ficheros abiertos por los procesos visibles.
#[derive(Debug, Clone, Default)]
pub struct OpenFiles {
    vnodes: Vec<OpenVnode>,
}

impl OpenFiles {
    /// Devuelve error si la lista de procesos no se puede obtener: quien
    /// llame debe tratarlo como «no determinable», que cuenta como «en uso».
    pub fn snapshot() -> io::Result<Self> {
        let me = std::process::id() as libc::pid_t;
        let mut vnodes = Vec::new();
        for pid in list_all_pids()? {
            if pid == me {
                continue;
            }
            // Un proceso que no se puede inspeccionar es de otro usuario o
            // ya ha terminado: no es visible para el usuario.
            let Some(fds) = list_fds(pid) else { continue };
            for fd in fds {
                if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
                    continue;
                }
                let Some(v) = vnode_info(pid, fd.proc_fd) else {
                    continue;
                };
                if v.pfi.fi_openflags & libc::O_EVTONLY as u32 != 0 {
                    continue;
                }
                let st = &v.pvip.vip_vi.vi_stat;
                vnodes.push(OpenVnode {
                    pid,
                    dev: st.vst_dev,
                    ino: st.vst_ino,
                    path: vnode_path(&v),
                });
            }
        }
        Ok(Self { vnodes })
    }

    /// Primer proceso que tiene abierto `path` o algo debajo de él.
    ///
    /// Coincide un descriptor cuyo vnode tiene el mismo `(dev, ino)` que el
    /// elemento (también un enlace duro abierto por otra ruta) o cuya ruta
    /// está bajo `path`. `path` debe ser canónica, como las del plan.
    pub fn holder(&self, path: &Path, dev: u64, ino: u64) -> Option<Holder> {
        // `st_dev` es un `i32` que llega aquí extendido con signo; el kernel
        // da los mismos 32 bits como `u32`.
        let dev = dev as u32;
        let v = self
            .vnodes
            .iter()
            .find(|v| (v.dev == dev && v.ino == ino) || v.path.starts_with(path))?;
        let process = pid_path(v.pid)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "?".to_owned());
        Some(Holder {
            pid: v.pid,
            process,
        })
    }
}

/// `struct proc_fileinfo` de `<sys/proc_info.h>` (no está en `libc`).
#[repr(C)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: libc::off_t,
    fi_type: i32,
    fi_guardflags: u32,
}

/// `struct vnode_fdinfowithpath` de `<sys/proc_info.h>`.
#[repr(C)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileInfo,
    pvip: libc::vnode_info_path,
}

/// `PROC_PIDFDVNODEPATHINFO` de `<sys/proc_info.h>`.
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;

// Tamaños de las cabeceras del SDK: si no cuadran, el kernel rechaza la
// llamada o escribe fuera del búfer.
const _: () = assert!(size_of::<ProcFileInfo>() == 24);
const _: () = assert!(size_of::<VnodeFdInfoWithPath>() == 1200);

#[allow(unsafe_code)]
fn list_fds(pid: libc::pid_t) -> Option<Vec<libc::proc_fdinfo>> {
    // Con búfer nulo, `proc_pidinfo` devuelve los bytes necesarios.
    // SAFETY: con un búfer nulo y tamaño 0 la función no escribe nada.
    let need =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if need <= 0 {
        return None;
    }
    // Margen por si el proceso abre más descriptores entre las dos llamadas.
    let capacity = need as usize / size_of::<libc::proc_fdinfo>() + 32;
    let mut fds: Vec<libc::proc_fdinfo> = Vec::with_capacity(capacity);
    let bytes = (capacity * size_of::<libc::proc_fdinfo>()) as libc::c_int;
    // SAFETY: `fds` tiene capacidad para `capacity` elementos y se declara
    // ese tamaño exacto en bytes; la función escribe como mucho esos bytes.
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            bytes,
        )
    };
    if got <= 0 {
        return None;
    }
    let n = (got as usize / size_of::<libc::proc_fdinfo>()).min(capacity);
    // SAFETY: la función ha inicializado `n` elementos completos.
    unsafe { fds.set_len(n) };
    Some(fds)
}

#[allow(unsafe_code)]
fn vnode_info(pid: libc::pid_t, fd: i32) -> Option<VnodeFdInfoWithPath> {
    let size = size_of::<VnodeFdInfoWithPath>() as libc::c_int;
    // SAFETY: la estructura es `repr(C)` y solo contiene enteros y arrays de
    // enteros, así que el patrón de ceros es un valor válido.
    let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
    // SAFETY: `info` mide exactamente `size` bytes, el tamaño que se declara.
    let got = unsafe {
        libc::proc_pidfdinfo(
            pid,
            fd,
            PROC_PIDFDVNODEPATHINFO,
            (&raw mut info).cast(),
            size,
        )
    };
    (got == size).then_some(info)
}

/// Ruta del vnode (terminada en NUL dentro de `vip_path`).
fn vnode_path(v: &VnodeFdInfoWithPath) -> PathBuf {
    let raw = v.pvip.vip_path.as_flattened();
    // `c_char` es `i8` en arm64 de Apple: se reinterpreta byte a byte.
    let bytes: Vec<u8> = raw.iter().map(|&c| c as u8).collect();
    match CStr::from_bytes_until_nul(&bytes) {
        Ok(c) => PathBuf::from(OsStr::from_bytes(c.to_bytes())),
        Err(_) => PathBuf::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::process::{Command, Stdio};

    fn holder(p: &Path) -> Option<Holder> {
        let m = std::fs::symlink_metadata(p).expect("lstat");
        OpenFiles::snapshot()
            .expect("lista")
            .holder(p, m.dev() as u64, m.ino())
    }

    #[test]
    fn child_holding_a_file_is_found_and_released() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canon");
        let dir = root.join("cache");
        std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
        let file = dir.join("sub/data.bin");
        std::fs::write(&file, b"x").expect("write");
        let other = root.join("otro");
        std::fs::write(&other, b"y").expect("write");
        assert_eq!(holder(&file), None);

        let held = std::fs::File::open(&file).expect("abrir");
        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::from(held))
            .spawn()
            .expect("lanzar sleep");
        let pid = child.id() as i32;

        let h = holder(&file);
        // La carpeta que lo contiene también está en uso; otro fichero, no.
        let in_dir = holder(&dir);
        let unrelated = holder(&other);
        child.kill().expect("kill");
        child.wait().expect("wait");

        assert_eq!(
            h,
            Some(Holder {
                pid,
                process: "sleep".into()
            })
        );
        assert_eq!(in_dir.map(|h| h.pid), Some(pid));
        assert_eq!(unrelated, None);
        assert_eq!(holder(&file), None);
    }

    #[test]
    fn own_descriptors_do_not_count() {
        let tmp = tempfile::NamedTempFile::new().expect("temp");
        let path = std::fs::canonicalize(tmp.path()).expect("canon");
        assert_eq!(holder(&path), None);
    }
}
