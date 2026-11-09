//! Colores de la salida para personas (07 §4, T-14).
//!
//! Hay colores solo si se cumplen todas estas condiciones:
//!
//! - la salida es para personas (sin `--json`, explícito o por tubería);
//! - ese flujo (stdout o stderr, cada uno por separado) es una terminal;
//! - no se ha pedido `--no-color`;
//! - `NO_COLOR` no está definida o está vacía (<https://no-color.org>:
//!   cualquier valor no vacío los quita).
//!
//! Pocos y con sentido: cabeceras en negrita, el resultado en verde, lo
//! omitido en amarillo y los errores en rojo. El texto es el mismo con o
//! sin colores: solo se envuelve la línea entera.

/// Variable estándar para quitar los colores.
pub const NO_COLOR_ENV: &str = "NO_COLOR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Green,
    Yellow,
    Red,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Style::Bold => "1",
            Style::Green => "32",
            Style::Yellow => "33",
            Style::Red => "31",
        }
    }
}

/// ¿Colores en un flujo? `no_color_env` es el valor de `NO_COLOR`, si está.
pub fn enabled(
    is_terminal: bool,
    json: bool,
    no_color_flag: bool,
    no_color_env: Option<&str>,
) -> bool {
    is_terminal && !json && !no_color_flag && no_color_env.is_none_or(str::is_empty)
}

/// `NO_COLOR` del entorno (`None` si no está o no es UTF-8 válido, que
/// también cuenta como definida y no vacía).
pub fn no_color_env() -> Option<String> {
    std::env::var_os(NO_COLOR_ENV).map(|v| match v.into_string() {
        Ok(s) => s,
        Err(_) => "1".to_owned(),
    })
}

/// `text` con el estilo si `on`; si no, tal cual.
pub fn paint(on: bool, style: Style, text: &str) -> String {
    if on && !text.is_empty() {
        format!("\x1b[{}m{text}\x1b[0m", style.code())
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_only_on_a_terminal_without_json_or_no_color() {
        assert!(enabled(true, false, false, None));
        assert!(
            enabled(true, false, false, Some("")),
            "NO_COLOR vacía no cuenta"
        );
        assert!(!enabled(false, false, false, None), "tubería o fichero");
        assert!(!enabled(true, true, false, None), "--json");
        assert!(!enabled(true, false, true, None), "--no-color");
        assert!(!enabled(true, false, false, Some("1")), "NO_COLOR=1");
        assert!(
            !enabled(true, false, false, Some("0")),
            "cualquier valor no vacío"
        );
    }

    #[test]
    fn paint_wraps_the_whole_text_or_nothing() {
        assert_eq!(paint(false, Style::Red, "error"), "error");
        assert_eq!(paint(true, Style::Red, "error"), "\x1b[31merror\x1b[0m");
        assert_eq!(paint(true, Style::Bold, ""), "");
    }
}
