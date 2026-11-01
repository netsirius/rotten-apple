//! Cerrojo de ejecución: un solo ejecutor a la vez (05 §4.2, RNF-ARQ-20).

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Mientras exista, ningún otro proceso puede ejecutar planes ni deshacer.
#[derive(Debug)]
pub struct ExecLock {
    _file: File,
}

impl ExecLock {
    /// Toma `exec.lock` en el directorio de estado sin esperar. Devuelve
    /// `Ok(None)` si otro ejecutor lo tiene.
    pub fn try_acquire(state_dir: &Path) -> io::Result<Option<Self>> {
        fs::create_dir_all(state_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(state_dir.join("exec.lock"))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
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
}
