//! Lints del catálogo y una prueba de fixture por regla (ADR-05 §6.4).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ra_safety::AppProbe;

use super::*;
use crate::fixture::Fixture;

/// Fixtures por id de regla, incrustados como las reglas.
const FIXTURES: &[(&str, &str)] = &[
    (
        "user-caches",
        include_str!("../../../catalog/fixtures/user-caches.toml"),
    ),
    (
        "user-logs",
        include_str!("../../../catalog/fixtures/user-logs.toml"),
    ),
    (
        "user-logs.diagnostic-reports",
        include_str!("../../../catalog/fixtures/user-logs.diagnostic-reports.toml"),
    ),
    (
        "browsers.safari.cache",
        include_str!("../../../catalog/fixtures/browsers.safari.cache.toml"),
    ),
    (
        "browsers.chrome.cache",
        include_str!("../../../catalog/fixtures/browsers.chrome.cache.toml"),
    ),
    (
        "browsers.chrome.profile-caches",
        include_str!("../../../catalog/fixtures/browsers.chrome.profile-caches.toml"),
    ),
    (
        "browsers.firefox.cache",
        include_str!("../../../catalog/fixtures/browsers.firefox.cache.toml"),
    ),
    (
        "saved-application-state",
        include_str!("../../../catalog/fixtures/saved-application-state.toml"),
    ),
];

#[test]
fn embedded_catalog_is_valid() {
    let c = Catalog::embedded().expect("catálogo incrustado válido");
    let cats: BTreeSet<_> = c.rules.iter().map(|r| r.cat.as_str()).collect();
    assert_eq!(
        cats,
        BTreeSet::from(["CAT-01", "CAT-03", "CAT-08", "CAT-10"])
    );
}

#[test]
fn every_rule_has_a_fixture_and_every_fixture_a_rule() {
    let c = Catalog::embedded().expect("catálogo");
    let rules: BTreeSet<_> = c.rules.iter().map(|r| r.id.as_str()).collect();
    let fixtures: BTreeSet<_> = FIXTURES.iter().map(|(id, _)| *id).collect();
    assert_eq!(rules, fixtures);
    // Y que el directorio no tenga fixtures huérfanos.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/fixtures");
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir)
        .expect("catalog/fixtures")
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".toml")
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(on_disk, fixtures.iter().map(|s| (*s).to_owned()).collect());
}

fn rel(home: &Path, p: &Path) -> String {
    p.strip_prefix(home)
        .expect("bajo home")
        .display()
        .to_string()
}

