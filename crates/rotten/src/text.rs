//! Textos para personas en español e inglés (RF-CLI-09). La salida JSON no
//! depende del idioma.

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

    pub fn t<'a>(self, es: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Es => es,
            Lang::En => en,
        }
    }
}

/// Tamaño en unidades decimales, como Finder.
pub fn size(bytes: u64, lang: Lang) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    let s = if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    };
    match lang {
        Lang::Es => s.replace('.', ","),
        Lang::En => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0, Lang::En), "0 B");
        assert_eq!(size(999, Lang::En), "999 B");
        assert_eq!(size(1_500, Lang::En), "1.5 KB");
        assert_eq!(size(1_234_000_000, Lang::Es), "1,2 GB");
    }
}
