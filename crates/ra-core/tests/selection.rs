//! Planes por selección explícita (T-10) y RF-SEG-09 sobre una carpeta
//! personal de fixture con Documentos, Escritorio y Descargas.
//!
//! Igual que `properties.rs`, cada caso vive en `raíz/{home,trash,state}`
//! dentro de un directorio temporal: nunca se toca el `~` real ni su
//! Papelera.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use proptest::prelude::*;
use ra_catalog::fixture::{Entry, Link, materialize};
use ra_core::{
    Action, Core, CoreError, Env, ErrorClass, Origin, Outcome, Preview, Selection, TrashBackend,
};
use ra_history::{Event, HistoryPaths, read_journal};

struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    trash: PathBuf,
    state: PathBuf,
    core: Core,
}

impl World {
    fn new(entries: &[Entry], links: &[Link]) -> World {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canon");
        let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
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
            home,
            trash,
            state,
            core,
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }
}

fn file(path: &str, age_days: u32) -> Entry {
    Entry {
        path: path.into(),
        size: 1000,
        age_days,
        dir: false,
    }
}

/// Carpetas de documentos de RF-SEG-09 (las que existen en una carpeta
/// personal local; iCloud Drive y File Provider ya están en la lista
/// protegida).
const USER_FOLDERS: &[&str] = &[
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
];

/// Nombres que cualquier regla del catálogo querría si estuvieran en su raíz.
const TEMPTING: &[&str] = &[
    "Library/Caches/com.example.alpha/Cache.db",
    "Library/Logs/old.log",
    "Library/Saved Application State/com.example.alpha.savedState/windows.plist",
    "Library/Application Support/Google/Chrome/Default/Cache/f_0001",
    "Caches/data.bin",
    "app.log",
    "informe.pdf",
    "foto.jpg",
    "clip.mov",
    "tema.mp3",
];

/// `(dev, ino)` de `p` y de todo lo que cuelga de él, sin seguir enlaces
/// simbólicos (un enlace cuenta por sí mismo, no por su destino).
fn identities_under(p: &Path) -> BTreeSet<(u64, u64)> {
    let mut out = BTreeSet::new();
    let mut stack = vec![p.to_path_buf()];
    while let Some(q) = stack.pop() {
        let Ok(m) = fs::symlink_metadata(&q) else {
            continue;
        };
        out.insert((m.dev(), m.ino()));
        if m.is_dir()
            && let Ok(rd) = fs::read_dir(&q)
        {
            stack.extend(rd.flatten().map(|e| e.path()));
        }
    }
    out
}

/// Identidades de las seis carpetas de documentos y de todo su contenido.
fn user_documents(home: &Path) -> BTreeSet<(u64, u64)> {
    USER_FOLDERS
        .iter()
        .flat_map(|f| identities_under(&home.join(f)))
        .collect()
}

/// Elementos del plan que son, o contienen, un objeto de las carpetas de
/// documentos. Por identidad y no por ruta: así también se detecta llegar
/// a un documento por un enlace simbólico en la ruta o por un enlace duro.
fn touching_documents(plan: &ra_core::Plan, docs: &BTreeSet<(u64, u64)>) -> Vec<PathBuf> {
    plan.items()
        .iter()
        .filter(|i| !identities_under(&i.path).is_disjoint(docs))
        .map(|i| i.path.clone())
        .collect()
}

fn all_categories(core: &Core) -> Selection {
    let cats: BTreeSet<String> = core.catalog().rules.iter().map(|r| r.cat.clone()).collect();
    Selection {
        categories: Some(cats.into_iter().collect()),
        include_review: true,
    }
}

