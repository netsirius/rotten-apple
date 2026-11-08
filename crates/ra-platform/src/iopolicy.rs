//! Políticas de E/S del proceso (`setiopolicy_np(3)`): RF-SEG-10 y
//! RNF-ARQ-09.
//!
//! Se fijan una vez, con ámbito de proceso (`IOPOL_SCOPE_PROCESS`), al crear
//! el núcleo. Afectan a todos los hilos, también a los que ya existen, y los
//! heredan los procesos hijos.
//!
//! | Tipo | Valor | Por qué |
//! |---|---|---|
//! | `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES` | `OFF` | No descargar ficheros de iCloud/File Provider (RF-SEG-10) |
//! | `IOPOL_TYPE_DISK` | `IOPOL_UTILITY` | E/S de prioridad baja (RNF-ARQ-09) |
//! | `IOPOL_TYPE_VFS_ATIME_UPDATES` | `OFF` | El escaneo no altera el `atime` (RNF-ARQ-09) |
//!
//! Verificado en el SDK de macOS 27 (`MacOSX27.0.sdk/usr/include/sys/resource.h`)
//! y en macOS 27.0.1 (26A434):
//!
//! - Las constantes de abajo coinciden con las del SDK. El crate `libc` no
//!   declara estas funciones ni sus constantes, así que se declaran aquí.
//! - Al arrancar, el proceso hereda `MATERIALIZE_DATALESS_FILES = ON` (2),
//!   aunque el manual dice que el valor por defecto del sistema es `OFF`. Por
//!   eso hay que fijarla explícitamente.
//! - `setiopolicy_np` devuelve 0; después, `getiopolicy_np` devuelve el valor
//!   fijado (1, 4 y 1). Repetir la llamada es inocuo.
//! - El único error documentado y observado es `EINVAL` (tipo, ámbito o
//!   valor desconocidos), que solo puede darse en un kernel que no conozca la
//!   constante.
//! - En APFS el `atime` solo se actualiza si es anterior al `mtime`. En ese
//!   caso `read` y `readdir` lo cambian con la política por defecto y no lo
//!   cambian con `IOPOL_ATIME_UPDATES_OFF`. `lstat` nunca lo cambia.

use std::io;

use libc::c_int;

const IOPOL_TYPE_DISK: c_int = 0;
const IOPOL_TYPE_VFS_ATIME_UPDATES: c_int = 2;
const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;

const IOPOL_SCOPE_PROCESS: c_int = 0;

/// `IOPOL_UTILITY`: E/S en segundo plano, cedida ante la del usuario.
pub const IOPOL_UTILITY: c_int = 4;
/// `IOPOL_ATIME_UPDATES_OFF`.
pub const IOPOL_ATIME_UPDATES_OFF: c_int = 1;
/// `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`.
pub const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;

#[allow(unsafe_code)]
unsafe extern "C" {
    fn getiopolicy_np(iotype: c_int, scope: c_int) -> c_int;
    fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
}

/// Valores de las políticas del proceso, tal como los devuelve
/// `getiopolicy_np`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessPolicies {
    pub materialize_dataless_files: c_int,
    pub disk: c_int,
    pub atime_updates: c_int,
}

impl ProcessPolicies {
    /// Lo que deja fijado [`apply`].
    pub const TARGET: Self = Self {
        materialize_dataless_files: IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
        disk: IOPOL_UTILITY,
        atime_updates: IOPOL_ATIME_UPDATES_OFF,
    };
}

/// Fija las tres políticas para todo el proceso.
///
/// Solo devuelve error si no se ha podido desactivar la materialización de
/// ficheros sin descargar. Es un control de seguridad (RF-SEG-10, «Must») y,
/// como el proceso hereda `ON`, un fallo dejaría las descargas permitidas
/// ante cualquier apertura accidental: quien llame debe abortar. En la
/// práctica no falla en ningún macOS soportado.
///
/// La prioridad de E/S y el `atime` (RNF-ARQ-09) son de rendimiento e
/// higiene, no de seguridad: si fallan, se sigue sin ellos.
pub fn apply() -> io::Result<()> {
    set_process(
        IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
        IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
    )?;
    // Sin canal de avisos todavía (el log técnico es T-14): se ignoran.
    let _ = set_process(IOPOL_TYPE_DISK, IOPOL_UTILITY);
    let _ = set_process(IOPOL_TYPE_VFS_ATIME_UPDATES, IOPOL_ATIME_UPDATES_OFF);
    Ok(())
}

/// Políticas vigentes del proceso, leídas con `getiopolicy_np`.
pub fn current() -> io::Result<ProcessPolicies> {
    Ok(ProcessPolicies {
        materialize_dataless_files: get_process(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES)?,
        disk: get_process(IOPOL_TYPE_DISK)?,
        atime_updates: get_process(IOPOL_TYPE_VFS_ATIME_UPDATES)?,
    })
}

#[allow(unsafe_code)]
fn set_process(iotype: c_int, policy: c_int) -> io::Result<()> {
    // SAFETY: solo recibe enteros por valor, sin punteros ni memoria
    // compartida; el kernel valida los tres y, si no los conoce, devuelve -1
    // con `errno = EINVAL` sin cambiar nada.
    let r = unsafe { setiopolicy_np(iotype, IOPOL_SCOPE_PROCESS, policy) };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[allow(unsafe_code)]
fn get_process(iotype: c_int) -> io::Result<c_int> {
    // SAFETY: solo recibe enteros por valor y no escribe en memoria del
    // llamante; devuelve -1 con `errno` si el tipo o el ámbito no existen.
    let r = unsafe { getiopolicy_np(iotype, IOPOL_SCOPE_PROCESS) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_type_is_an_error_not_a_panic() {
        let err = set_process(99, 0).expect_err("tipo desconocido");
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
        let err = get_process(99).expect_err("tipo desconocido");
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
    }
}
