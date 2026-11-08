use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use proptest::prelude::*;
use ra_catalog::{Block, Candidate, Risk, RuleAction, ScanResult};
use ra_safety::{Config, Exclusions, FileKind, Identity, ProtectedList, TreeFingerprint};

use super::*;

const HOME: &str = "/Users/ana";

fn cand(path: &str, cat: &str, preselected: bool, block: Option<Block>) -> Candidate {
    Candidate {
        path: PathBuf::from(path),
        rule_id: format!("rule.{cat}"),
        rule_version: 1,
        cat: cat.into(),
        risk: Risk::Safe,
        regenerable: true,
        preselected,
        action: RuleAction::Trash,
        identity: Identity {
            dev: 1,
            ino: path.len() as u64,
            kind: FileKind::Dir,
            size: 64,
            mtime: 1,
            mtime_nsec: 0,
            ctime: 1,
            ctime_nsec: 0,
        },
        fingerprint: Some(TreeFingerprint {
            entries: 1,
            bytes_allocated: 4096,
            bytes_logical: 100,
            newest_mtime: 1,
            crosses_device: false,
            digest: "blake3:00".into(),
        }),
        apps: vec![],
        block,
    }
}

fn build(cands: Vec<Candidate>, sel: &Selection, excl: &str) -> Plan {
    build_from(cands, sel, excl, Origin::Rules)
}

fn build_from(cands: Vec<Candidate>, sel: &Selection, excl: &str, origin: Origin) -> Plan {
    let home = std::path::Path::new(HOME);
    let protected =
        ProtectedList::embedded(home, &home.join("Library/Application Support/app.test"));
    let config: Config = toml::from_str(excl).expect("toml");
    let exclusions = Exclusions::compile(&config, home).expect("exclusiones");
    let scan = ScanResult {
        catalog_version: "test".into(),
        candidates: cands,
        roots: vec![],
    };
    Plan::build(&PlanInputs {
        scan: &scan,
        home,
        protected: &protected,
        exclusions: &exclusions,
        selection: sel,
        action: Action::Trash,
        origin,
        now: SystemTime::now(),
    })
}

#[test]
fn explicit_selection_takes_everything_and_annotates_origin() {
    let cands = vec![
        cand("/Users/ana/Downloads/setup.dmg", "SEL", false, None),
        cand("/Users/ana/Desktop/notas", "SEL", false, None),
        cand("/Users/ana/Library", "SEL", false, None),
        cand("/Users/ana/Desktop/viejo", "SEL", false, None),
    ];
    let excl = "[exclusions]\npaths = [\"~/Desktop/viejo\"]\n";
    // Con origen «reglas», las carpetas de documentos no entran nunca.
    let rules = build_from(cands.clone(), &Selection::default(), excl, Origin::Rules);
    assert!(rules.items().is_empty());

    let p = build_from(
        cands,
        &Selection::default(),
        excl,
        Origin::ExplicitSelection,
    );
    assert_eq!(p.origin(), Origin::ExplicitSelection);
    let paths: Vec<_> = p.items().iter().map(|i| i.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/Users/ana/Desktop/notas"),
            PathBuf::from("/Users/ana/Downloads/setup.dmg"),
        ]
    );
    assert!(
        p.items()
            .iter()
            .all(|i| i.origin == Origin::ExplicitSelection && !i.preselected)
    );
    // El ancestro de una ruta protegida y la ruta excluida quedan omitidos.
    assert!(
        p.skipped()
            .iter()
            .any(|s| s.path == Path::new("/Users/ana/Library")
                && matches!(s.reason, SkipReason::Protected(_)))
    );
    assert!(
        p.skipped()
            .iter()
            .any(|s| s.path == Path::new("/Users/ana/Desktop/viejo")
                && matches!(s.reason, SkipReason::Excluded(_)))
    );

    // El origen viaja en el JSON y el plan se puede volver a cargar.
    let v = serde_json::to_value(&p).expect("json");
    assert_eq!(v["origin"], "explicit_selection");
    assert_eq!(v["items"][0]["origin"], "explicit_selection");
    let back = Plan::from_json(&serde_json::to_string(&p).expect("json")).expect("carga");
    assert_eq!(back, p);
}

#[test]
fn rule_items_serialize_as_before() {
    let p = build(
        vec![cand("/Users/ana/Library/Logs/b", "CAT-03", true, None)],
        &Selection::default(),
        "",
    );
    let v = serde_json::to_value(&p).expect("json");
    assert_eq!(v["origin"], "rules");
    assert!(v["items"][0].get("origin").is_none());
    let back = Plan::from_json(&serde_json::to_string(&p).expect("json")).expect("carga");
    assert_eq!(back.items()[0].origin, Origin::Rules);
}