prop_compose! {
    /// Documentos, Escritorio y Descargas (y el resto de carpetas de
    /// usuario) llenos de nombres tentadores, más cachés y logs de verdad
    /// para que los planes no salgan vacíos.
    fn home_tree()(
        docs in prop::collection::vec(
            (prop::sample::select(USER_FOLDERS), prop::sample::select(TEMPTING), 0u32..60),
            1..16),
        real in prop::collection::vec(
            (prop::sample::select(&[
                "Library/Logs/a.log",
                "Library/Logs/AppDir/x.log",
                "Library/Caches/com.example.beta/data.bin",
                "Library/Saved Application State/com.example.beta.savedState/windows.plist",
            ][..]), 8u32..60),
            0..4),
        link_docs in any::<bool>(),
    ) -> (Vec<Entry>, Vec<Link>) {
        let mut e: Vec<Entry> = docs
            .into_iter()
            .map(|(folder, name, age)| file(&format!("{folder}/{name}"), age))
            .collect();
        e.extend(real.into_iter().map(|(p, age)| file(p, age)));
        let links = if link_docs {
            vec![Link { path: "Library/Caches/com.example.link".into(), target: "Documents".into() }]
        } else {
            vec![]
        };
        (e, links)
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, .. ProptestConfig::default() })]

    /// RF-SEG-09: preseleccionados ∩ documentos = ∅. Ni lo preseleccionado
    /// ni siquiera todas las categorías con `--include-review` meten nada
    /// de las carpetas de documentos en el plan.
    #[test]
    fn preselected_never_intersects_user_documents((entries, links) in home_tree()) {
        let w = World::new(&entries, &links);
        let docs = user_documents(&w.home);
        prop_assert!(!docs.is_empty());
        let default = w.core.plan(&Selection::default(), Action::Trash).expect("plan");
        for i in default.plan.items() {
            prop_assert!(i.preselected, "{:?}", i.path);
        }
        let hit = touching_documents(&default.plan, &docs);
        prop_assert!(hit.is_empty(), "preseleccionado: {:?}", hit);
        let all = w.core.plan(&all_categories(&w.core), Action::Trash).expect("plan");
        for i in all.plan.items() {
            prop_assert_eq!(i.origin, Origin::Rules);
        }
        let hit = touching_documents(&all.plan, &docs);
        prop_assert!(hit.is_empty(), "en el plan: {:?}", hit);
    }
}

#[test]
fn explicit_selection_can_reach_user_folders_but_is_never_preselected() {
    let w = World::new(
        &[
            file("Downloads/setup.dmg", 30),
            file("Desktop/carpeta/nota.txt", 30),
            file("Documents/tesis.docx", 30),
        ],
        &[],
    );
    let preview = w
        .core
        .plan_selection(&[w.p("Downloads/setup.dmg"), w.p("Desktop/carpeta")])
        .expect("plan");
    assert_eq!(preview.plan.origin(), Origin::ExplicitSelection);
    assert_eq!(preview.plan.items().len(), 2);
    for i in preview.plan.items() {
        assert_eq!(i.origin, Origin::ExplicitSelection);
        assert!(!i.preselected);
        assert!(!i.regenerable);
    }
    // La simulación usa la misma validación que la ejecución.
    assert!(
        preview
            .dry_run
            .items
            .iter()
            .all(|i| matches!(i.outcome, Outcome::Would { .. }))
    );
}

/// Aborto por seguridad (código 5); devuelve el mensaje (en inglés).
fn safety(r: Result<Preview, CoreError>) -> String {
    match r {
        Err(e) if e.class() == ErrorClass::Safety => e.to_string(),
        other => panic!("se esperaba un aborto por seguridad: {other:?}"),
    }
}

#[test]
fn protected_paths_and_their_ancestors_abort_everything() {
    let w = World::new(
        &[
            file("Downloads/setup.dmg", 30),
            file("Library/Keychains/login.keychain-db", 30),
            file("Library/Caches/x/data.bin", 30),
        ],
        &[],
    );
    let ok = w.p("Downloads/setup.dmg");
    for bad in [
        w.p("Library/Keychains/login.keychain-db"),
        w.p("Library/Keychains"),
        w.p("Library"),
        w.home.clone(),
        w.state.clone(),
        PathBuf::from("/"),
        PathBuf::from("/System/Library"),
    ] {
        let m = safety(w.core.plan_selection(&[ok.clone(), bad.clone()]));
        assert!(m.contains("Nothing is moved"), "{bad:?}: {m}");
    }
    // Un enlace en la carpeta padre se resuelve: no sirve para colarse.
    std::os::unix::fs::symlink(w.p("Library/Keychains"), w.p("Downloads/kc")).expect("enlace");
    safety(
        w.core
            .plan_selection(&[w.p("Downloads/kc/login.keychain-db")]),
    );
    // Lo que no se puede resolver ni clasificar también aborta.
    safety(w.core.plan_selection(&[w.p("Downloads/no-existe.dmg")]));
    safety(w.core.plan_selection(&[w.p("No/Existe/nada")]));
    // Una ruta relativa es un error de uso.
    assert!(matches!(
        w.core
            .plan_selection(&[PathBuf::from("Downloads/setup.dmg")]),
        Err(CoreError::RelativePath(_))
    ));
    // Nada se ha movido.
    assert!(ok.exists());
    assert!(fs::read_dir(&w.trash).expect("trash").next().is_none());
}

