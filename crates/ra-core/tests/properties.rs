//! Pruebas de propiedades del motor de planes y del ejecutor (05 §10.3)
//! sobre árboles fixture reales en un directorio temporal.
//!
//! Cada caso crea `raíz/{home,trash,state}`: la carpeta personal de prueba,
//! la Papelera de prueba (`TrashBackend::Dir`) y el estado de la app. Nada
//! sale de la raíz temporal; la Papelera real del usuario no se toca.
//!
//! Invariantes:
//! 1. Nunca toca nada fuera del plan.
//! 2. Todo pasa por la Papelera salvo `--permanent`.
//! 3. Con las exclusiones corruptas, aborta sin tocar nada (también si se
//!    corrompen entre la revisión y la ejecución). Lo mismo si en la ruta
//!    del enlace a la configuración hay otra cosa (T-13); y `config set` no
//!    reescribe una configuración corrupta.
//! 4. Omite los elementos que cambian entre el plan y la ejecución.
//! 5. `undo(execute(P))` devuelve el árbol a su estado.
//! 6. La simulación no cambia nada.
//! 7. Omite los elementos con ficheros abiertos por otro proceso (RF-SEG-08).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use proptest::prelude::*;
use ra_catalog::fixture::{Entry, Link, materialize};
use ra_core::{
    Action, Core, Env, ExecReport, FileKind, Outcome, Selection, SkipCause, TrashBackend,
};

// ---------------------------------------------------------------------------
// Mundo de prueba

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    trash: PathBuf,
    state: PathBuf,
    core: Core,
}

