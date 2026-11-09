//! Textos para personas (RF-CLI-09, RNF-24): catálogo de mensajes Fluent
//! por idioma (`locales/*.ftl`), con plurales, y números, tamaños y fechas
//! según el idioma. El JSON nunca pasa por aquí (RNF-25).
//!
//! - Un id que falta o un argumento que no cuadra hace `panic!` en las
//!   compilaciones de depuración (las pruebas lo detectan); en la versión
//!   final se muestra el id.
//! - `ROTTEN_PSEUDO_LOCALE=1`: pseudo-localización. Textos en inglés con
//!   acentos, cada mensaje entre `⟦…⟧` y los argumentos aislados (U+2068
//!   … U+2069). Una línea que, sin los argumentos, no es `⟦…⟧` es un texto
//!   sin catálogo o concatenado.

use std::borrow::Cow;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

const ES_FTL: &str = include_str!("../locales/es.ftl");
const EN_FTL: &str = include_str!("../locales/en.ftl");

/// Variable de entorno de la pseudo-localización.
pub const PSEUDO_ENV: &str = "ROTTEN_PSEUDO_LOCALE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// `--lang`, o el idioma del sistema (`LC_ALL`, `LC_MESSAGES`, `LANG`).
    pub fn detect(flag: Option<&str>) -> Self {
        let pick = |s: &str| {
            if s.starts_with("es") {
                Lang::Es
            } else {
                Lang::En
            }
        };
        if let Some(f) = flag {
            return pick(f);
        }
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var)
                && !v.is_empty()
            {
                return pick(&v);
            }
        }
        Lang::En
    }

    /// Código para los textos del catálogo de reglas (`Localized::get`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }
}

pub struct I18n {
    lang: Lang,
    pseudo: bool,
    bundle: FluentBundle<FluentResource>,
}

/// Agrupa las cifras de un entero: `sep` cada tres, a partir de
/// `min_digits` cifras (CLDR: el español no agrupa los números de cuatro).
fn group(n: f64, sep: char, min_digits: usize) -> Option<String> {
    if n.fract() != 0.0 || n.abs() >= 1e15 {
        return None;
    }
    let digits = (n.abs() as u64).to_string();
    let mut out = String::new();
    if n < 0.0 {
        out.push('-');
    }
    if digits.len() < min_digits {
        out.push_str(&digits);
        return Some(out);
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    Some(out)
}

fn format_es<M>(v: &FluentValue, _: &M) -> Option<String> {
    match v {
        FluentValue::Number(n) => group(n.value, '.', 5),
        _ => None,
    }
}

fn format_en<M>(v: &FluentValue, _: &M) -> Option<String> {
    match v {
        FluentValue::Number(n) => group(n.value, ',', 4),
        _ => None,
    }
}

/// Acentos de la pseudo-localización: el texto sigue siendo legible y se
/// nota lo que no pasa por el catálogo.
fn pseudo_transform(s: &str) -> Cow<'_, str> {
    Cow::Owned(
        s.chars()
            .map(|c| match c {
                'a' => 'å',
                'e' => 'ë',
                'i' => 'ï',
                'o' => 'ø',
                'u' => 'ü',
                'A' => 'Å',
                'E' => 'Ë',
                'I' => 'Ï',
                'O' => 'Ø',
                'U' => 'Ü',
                c => c,
            })
            .collect(),
    )
}

