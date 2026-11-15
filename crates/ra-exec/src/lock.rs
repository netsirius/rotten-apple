//! Cerrojo de ejecución: un solo ejecutor a la vez (05 §4.2, RNF-ARQ-20).

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// Cuánto se reintenta ante un cerrojo ocupado antes de darlo por tomado.
const BUSY_GRACE: Duration = Duration::from_millis(100);
/// Espera entre intentos.
const BUSY_STEP: Duration = Duration::from_millis(5);

/// Mientras exista, ningún otro proceso puede ejecutar planes ni deshacer.
#[derive(Debug)]
pub struct ExecLock {
    _file: File,
}

impl ExecLock {
    /// Toma `exec.lock` en el directorio de estado sin esperar a otro
    /// ejecutor. Devuelve `Ok(None)` si otro ejecutor lo tiene.
    ///
    /// Ante un cerrojo ocupado se reintenta con esperas cortas durante
    /// [`BUSY_GRACE`] (100 ms) antes de rendirse. El motivo: un `flock`
    /// pertenece a la descripción de fichero abierta, y un proceso hijo
    /// lanzado con `Command` desde otro hilo hereda un instante todos los
    /// descriptores (hasta su `exec`, que cierra los `O_CLOEXEC`). Si en ese
    /// instante este proceso suelta el cerrojo y lo vuelve a pedir, el hijo
    /// aún lo mantiene vivo y `try_lock` da `WouldBlock` aunque nadie lo
    /// tenga de verdad. Así fallaba de vez en cuando una prueba que ejecuta y
    /// deshace seguido mientras otra lanza procesos, y también lo puede ver
    /// un `config set` justo después de un `doctor`. Un cerrojo tomado de
    /// verdad sigue dando `None`, solo que tras ese margen.
    pub fn try_acquire(state_dir: &Path) -> io::Result<Option<Self>> {
        fs::create_dir_all(state_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(state_dir.join("exec.lock"))?;
        let deadline = Instant::now() + BUSY_GRACE;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Some(Self { _file: file })),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(BUSY_STEP);
                }
                Err(TryLockError::WouldBlock) => return Ok(None),
                Err(TryLockError::Error(e)) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_is_refused_until_released() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = ExecLock::try_acquire(dir.path())
            .expect("io")
            .expect("libre");
        assert!(ExecLock::try_acquire(dir.path()).expect("io").is_none());
        drop(a);
        assert!(ExecLock::try_acquire(dir.path()).expect("io").is_some());
    }

    /// Un cerrojo tomado de verdad da `None` tras el margen, no antes.
    #[test]
    fn a_held_lock_is_refused_after_the_grace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _a = ExecLock::try_acquire(dir.path())
            .expect("io")
            .expect("libre");
        let start = Instant::now();
        assert!(ExecLock::try_acquire(dir.path()).expect("io").is_none());
        assert!(start.elapsed() >= BUSY_GRACE);
    }

    /// Hijos lanzados desde otros hilos heredan un instante el descriptor
    /// del cerrojo: tomarlo y soltarlo mientras tanto nunca debe dar `None`.
    #[test]
    fn children_spawned_elsewhere_do_not_make_the_lock_look_busy() {
        use std::process::Command;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::tempdir().expect("tempdir");
        let stop = AtomicBool::new(false);
        thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = Command::new("/usr/bin/true").status();
                    }
                });
            }
            let mut busy = 0;
            for _ in 0..300 {
                match ExecLock::try_acquire(dir.path()).expect("io") {
                    Some(lock) => drop(lock),
                    None => busy += 1,
                }
            }
            stop.store(true, Ordering::Relaxed);
            assert_eq!(busy, 0, "el cerrojo pareció ocupado {busy} veces");
        });
    }
}