#[test]
fn only_preselected_by_default() {
    let p = build(
        vec![
            cand("/Users/ana/Library/Caches/a", "CAT-01", false, None),
            cand("/Users/ana/Library/Logs/b", "CAT-03", true, None),
        ],
        &Selection::default(),
        "",
    );
    assert_eq!(p.items().len(), 1);
    assert_eq!(p.items()[0].category, "CAT-03");
    assert_eq!(p.not_selected()["CAT-01"].items, 1);
}

#[test]
fn category_selection_takes_all_safe() {
    let sel = Selection {
        categories: Some(vec!["cat-01".into()]),
        include_review: false,
    };
    let p = build(
        vec![
            cand("/Users/ana/Library/Caches/a", "CAT-01", false, None),
            cand("/Users/ana/Library/Logs/b", "CAT-03", true, None),
        ],
        &sel,
        "",
    );
    assert_eq!(p.items().len(), 1);
    assert_eq!(p.items()[0].category, "CAT-01");
}

#[test]
fn blocked_protected_excluded_and_overlapping_are_skipped() {
    let p = build(
        vec![
            cand("/Users/ana/Library/Caches/a", "CAT-01", true, None),
            cand("/Users/ana/Library/Caches/a/inner", "CAT-01", true, None),
            cand(
                "/Users/ana/Library/Caches/b",
                "CAT-01",
                true,
                Some(Block::CrossesDevice),
            ),
            cand("/Users/ana/Library/Keychains/x", "CAT-01", true, None),
            cand("/Users/ana/Library/Caches/keep", "CAT-01", true, None),
        ],
        &Selection::default(),
        "[exclusions]\npaths = [\"~/Library/Caches/keep\"]\n",
    );
    let paths: Vec<_> = p.items().iter().map(|i| i.path.clone()).collect();
    assert_eq!(paths, vec![PathBuf::from("/Users/ana/Library/Caches/a")]);
    let reasons: Vec<_> = p.skipped().iter().map(|s| &s.reason).collect();
    assert!(reasons.iter().any(|r| matches!(r, SkipReason::Overlaps(_))));
    assert!(reasons.iter().any(|r| matches!(r, SkipReason::Blocked(_))));
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, SkipReason::Protected(_)))
    );
    assert!(reasons.iter().any(|r| matches!(r, SkipReason::Excluded(_))));
}

#[test]
fn approval_is_bound_to_hash_and_expires() {
    let p = build(
        vec![cand("/Users/ana/Library/Logs/b", "CAT-03", true, None)],
        &Selection::default(),
        "",
    );
    let hash = p.plan_hash().to_owned();
    assert!(p.clone().approve("blake3:otro", SystemTime::now()).is_err());
    let later = SystemTime::now() + PREVIEW_TTL + Duration::from_secs(1);
    assert!(matches!(
        p.clone().approve(&hash, later),
        Err(PlanError::Expired { .. })
    ));
    assert!(p.approve(&hash, SystemTime::now()).is_ok());
}

#[test]
fn json_roundtrip_verifies_and_detects_tampering() {
    let p = build(
        vec![cand("/Users/ana/Library/Logs/b", "CAT-03", true, None)],
        &Selection::default(),
        "",
    );
    let text = serde_json::to_string_pretty(&p).expect("json");
    assert_eq!(Plan::from_json(&text).expect("válido"), p);
    let tampered = text.replace("/Users/ana/Library/Logs/b", "/Users/ana/Documents");
    assert!(matches!(
        Plan::from_json(&tampered),
        Err(PlanError::HashMismatch { .. })
    ));
}

#[test]
fn ids_are_unique_and_sortable() {
    let t = SystemTime::now();
    let a = new_id(t);
    let b = new_id(t);
    assert_ne!(a, b);
    assert!(new_id(t + Duration::from_secs(1)) > a);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// El hash no depende del orden en que llegan los candidatos, y cambiar
    /// cualquier byte de una ruta lo cambia.
    #[test]
    fn hash_is_canonical(names in proptest::collection::btree_set("[a-z]{1,8}", 1..8), flip in 0usize..8) {
        let mk = |rev: bool| {
            let mut v: Vec<_> = names.iter().map(|n| cand(&format!("{HOME}/Library/Caches/{n}"), "CAT-01", true, None)).collect();
            if rev { v.reverse(); }
            v
        };
        let a = build(mk(false), &Selection::default(), "");
        let b = build(mk(true), &Selection::default(), "");
        prop_assert_eq!(a.items(), b.items());
        let mut json: serde_json::Value = serde_json::to_value(&a).expect("json");
        let idx = flip % a.items().len();
        json["items"][idx]["path"] = serde_json::Value::String(format!("{HOME}/Library/Caches/zz-cambiado"));
        let tampered: Plan = serde_json::from_value(json).expect("plan");
        prop_assert!(tampered.verify().is_err());
    }
}
