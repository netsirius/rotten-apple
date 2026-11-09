//! `rotten completion <zsh|bash|fish>` (T-14, 07 §2): el script de
//! autocompletado, generado con `clap_complete` a partir de la misma
//! definición que la ayuda. Completa subcomandos, opciones y valores.
//!
//! - La salida es siempre el script, también en una tubería (`rotten
//!   completion zsh > _rotten` es el uso normal): no hay versión JSON, como
//!   en `--version` y `--help`.
//! - Las descripciones salen en el idioma de la ayuda (`--lang`, `LANG`).
//! - No necesita la carpeta personal ni el núcleo: funciona aunque falte
//!   `HOME` o la configuración esté corrupta, y no deja nada en el log.
//!
//! Instalación: `rotten completion zsh > ~/.zfunc/_rotten` (con `~/.zfunc`
//! en `fpath`), `rotten completion bash > ~/.local/share/bash-completion/
//! completions/rotten` o `rotten completion fish > ~/.config/fish/
//! completions/rotten.fish`.

use std::io::{self, Write};

use clap_complete::Shell;

use crate::Exit;
use crate::args::{self, CompletionShell};
use crate::i18n::I18n;

/// Nombre del binario en los scripts.
const BIN: &str = "rotten";

/// El script de `shell`, con la ayuda en el idioma de `i18n`.
pub fn script(shell: CompletionShell, i18n: &I18n) -> Vec<u8> {
    let shell = match shell {
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Fish => Shell::Fish,
    };
    let mut cmd = args::localized(i18n);
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut cmd, BIN, &mut out);
    out
}

pub fn run(shell: CompletionShell, i18n: &I18n) -> Exit {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(&script(shell, i18n))
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Exit::Ok,
        // Tubería cerrada (`| head`) o disco lleno: no hay a quién avisar.
        Err(_) => Exit::Error,
    }
}