fn bundle(lang: Lang, pseudo: bool) -> FluentBundle<FluentResource> {
    let (id, ftl): (LanguageIdentifier, &str) = match lang {
        Lang::Es => ("es".parse().expect("es"), ES_FTL),
        Lang::En => ("en".parse().expect("en"), EN_FTL),
    };
    let res = FluentResource::try_new(ftl.to_owned()).unwrap_or_else(|(r, errs)| {
        debug_assert!(errs.is_empty(), "{lang:?}.ftl no es válido: {errs:?}");
        r
    });
    let mut b = FluentBundle::new(vec![id]);
    if let Err(errs) = b.add_resource(res) {
        debug_assert!(errs.is_empty(), "{lang:?}.ftl: {errs:?}");
    }
    b.set_use_isolating(pseudo);
    match lang {
        Lang::Es => b.set_formatter(Some(format_es)),
        Lang::En => b.set_formatter(Some(format_en)),
    }
    if pseudo {
        b.set_transform(Some(pseudo_transform));
    }
    b
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        let pseudo = std::env::var(PSEUDO_ENV).is_ok_and(|v| v == "1");
        Self::with(lang, pseudo)
    }

    pub fn with(lang: Lang, pseudo: bool) -> Self {
        // La pseudo-localización parte del inglés.
        let lang = if pseudo { Lang::En } else { lang };
        I18n {
            lang,
            pseudo,
            bundle: bundle(lang, pseudo),
        }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// El mensaje `id` con sus argumentos.
    pub fn tr(&self, id: &str, args: Option<&FluentArgs>) -> String {
        let Some(pattern) = self.bundle.get_message(id).and_then(|m| m.value()) else {
            if cfg!(debug_assertions) {
                panic!("mensaje sin traducir: {id} ({:?})", self.lang);
            }
            return id.to_owned();
        };
        let mut errors = vec![];
        let text = self.bundle.format_pattern(pattern, args, &mut errors);
        if cfg!(debug_assertions) && !errors.is_empty() {
            panic!("mensaje {id} ({:?}): {errors:?}", self.lang);
        }
        if self.pseudo {
            format!("⟦{text}⟧")
        } else {
            text.into_owned()
        }
    }

    /// Separador decimal del idioma.
    fn decimal_separator(&self) -> &'static str {
        match self.lang {
            Lang::Es => ",",
            Lang::En => ".",
        }
    }

    /// Tamaño en unidades decimales, como Finder: KB sin decimales, MB con
    /// uno, GB y más con dos, sin ceros a la derecha.
    pub fn size(&self, bytes: u64) -> String {
        if bytes < 1000 {
            let mut a = FluentArgs::new();
            a.set("n", bytes);
            return self.tr("size-bytes", Some(&a));
        }
        const UNITS: [(&str, usize); 5] = [("KB", 0), ("MB", 1), ("GB", 2), ("TB", 2), ("PB", 2)];
        let mut v = bytes as f64 / 1000.0;
        let mut u = 0;
        loop {
            let rounded = format!("{v:.prec$}", prec = UNITS[u].1);
            // 999 999 B son «1 MB», no «1000 KB».
            if rounded.parse::<f64>().unwrap_or(v) >= 1000.0 && u + 1 < UNITS.len() {
                v /= 1000.0;
                u += 1;
                continue;
            }
            let mut s = rounded;
            if s.contains('.') {
                s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
            }
            let s = s.replace('.', self.decimal_separator());
            let mut a = FluentArgs::new();
            a.set("value", s);
            a.set("unit", UNITS[u].0);
            return self.tr("size-unit", Some(&a));
        }
    }

    /// Fecha y hora RFC 3339 (las del journal, en UTC) en la zona horaria
    /// local, con el orden del idioma. Si no se entiende, tal cual.
    pub fn date(&self, rfc3339: &str) -> String {
        match rfc3339.parse::<jiff::Timestamp>() {
            Ok(ts) => self.date_in(ts, &jiff::tz::TimeZone::system()),
            Err(_) => rfc3339.to_owned(),
        }
    }

    fn date_in(&self, ts: jiff::Timestamp, tz: &jiff::tz::TimeZone) -> String {
        let z = ts.to_zoned(tz.clone());
        let fmt = match self.lang {
            Lang::Es => "%-d/%-m/%Y %H:%M",
            Lang::En => "%-m/%-d/%Y %-I:%M %p",
        };
        z.strftime(fmt).to_string()
    }
}