/// RF-SEG-19: solo rutas dentro de la carpeta personal, ya resueltas.
#[test]
fn paths_outside_home_abort_everything() {
    let w = World::new(&[file("Downloads/setup.dmg", 30)], &[]);
    let outside = w.home.parent().expect("raíz").join("fuera");
    fs::create_dir_all(&outside).expect("mkdir");
    fs::write(outside.join("x.txt"), "x").expect("write");
    let ok = w.p("Downloads/setup.dmg");
    for bad in [outside.join("x.txt"), outside.clone(), w.trash.clone()] {
        let m = safety(w.core.plan_selection(&[ok.clone(), bad.clone()]));
        assert!(m.contains("outside the home folder"), "{bad:?}: {m}");
    }
    // Un enlace en la carpeta padre que sale de `~` tampoco sirve.
    std::os::unix::fs::symlink(&outside, w.p("Downloads/fuera")).expect("enlace");
    let m = safety(w.core.plan_selection(&[w.p("Downloads/fuera/x.txt")]));
    assert!(m.contains("outside the home folder"), "{m}");
    assert!(ok.exists());
    assert!(outside.join("x.txt").exists());
    assert!(fs::read_dir(&w.trash).expect("trash").next().is_none());
}

/// Las seis carpetas de documentos en sí abortan con selección explícita,
/// también por un enlace que apunte a ellas; su contenido sí entra.
#[test]
fn user_folder_roots_abort_but_their_contents_do_not() {
    let entries: Vec<Entry> = USER_FOLDERS
        .iter()
        .map(|f| file(&format!("{f}/dentro.txt"), 30))
        .collect();
    let w = World::new(&entries, &[]);
    for f in USER_FOLDERS {
        let root = w.p(f);
        let m = safety(w.core.plan_selection(std::slice::from_ref(&root)));
        // `~/Music` ya era ancestro de `~/Music/Music`, que es protegida.
        if *f != "Music" {
            assert!(m.contains("its contents can be moved"), "{f}: {m}");
        }
        // `~/Downloads/.` es la misma carpeta.
        safety(w.core.plan_selection(&[root.join(".")]));
        let inside = w
            .core
            .plan_selection(&[root.join("dentro.txt")])
            .expect("el contenido sí se puede elegir");
        assert_eq!(inside.plan.items().len(), 1, "{f}");
    }
    // Sin distinguir mayúsculas.
    safety(w.core.plan_selection(&[w.p("downloads")]));
    // Un enlace a una de ellas cuenta como la carpeta.
    std::os::unix::fs::symlink(w.p("Downloads"), w.p("Desktop/dl")).expect("enlace");
    let m = safety(w.core.plan_selection(&[w.p("Desktop/dl")]));
    assert!(m.contains("it is a link"), "{m}");
    for f in USER_FOLDERS {
        assert!(w.p(f).join("dentro.txt").exists());
    }
    assert!(fs::read_dir(&w.trash).expect("trash").next().is_none());
}

