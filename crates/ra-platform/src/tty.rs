/// Descarta la entrada de la terminal de stdin recibida y aún no leída
/// (`tcflush(STDIN, TCIFLUSH)`, con `rustix`, sin `unsafe`). Si stdin no es
/// una terminal, `tcflush` falla con `ENOTTY` y no pasa nada: no hay nada
/// que descartar.
pub fn discard_pending_input() {
    let _ = rustix::termios::tcflush(std::io::stdin(), rustix::termios::QueueSelector::IFlush);
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