/// `tr!(ui, "id")` o `tr!(ui, "id", nombre = valor, …)`: el mensaje `id`
/// del catálogo con sus argumentos (enteros para los plurales, textos para
/// lo demás). `ui` es cualquier cosa con un método `tr` como el de [`I18n`].
macro_rules! tr {
    ($ui:expr, $id:literal $(,)?) => {
        $ui.tr($id, None)
    };
    ($ui:expr, $id:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut args = ::fluent_bundle::FluentArgs::new();
        $( args.set(stringify!($k), $v); )+
        $ui.tr($id, Some(&args))
    }};
}
pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ftl: &str) -> std::collections::BTreeSet<String> {
        ftl.lines()
            .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_lowercase()))
            .filter_map(|l| l.split_once(" =").map(|(id, _)| id.trim().to_owned()))
            .collect()
    }

    #[test]
    fn both_catalogs_parse_and_have_the_same_messages() {
        for ftl in [ES_FTL, EN_FTL] {
            if let Err((_, errs)) = FluentResource::try_new(ftl.to_owned()) {
                panic!("{errs:?}");
            }
        }
        let (es, en) = (ids(ES_FTL), ids(EN_FTL));
        assert!(en.len() > 50, "{}", en.len());
        assert_eq!(
            es.symmetric_difference(&en).collect::<Vec<_>>(),
            Vec::<&String>::new(),
            "es.ftl y en.ftl deben tener los mismos mensajes"
        );
    }

    /// Todo `tr!(…, "id"` del código existe en el catálogo (los ids
    /// dinámicos, como los de la ayuda, los cubren las pruebas del binario).
    #[test]
    fn every_message_used_in_the_code_exists() {
        let en = ids(EN_FTL);
        // `main.rs`, `render.rs` y todos los `cmd/*.rs`: un comando nuevo
        // entra solo.
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src_dir.join("main.rs"), src_dir.join("render.rs")];
        let mut cmds: Vec<_> = std::fs::read_dir(src_dir.join("cmd"))
            .expect("src/cmd")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .collect();
        cmds.sort();
        assert!(
            cmds.iter().any(|p| p.ends_with("cmd/config.rs")),
            "{cmds:?}"
        );
        files.extend(cmds);
        let sources: Vec<String> = files
            .iter()
            .map(|p| std::fs::read_to_string(p).expect("leer fuente"))
            .collect();
        let mut used = 0;
        for src in &sources {
            for (i, _) in src.match_indices("tr!(") {
                let rest = &src[i..];
                let Some(q) = rest.find('"') else { continue };
                let id: String = rest[q + 1..].chars().take_while(|c| *c != '"').collect();
                if id == "id" {
                    continue; // el ejemplo de la documentación
                }
                assert!(en.contains(&id), "falta el mensaje «{id}»");
                used += 1;
            }
        }
        assert!(used > 50, "{used}");
    }

    #[test]
    fn sizes_like_finder() {
        let en = I18n::with(Lang::En, false);
        let es = I18n::with(Lang::Es, false);
        assert_eq!(en.size(0), "0 bytes");
        assert_eq!(en.size(1), "1 byte");
        assert_eq!(es.size(1), "1 byte");
        assert_eq!(es.size(999), "999 bytes");
        assert_eq!(en.size(1_000), "1 KB");
        assert_eq!(en.size(1_499), "1 KB");
        assert_eq!(en.size(12_600), "13 KB");
        assert_eq!(en.size(999_999), "1 MB");
        assert_eq!(en.size(1_260_000), "1.3 MB");
        assert_eq!(es.size(1_260_000), "1,3 MB");
        assert_eq!(es.size(2_000_000), "2 MB");
        assert_eq!(en.size(1_234_000_000), "1.23 GB");
        assert_eq!(es.size(1_234_000_000), "1,23 GB");
        assert_eq!(es.size(1_500_000_000), "1,5 GB");
        assert_eq!(en.size(3_000_000_000_000), "3 TB");
    }

    #[test]
    fn counts_and_plurals_by_language() {
        let en = I18n::with(Lang::En, false);
        let es = I18n::with(Lang::Es, false);
        assert_eq!(tr!(en, "count-items", count = 1234), "1,234 items");
        assert_eq!(tr!(es, "count-items", count = 1234), "1234 elementos");
        assert_eq!(tr!(en, "count-items", count = 1_234_567), "1,234,567 items");
        assert_eq!(tr!(en, "count-items", count = 1), "1 item");
        assert_eq!(tr!(en, "count-items", count = 12_345), "12,345 items");
        assert_eq!(tr!(es, "count-items", count = 1), "1 elemento");
        assert_eq!(tr!(es, "count-items", count = 0), "0 elementos");
        assert_eq!(tr!(es, "count-items", count = 12_345), "12.345 elementos");
    }

    #[test]
    fn dates_in_local_order() {
        let ts: jiff::Timestamp = "2026-03-07T15:04:05.123Z".parse().expect("fecha");
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(
            I18n::with(Lang::Es, false).date_in(ts, &utc),
            "7/3/2026 15:04"
        );
        assert_eq!(
            I18n::with(Lang::En, false).date_in(ts, &utc),
            "3/7/2026 3:04 PM"
        );
        assert_eq!(
            I18n::with(Lang::En, false).date("no es una fecha"),
            "no es una fecha"
        );
    }

    #[test]
    fn pseudo_locale_marks_lines_and_isolates_arguments() {
        let p = I18n::with(Lang::Es, true);
        let s = tr!(p, "count-items", count = 3);
        assert!(s.starts_with('⟦') && s.ends_with('⟧'), "{s}");
        assert!(s.contains('\u{2068}') && s.contains('\u{2069}'), "{s:?}");
        assert!(s.contains("ïtëms"), "{s}");
    }

    /// RF-SEG-03: la confirmación de borrar para siempre dice cuántos
    /// elementos, cuánto ocupan y que no se puede deshacer, y la respuesta
    /// por defecto es «No» (`[s/N]`, `[y/N]`; `Ui::confirm` solo acepta un
    /// «sí» explícito).
    #[test]
    fn permanent_confirmations_are_explicit_and_default_to_no() {
        for (lang, count, undo, default) in [
            (Lang::Es, "3 elementos", "No se puede deshacer", "[s/N]"),
            (Lang::En, "3 items", "cannot be undone", "[y/N]"),
        ] {
            let i = I18n::with(lang, false);
            // `rotten free` y `rotten trash empty` (T-12).
            for q in [
                tr!(i, "confirm-free", count = 3, size = "2 MB"),
                tr!(i, "confirm-empty", count = 3, size = "2 MB"),
            ] {
                assert!(
                    q.contains(count) && q.contains("2 MB") && q.contains(undo),
                    "{q}"
                );
                let prompt = tr!(i, "confirm-prompt", question = q);
                assert!(prompt.ends_with(default), "{prompt}");
            }
        }
    }

    #[test]
    fn language_detection() {
        assert_eq!(Lang::detect(Some("es")), Lang::Es);
        assert_eq!(Lang::detect(Some("en")), Lang::En);
    }
}