impl World {
    fn new(entries: &[Entry], links: &[Link]) -> World {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canon");
        let home = root.join("home");
        let trash = root.join("trash");
        let state = root.join("state");
        for d in [&home, &trash, &state] {
            fs::create_dir_all(d).expect("mkdir");
        }
        materialize(&home, entries, links, SystemTime::now()).expect("materializar");
        let env = Env::new(
            home.clone(),
            state.clone(),
            TrashBackend::Dir(trash.clone()),
        );
        let core = Core::with_apps(env, Box::new(BTreeSet::<String>::new())).expect("core");
        World {
            _tmp: tmp,
            root,
            home,
            trash,
            state,
            core,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Meta {
    ino: u64,
    dir: bool,
    size: u64,
    mtime: (i64, i64),
}

/// Todo lo que hay bajo `base` (sin seguir enlaces), por ruta relativa.
fn snapshot(base: &Path) -> BTreeMap<PathBuf, Meta> {
    let mut out = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let m = fs::symlink_metadata(&p).expect("lstat");
            let rel = p.strip_prefix(base).expect("rel").to_path_buf();
            out.insert(
                rel,
                Meta {
                    ino: m.ino(),
                    dir: m.is_dir(),
                    size: m.size(),
                    mtime: (m.mtime(), m.mtime_nsec()),
                },
            );
            if m.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

/// Proceso hijo (`sleep`) con un fichero abierto como entrada estándar. Se
/// mata al salir de ámbito, también si la prueba falla.
struct HeldOpen(Child);

impl HeldOpen {
    fn spawn(file: &Path) -> HeldOpen {
        let f = fs::File::open(file).expect("abrir");
        let child = Command::new("/bin/sleep")
            .arg("60")
            .stdin(Stdio::from(f))
            .spawn()
            .expect("lanzar sleep");
        HeldOpen(child)
    }
}

impl Drop for HeldOpen {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn inodes(s: &BTreeMap<PathBuf, Meta>) -> BTreeSet<u64> {
    s.values().map(|m| m.ino).collect()
}

fn done_paths(r: &ExecReport) -> Vec<(PathBuf, Option<PathBuf>)> {
    r.items
        .iter()
        .filter_map(|i| match &i.outcome {
            Outcome::Done { result_path, .. } => Some((i.path.clone(), result_path.clone())),
            _ => None,
        })
        .collect()
}

fn under(p: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| p.starts_with(r))
}

fn is_ancestor_of_any(p: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| r.starts_with(p) && r != p)
}

// ---------------------------------------------------------------------------
// Generadores

const CACHE_NAMES: &[&str] = &[
    "com.example.alpha",
    "com.example.beta",
    "org.sample.gamma",
    "net.app.delta",
    "SomeTool",
    "com.apple.bird",
    "Homebrew",
];
const CACHE_FILES: &[&str] = &["data.bin", "Cache.db", "sub/deep.bin", "sub/more/x"];
const LOG_FILES: &[&str] = &[
    "a.log",
    "b.log",
    "AppDir/x.log",
    "Other/y.log",
    "DiagnosticReports/r.ips",
];
const SAVED: &[&str] = &["com.example.alpha", "com.example.beta", "org.sample.gamma"];

/// Fuera de cualquier regla: nunca debe moverse.
fn fixed_outside() -> Vec<Entry> {
    [
        "Documents/tesis.docx",
        "Library/Preferences/com.example.alpha.plist",
        "Library/Keychains/login.keychain-db",
        "Library/Mobile Documents/com~apple~CloudDocs/notas.txt",
        "Library/Application Support/com.example.alpha/state.db",
        "Library/Application Support/Google/Chrome/Default/Cookies",
    ]
    .iter()
    .map(|p| Entry {
        path: (*p).into(),
        size: 64,
        age_days: 30,
        dir: false,
    })
    .collect()
}

fn entry(path: String, size: u64, age_days: u32) -> Entry {
    Entry {
        path,
        size,
        age_days,
        dir: false,
    }
}

prop_compose! {
    fn tree()(
        caches in prop::collection::vec(
            (prop::sample::select(CACHE_NAMES), prop::sample::select(CACHE_FILES), 0u64..20_000, prop_oneof![3 => 8u32..60, 1 => 0u32..3]),
            0..10),
        logs in prop::collection::vec(
            (prop::sample::select(LOG_FILES), 0u64..5_000, prop_oneof![3 => 8u32..60, 1 => 0u32..3]),
            0..6),
        saved in prop::collection::vec((prop::sample::select(SAVED), 0u32..30), 0..3),
        chrome in prop::collection::vec((prop::sample::select(&["Default", "Profile 1"][..]), 0u64..9_000), 0..3),
        links in prop::collection::vec(any::<bool>(), 3),
    ) -> (Vec<Entry>, Vec<Link>) {
        let mut e = fixed_outside();
        for (n, f, s, a) in caches {
            e.push(entry(format!("Library/Caches/{n}/{f}"), s, a));
        }
        for (f, s, a) in logs {
            e.push(entry(format!("Library/Logs/{f}"), s, a));
        }
        for (b, a) in saved {
            e.push(entry(format!("Library/Saved Application State/{b}.savedState/windows.plist"), 100, a));
        }
        for (p, s) in chrome {
            e.push(entry(format!("Library/Caches/Google/Chrome/{p}/Cache/f_0001"), s, 2));
            e.push(entry(format!("Library/Application Support/Google/Chrome/{p}/GPUCache/data_0"), s, 2));
        }
        let mut l = Vec::new();
        // Enlaces maliciosos o raros dentro de raíces limpiables.
        if links[0] {
            l.push(Link { path: "Library/Saved Application State/evil.savedState".into(), target: "Documents".into() });
        }
        if links[1] {
            l.push(Link { path: "Library/Caches/com.example.link".into(), target: "Library/Keychains".into() });
        }
        if links[2] {
            l.push(Link { path: "Library/Logs/link.log".into(), target: "Documents/tesis.docx".into() });
        }
        (e, l)
    }
}

fn selection(all: bool) -> Selection {
    Selection {
        categories: all.then(|| {
            ["CAT-01", "CAT-03", "CAT-08", "CAT-10"]
                .map(String::from)
                .to_vec()
        }),
        include_review: false,
    }
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48)
}

/// Comprueba que todo lo que no está dentro de un elemento hecho sigue igual.
fn assert_untouched_outside(
    before: &BTreeMap<PathBuf, Meta>,
    after: &BTreeMap<PathBuf, Meta>,
    home: &Path,
    done: &[PathBuf],
) -> Result<(), TestCaseError> {
    for (rel, m0) in before {
        let abs = home.join(rel);
        if under(&abs, done) {
            prop_assert!(!after.contains_key(rel), "{rel:?} debería haberse movido");
            continue;
        }
        let m1 = after.get(rel);
        prop_assert!(m1.is_some(), "{rel:?} ha desaparecido sin estar en el plan");
        let m1 = m1.expect("existe");
        prop_assert_eq!(m0.ino, m1.ino, "{:?} cambió de inode", rel);
        if m0.dir && is_ancestor_of_any(&abs, done) {
            // Su contenido directo cambió (se movió un hijo): mtime distinto.
            continue;
        }
        prop_assert_eq!(m0, m1, "{:?} cambió", rel);
    }
    for rel in after.keys() {
        prop_assert!(before.contains_key(rel), "apareció {rel:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Propiedades

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    /// 1 y 2: nunca toca nada fuera del plan, y lo que toca va a la Papelera
    /// (o desaparece solo con `--permanent`).
    #[test]
    fn never_touches_anything_outside_the_plan((entries, links) in tree(), all in any::<bool>(), permanent in prop::bool::weighted(0.25)) {
        let w = World::new(&entries, &links);
        let before = snapshot(&w.home);
        let docs_before = fs::read(w.home.join("Documents/tesis.docx")).expect("doc");

        let action = if permanent { Action::Delete } else { Action::Trash };
        let preview = w.core.plan(&selection(all), action).expect("plan");
        let hash = preview.plan.plan_hash().to_owned();
        let planned: Vec<PathBuf> = preview.plan.items().iter().map(|i| i.path.clone()).collect();
        let report = w.core.execute(preview.plan, &hash, "test").expect("ejecución");

        let done = done_paths(&report);
        let done_src: Vec<PathBuf> = done.iter().map(|(p, _)| p.clone()).collect();
        for p in &done_src {
            prop_assert!(planned.contains(p), "{p:?} hecho sin estar en el plan");
        }
        let after = snapshot(&w.home);
        assert_untouched_outside(&before, &after, &w.home, &done_src)?;
        prop_assert_eq!(fs::read(w.home.join("Documents/tesis.docx")).expect("doc"), docs_before);

        let trash = snapshot(&w.trash);
        if permanent {
            prop_assert!(trash.is_empty(), "--permanent no debe usar la Papelera");
            for (_, r) in &done {
                prop_assert!(r.is_none());
            }
        } else {
            // Nada se destruye: los inodes de antes están en home o en la Papelera.
            let mut now = inodes(&after);
            now.extend(inodes(&trash));
            prop_assert_eq!(inodes(&before), now);
            for (_, r) in &done {
                let r = r.as_ref().expect("ruta en la Papelera");
                prop_assert!(r.starts_with(&w.trash), "{r:?} fuera de la Papelera");
            }
        }
        // Y nada fuera de la raíz temporal.
        prop_assert!(w.state.starts_with(&w.root));
    }

    /// 3: exclusiones corruptas → aborta (código 5) y no toca nada, también
    /// si se corrompen después de revisar el plan.
    #[test]
    fn aborts_when_exclusions_are_corrupt(
        (entries, links) in tree(),
        garbage in prop_oneof![
            Just("[exclusions\npaths = [".to_owned()),
            Just("[exclusions]\npaths = [\"relativa\"]\n".to_owned()),
            Just("[exclusions]\nglobs = [\"~/Library/[\"]\n".to_owned()),
            Just("[exclusiones]\npaths = []\n".to_owned()),
            Just("schema_version = 7\n".to_owned()),
            "[a-z=\\[\\]\"\n ]{1,40}",
        ],
    ) {
        let w = World::new(&entries, &links);
        let config = w.state.join("config.toml");
        let preview = w.core.plan(&selection(true), Action::Trash).expect("plan con config válida");
        let hash = preview.plan.plan_hash().to_owned();

        fs::write(&config, &garbage).expect("corromper");
        // Una cadena aleatoria puede ser una configuración válida (p. ej.
        // vacía): en ese caso no hay nada que comprobar.
        prop_assume!(matches!(w.core.config_state(), ra_core::ConfigState::Corrupt { .. }));

        let before = snapshot(&w.home);
        prop_assert_eq!(w.core.plan(&selection(true), Action::Trash).err().map(|e| e.code()), Some("config_corrupt"));
        prop_assert_eq!(w.core.execute(preview.plan, &hash, "test").err().map(|e| e.code()), Some("config_corrupt"));
        prop_assert_eq!(snapshot(&w.home), before);
        prop_assert!(snapshot(&w.trash).is_empty());
        prop_assert!(!w.state.join("journal").exists(), "no se escribe journal");
        prop_assert_eq!(fs::read_to_string(&config).expect("config"), garbage, "no se reescribe");
    }

    /// 3 (T-13, RF-SEG-05 c): con otra cosa en la ruta del enlace a la
    /// configuración (fichero, carpeta o enlace a otro sitio) no se
    /// planifica ni se ejecuta, aunque aparezca después de la revisión. Lo
    /// que hay en esa ruta tampoco se toca.
    #[test]
    fn aborts_when_something_else_is_at_the_config_link(
        (entries, links) in tree(),
        foreign in 0..3u8,
    ) {
        let w = World::new(&entries, &links);
        let preview = w.core.plan(&selection(true), Action::Trash).expect("plan sin nada en el enlace");
        let hash = preview.plan.plan_hash().to_owned();

        let link = w.core.env().config_link_path.clone().expect("el enlace aplica");
        fs::create_dir_all(link.parent().expect("carpeta")).expect("mkdir");
        match foreign {
            0 => fs::write(&link, "language = \"en\"\n").expect("write"),
            1 => fs::create_dir(&link).expect("mkdir"),
            _ => std::os::unix::fs::symlink(w.root.join("otra.toml"), &link).expect("ln"),
        }

        let before = snapshot(&w.home);
        prop_assert_eq!(w.core.plan(&selection(true), Action::Trash).err().map(|e| e.code()), Some("config_link_conflict"));
        prop_assert_eq!(w.core.execute(preview.plan, &hash, "test").err().map(|e| e.code()), Some("config_link_conflict"));
        prop_assert_eq!(w.core.config_set("language", "es", false).err().map(|e| e.code()), Some("config_link_conflict"));
        prop_assert_eq!(snapshot(&w.home), before);
        prop_assert!(snapshot(&w.trash).is_empty());
        prop_assert!(!w.state.join("journal").exists(), "no se escribe journal");
        prop_assert!(!w.state.join("config.toml").exists(), "no se escribe la configuración");
    }

    /// 3 (T-13): `config set` nunca reescribe una configuración corrupta ni
    /// crea el enlace, con cualquier clave y valor.
    #[test]
    fn config_set_never_rewrites_a_corrupt_config(
        garbage in prop_oneof![
            Just("[exclusions\npaths = [".to_owned()),
            Just("[exclusions]\npaths = [\"relativa\"]\n".to_owned()),
            Just("language = \"fr\"\n".to_owned()),
            Just("[exclusion]\npaths = []\n".to_owned()),
            Just("schema_version = 7\n".to_owned()),
            "[a-z=\\[\\]\"\n ]{1,40}",
        ],
        key in prop_oneof![Just("language"), Just("exclusions.paths"), Just("nope")],
        value in prop_oneof![Just("es".to_owned()), Just("30".to_owned()), "[a-z0-9]{0,6}"],
        dry_run in any::<bool>(),
    ) {
        let w = World::new(&[], &[]);
        let config = w.state.join("config.toml");
        fs::write(&config, &garbage).expect("corromper");
        prop_assume!(matches!(w.core.config_state(), ra_core::ConfigState::Corrupt { .. }));
        prop_assert_eq!(w.core.config_set(key, &value, dry_run).err().map(|e| e.code()), Some("config_corrupt"));
        prop_assert_eq!(fs::read_to_string(&config).expect("config"), garbage, "no se reescribe");
        let link = w.core.env().config_link_path.clone().expect("el enlace aplica");
        prop_assert!(fs::symlink_metadata(&link).is_err(), "no se crea el enlace");
    }

    /// 4: omite los elementos que cambian entre el plan y la ejecución, y no
    /// toca lo que haya en su lugar.
    #[test]
    fn skips_items_changed_between_plan_and_execution(
        (entries, links) in tree(),
        picks in prop::collection::vec((any::<prop::sample::Index>(), 0u8..6), 1..4),
    ) {
        let w = World::new(&entries, &links);
        let preview = w.core.plan(&selection(true), Action::Trash).expect("plan");
        let items: Vec<(PathBuf, FileKind)> = preview
            .plan
            .items()
            .iter()
            .map(|i| (i.path.clone(), i.kind))
            .collect();
        prop_assume!(!items.is_empty());

        let decoy = w.root.join("decoy");
        fs::create_dir_all(&decoy).expect("mkdir");
        fs::write(decoy.join("precious"), b"no tocar").expect("write");

        let mut mutated: BTreeSet<PathBuf> = BTreeSet::new();
        for (idx, kind) in picks {
            let (path, item_kind) = &items[idx.index(items.len())];
            if mutated.iter().any(|m| path.starts_with(m) || m.starts_with(path)) {
                continue;
            }
            let ok = mutate(path, *item_kind, kind, &decoy);
            if ok {
                mutated.insert(path.clone());
            }
        }
        prop_assume!(!mutated.is_empty());
        let after_mutation = snapshot(&w.home);

        let hash = preview.plan.plan_hash().to_owned();
        let report = w.core.execute(preview.plan, &hash, "test").expect("ejecución");
        for r in &report.items {
            if mutated.contains(&r.path) {
                let skipped = matches!(r.outcome, Outcome::Skipped { .. });
                prop_assert!(skipped, "{:?} cambió y no se omitió: {:?}", r.path, r.outcome);
                // Lo que ocupa ahora su sitio sigue intacto.
                let rel = r.path.strip_prefix(&w.home).expect("rel");
                let now = snapshot(&w.home);
                prop_assert_eq!(now.get(rel), after_mutation.get(rel));
            } else {
                let done = matches!(r.outcome, Outcome::Done { .. });
                prop_assert!(done, "{:?} no cambió y no se movió: {:?}", r.path, r.outcome);
            }
        }
        prop_assert_eq!(fs::read(decoy.join("precious")).expect("decoy"), b"no tocar".to_vec());
        prop_assert_eq!(report.skipped as usize, mutated.len());
    }

    /// 5: deshacer devuelve todo a su sitio (mismos inodes, tamaños y fechas).
    #[test]
    fn undo_restores_the_tree((entries, links) in tree(), all in any::<bool>()) {
        let w = World::new(&entries, &links);
        let before = snapshot(&w.home);
        let preview = w.core.plan(&selection(all), Action::Trash).expect("plan");
        let hash = preview.plan.plan_hash().to_owned();
        let report = w.core.execute(preview.plan, &hash, "test").expect("ejecución");
        prop_assume!(report.done > 0);
        prop_assert_eq!(report.skipped + report.failed, 0);

        let undo = w.core.prepare_undo(report.op_id.as_deref(), None).expect("deshacer");
        prop_assert_eq!(undo.plan.ready.len() as u64, report.done);
        let restored = w.core.undo(&undo.plan, "test").expect("restaurar");
        prop_assert_eq!(restored.done, report.done);

        let after = snapshot(&w.home);
        prop_assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
        for (rel, m0) in &before {
            let m1 = &after[rel];
            prop_assert_eq!(m0.ino, m1.ino, "{:?}", rel);
            if !m0.dir {
                prop_assert_eq!(m0, m1, "{:?}", rel);
            }
        }
        prop_assert!(snapshot(&w.trash).is_empty());
        let (ops, _) = w.core.history().expect("historial");
        let clean = ops.iter().find(|o| Some(&o.op_id) == report.op_id.as_ref()).expect("op");
        prop_assert_eq!(clean.undo_state, ra_core::UndoState::Restored);
    }

    /// 6: la simulación recorre la misma validación y no cambia nada.
    #[test]
    fn dry_run_changes_nothing((entries, links) in tree(), all in any::<bool>()) {
        let w = World::new(&entries, &links);
        let before = snapshot(&w.root);
        let preview = w.core.plan(&selection(all), Action::Trash).expect("plan");
        prop_assert_eq!(snapshot(&w.root), before);
        prop_assert_eq!(preview.dry_run.items.len(), preview.plan.items().len());
        let all_would = preview.dry_run.items.iter().all(|i| matches!(i.outcome, Outcome::Would { .. }));
        prop_assert!(all_would);
    }

    /// 7: un elemento con un fichero abierto por otro proceso se omite con
    /// motivo `in_use` y queda intacto; el resto se procesa (RF-SEG-08).
    #[test]
    fn skips_items_open_by_another_process(
        (entries, links) in tree(),
        pick in any::<prop::sample::Index>(),
        permanent in any::<bool>(),
    ) {
        let w = World::new(&entries, &links);
        let action = if permanent { Action::Delete } else { Action::Trash };
        let preview = w.core.plan(&selection(true), action).expect("plan");
        // Elementos con algún fichero que abrir: el propio fichero o el
        // primero de la carpeta.
        let candidates: Vec<(PathBuf, PathBuf)> = preview
            .plan
            .items()
            .iter()
            .filter_map(|i| match i.kind {
                FileKind::File => Some((i.path.clone(), i.path.clone())),
                FileKind::Dir => first_file(&i.path).map(|f| (i.path.clone(), f)),
                _ => None,
            })
            .collect();
        prop_assume!(!candidates.is_empty());
        let (held_item, held_file) = &candidates[pick.index(candidates.len())];
        let contents = fs::read(held_file).expect("leer");

        let holder = HeldOpen::spawn(held_file);
        let hash = preview.plan.plan_hash().to_owned();
        let report = w.core.execute(preview.plan, &hash, "test").expect("ejecución");
        let pid = holder.0.id();
        drop(holder);

        for r in &report.items {
            if &r.path == held_item {
                let in_use = matches!(&r.outcome, Outcome::Skipped { cause: SkipCause::InUse { .. }, .. });
                prop_assert!(in_use, "{:?} está abierto y no se omitió: {:?}", r.path, r.outcome);
                let v = serde_json::to_value(&r.outcome).expect("json");
                prop_assert_eq!(&v["reason"], "in_use");
                prop_assert_eq!(&v["detail"]["pid"], pid);
            } else {
                let done = matches!(r.outcome, Outcome::Done { .. });
                prop_assert!(done, "{:?} no está abierto y no se procesó: {:?}", r.path, r.outcome);
            }
        }
        prop_assert_eq!(report.skipped, 1);
        prop_assert_eq!(fs::read(held_file).expect("sigue en su sitio"), contents);
    }
}

/// Aplica una mutación adversaria a un elemento planificado. Devuelve si
/// se aplicó.
fn mutate(path: &Path, item: FileKind, kind: u8, decoy: &Path) -> bool {
    let old = SystemTime::now() - Duration::from_secs(86_400 * 400);
    let is_dir = item == FileKind::Dir;
    // Abrir un enlace seguiría a su destino: a un enlace solo se le sustituye.
    let kind = if item == FileKind::Symlink && kind < 2 {
        2
    } else {
        kind
    };
    match kind {
        // Cambiar el mtime del propio elemento.
        0 => fs::File::open(path)
            .and_then(|f| f.set_times(fs::FileTimes::new().set_modified(old)))
            .is_ok(),
        // Cambiar el contenido (en una carpeta, un fichero de dentro).
        1 => {
            let target = if is_dir {
                first_file(path)
            } else {
                Some(path.to_path_buf())
            };
            match target {
                Some(t) => fs::OpenOptions::new()
                    .append(true)
                    .open(t)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, b"+"))
                    .is_ok(),
                None => false,
            }
        }
        // Sustituirlo por otro con el mismo nombre (otro inode).
        2 => {
            let removed = if is_dir {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            removed.is_ok() && fs::write(path, b"otro").is_ok()
        }
        // Sustituirlo por un enlace simbólico hacia fuera.
        3 => {
            let removed = if is_dir {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            removed.is_ok() && std::os::unix::fs::symlink(decoy, path).is_ok()
        }
        // Crear un fichero nuevo dentro (solo carpetas).
        4 if is_dir => fs::write(path.join("nuevo-tras-revisar"), b"n").is_ok(),
        // Borrarlo.
        _ => {
            if is_dir {
                fs::remove_dir_all(path).is_ok()
            } else {
                fs::remove_file(path).is_ok()
            }
        }
    }
}

fn first_file(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).ok()?.flatten() {
            let t = e.file_type().ok()?;
            if t.is_file() {
                return Some(e.path());
            }
            if t.is_dir() {
                stack.push(e.path());
            }
        }
    }
    None
}
