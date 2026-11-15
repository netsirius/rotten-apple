//! Terminal de stdin.

/// Descarta la entrada de la terminal de stdin recibida y aún no leída
/// (`tcflush(STDIN, TCIFLUSH)`). Si stdin no es una terminal, `tcflush`
/// falla con `ENOTTY` y no pasa nada: no hay nada que descartar.
#[allow(unsafe_code)]
pub fn discard_pending_input() {
    // SAFETY: `tcflush` solo recibe un descriptor y una constante; no toca
    // memoria del proceso. Un descriptor que no es una terminal da error.
    let _ = unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sin terminal (las pruebas no tienen) no falla ni bloquea.
    #[test]
    fn without_a_terminal_it_does_nothing() {
        discard_pending_input();
    }
}