/// I-1: con otra caja en el último componente, el plan usa el nombre real
/// de la entrada (por identidad, sin seguir enlaces).
#[test]
fn the_last_component_keeps_its_real_case() {
    let w = World::new(
        &[
            file("Desktop/carpeta/nota.txt", 30),
            file("Downloads/setup.dmg", 30),
        ],
        &[],
    );
    if !w.p("Desktop/CARPETA").exists() {
        eprintln!("volumen con distinción de mayúsculas: no aplica");
        return;
    }
    // Un enlace con el nombre en mayúsculas no se sigue: se mueve el enlace.
    std::os::unix::fs::symlink(w.p("Downloads/setup.dmg"), w.p("Desktop/Enlace")).expect("enlace");
    let preview = w
        .core
        .plan_selection(&[w.p("Desktop/CARPETA"), w.p("Desktop/ENLACE")])
        .expect("plan");
    let mut paths: Vec<_> = preview
        .plan
        .items()
        .iter()
        .map(|i| i.path.clone())
        .collect();
    paths.sort();
    assert_eq!(paths, vec![w.p("Desktop/Enlace"), w.p("Desktop/carpeta")]);
    let link = preview
        .plan
        .items()
        .iter()
        .find(|i| i.path == w.p("Desktop/Enlace"))
        .expect("enlace");
    assert_eq!(link.kind, ra_core::FileKind::Symlink);
}

#[test]
fn excluded_paths_stay_out_of_the_plan() {
    let w = World::new(
        &[
            file("Downloads/setup.dmg", 30),
            file("Downloads/guardar/a.zip", 30),
        ],
        &[],
    );
    w.core
        .edit_exclusion("~/Downloads/guardar", true, false)
        .expect("exclusión");
    let preview = w
        .core
        .plan_selection(&[w.p("Downloads/setup.dmg"), w.p("Downloads/guardar/a.zip")])
        .expect("plan");
    let items: Vec<_> = preview.plan.items().iter().map(|i| &i.path).collect();
    assert_eq!(items, vec![&w.p("Downloads/setup.dmg")]);
    assert_eq!(preview.plan.skipped().len(), 1);
    assert_eq!(
        preview.plan.skipped()[0].path,
        w.p("Downloads/guardar/a.zip")
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Meta {
    ino: u64,
    dir: bool,
    size: u64,
    mtime: (i64, i64),
}

fn snapshot(base: &Path) -> BTreeMap<PathBuf, Meta> {
    let mut out = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let m = fs::symlink_metadata(&p).expect("lstat");
            out.insert(
                p.strip_prefix(base).expect("rel").to_path_buf(),
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

#[test]
fn execute_then_undo_restores_the_tree_and_journals_the_origin() {
    let w = World::new(
        &[
            file("Downloads/setup.dmg", 30),
            file("Desktop/carpeta/nota.txt", 2),
            file("Desktop/carpeta/sub/foto.jpg", 2),
            file("Documents/tesis.docx", 30),
        ],
        &[],
    );
    let before = snapshot(&w.home);
    let targets = [w.p("Downloads/setup.dmg"), w.p("Desktop/carpeta")];
    let preview = w.core.plan_selection(&targets).expect("plan");
    let hash = preview.plan.plan_hash().to_owned();
    let report = w
        .core
        .execute(preview.plan, &hash, "test")
        .expect("ejecución");
    assert_eq!((report.done, report.skipped, report.failed), (2, 0, 0));
    for t in &targets {
        assert!(!t.exists(), "{t:?} debería estar en la Papelera");
    }
    assert!(w.p("Documents/tesis.docx").exists());

    // El journal anota el origen de cada elemento.
    let (records, _) = read_journal(&HistoryPaths::new(&w.state)).expect("journal");
    let origins: Vec<_> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::ItemIntent { origin, .. } => Some(*origin),
            _ => None,
        })
        .collect();
    assert_eq!(origins, vec![Some(Origin::ExplicitSelection); 2]);

    let undo = w
        .core
        .prepare_undo(report.op_id.as_deref(), None)
        .expect("deshacer");
    let restored = w.core.undo(&undo.plan, "test").expect("restaurar");
    assert_eq!(restored.done, 2);
    let after = snapshot(&w.home);
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>()
    );
    for (rel, m0) in &before {
        let m1 = &after[rel];
        assert_eq!(m0.ino, m1.ino, "{rel:?}");
        // Mover cambia el mtime de las carpetas padre; los ficheros, intactos.
        if !m0.dir {
            assert_eq!(m0, m1, "{rel:?}");
        }
    }
    assert!(snapshot(&w.trash).is_empty());
}