#[test]
fn every_rule_matches_its_fixture() {
    let catalog = Catalog::embedded().expect("catálogo");
    for (id, text) in FIXTURES {
        let fx = Fixture::parse(text).unwrap_or_else(|e| panic!("fixture {id}: {e}"));
        assert!(
            !fx.expect.candidates.is_empty(),
            "{id}: falta un caso positivo"
        );
        let positives: BTreeSet<_> = fx
            .expect
            .candidates
            .iter()
            .chain(&fx.expect.blocked)
            .collect();
        assert!(
            fx.entry
                .iter()
                .any(|e| !positives.iter().any(|p| e.path.starts_with(p.as_str()))),
            "{id}: falta un caso negativo"
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let home = std::fs::canonicalize(tmp.path()).expect("canon");
        let now = SystemTime::now();
        fx.materialize(&home, now).expect("materializar");
        let running: BTreeSet<String> = fx.running_apps.iter().cloned().collect();
        let apps = AppProbe::snapshot(&running);
        let only = catalog.filtered(|r| r.id == *id);
        let res = only.scan(&ScanOptions {
            home: &home,
            apps: &apps,
            now,
        });

        let mut ok = BTreeSet::new();
        let mut blocked = BTreeSet::new();
        let mut pre = BTreeSet::new();
        for c in &res.candidates {
            let r = rel(&home, &c.path);
            if c.block.is_some() {
                blocked.insert(r);
            } else {
                if c.preselected {
                    pre.insert(r.clone());
                }
                ok.insert(r);
            }
        }
        let want = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(ok, want(&fx.expect.candidates), "{id}: candidatos");
        assert_eq!(blocked, want(&fx.expect.blocked), "{id}: bloqueados");
        assert_eq!(pre, want(&fx.expect.preselected), "{id}: preseleccionados");
    }
}

fn lint(rules: &str) -> Result<Catalog, CatalogError> {
    Catalog::from_sources(CATALOG_TOML, &[("t.toml", rules)])
}

const BASE: &str = r#"
id = "t"
version = 1
cat = "CAT-01"
risk = "safe"
regenerable = true
preselect = true
action = "trash"
include = ["*"]
title = { es = "t", en = "t" }
explain = { es = "t", en = "t" }
evidence = "test"
"#;

fn rule_with(extra: &str) -> String {
    format!("[[rule]]\n{BASE}\n{extra}\n")
}

#[test]
fn lints_reject_dangerous_rules() {
    assert!(lint(&rule_with(r#"roots = ["~/Library/Caches"]"#)).is_ok());
    for (bad, why) in [
        (r#"roots = ["~/Library"]"#, "demasiado amplia"),
        (r#"roots = ["~"]"#, "demasiado amplia"),
        (r#"roots = ["~/Documents"]"#, "fuera de prefijos"),
        (r#"roots = ["/Library/Caches"]"#, "no empieza por ~/"),
        (r#"roots = ["~/Library/Caches/../Keychains"]"#, "con .."),
        (r#"roots = ["~/Library/Mobile Documents"]"#, "protegida"),
    ] {
        assert!(lint(&rule_with(bad)).is_err(), "{why}: {bad}");
    }
    let review =
        rule_with("roots = [\"~/Library/Caches\"]").replace("risk = \"safe\"", "risk = \"review\"");
    assert!(lint(&review).is_err(), "preselección de algo no seguro");
    let delete = rule_with("roots = [\"~/Library/Caches\"]")
        .replace("action = \"trash\"", "action = \"delete\"");
    assert!(lint(&delete).is_err(), "las reglas no borran para siempre");
    let unknown = rule_with("roots = [\"~/Library/Caches\"]\nshell = \"rm -rf\"");
    assert!(lint(&unknown).is_err(), "campos desconocidos");
}

#[test]
fn symlinked_root_is_not_walked() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(tmp.path()).expect("canon");
    let elsewhere = home.join("Elsewhere");
    std::fs::create_dir_all(elsewhere.join("victim")).expect("mkdir");
    std::fs::create_dir_all(home.join("Library")).expect("mkdir");
    std::os::unix::fs::symlink(&elsewhere, home.join("Library/Caches")).expect("symlink");
    let catalog = Catalog::embedded()
        .expect("catálogo")
        .filtered(|r| r.id == "user-caches");
    let apps = AppProbe::snapshot(&BTreeSet::<String>::new());
    let res = catalog.scan(&ScanOptions {
        home: &home,
        apps: &apps,
        now: SystemTime::now(),
    });
    assert!(res.candidates.is_empty());
    assert!(matches!(res.roots[0].state, RootState::Unavailable { .. }));
}

#[test]
fn candidates_are_grouped_by_rule() {
    // Ningún candidato de una regla está dentro de otro de otra regla sobre
    // el fixture combinado de todas.
    let catalog = Catalog::embedded().expect("catálogo");
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = std::fs::canonicalize(tmp.path()).expect("canon");
    let now = SystemTime::now();
    for (_, text) in FIXTURES {
        let fx = Fixture::parse(text).expect("fixture");
        fx.materialize(&home, now).expect("materializar");
    }
    let apps = AppProbe::snapshot(&BTreeSet::<String>::new());
    let res = catalog.scan(&ScanOptions {
        home: &home,
        apps: &apps,
        now,
    });
    let mut by_path: BTreeMap<PathBuf, &str> = BTreeMap::new();
    for c in &res.candidates {
        assert!(
            by_path.insert(c.path.clone(), &c.rule_id).is_none(),
            "duplicado: {:?}",
            c.path
        );
    }
    for a in by_path.keys() {
        for b in by_path.keys() {
            if a != b {
                assert!(!b.starts_with(a), "{b:?} está dentro de {a:?}");
            }
        }
    }
}

/// El motivo «app» conserva su `kind` junto al detalle de la app (antes, la
/// clave `kind` del detalle pisaba a la del motivo) y el JSON se vuelve a
/// leer igual.
#[test]
fn app_block_serializes_with_its_own_kind() {
    let b = Block::App {
        detail: ra_safety::apps::AppBlock::Running {
            bundle_id: "com.google.Chrome".into(),
        },
    };
    let text = serde_json::to_string(&b).expect("serializar");
    assert_eq!(
        text,
        r#"{"kind":"app","detail":{"kind":"running","bundle_id":"com.google.Chrome"}}"#
    );
    let back: Block = serde_json::from_str(&text).expect("leer");
    assert_eq!(back, b);
}
