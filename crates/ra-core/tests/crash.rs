//! Arnés de cierre abrupto (T-07: RNF-09, RNF-ARQ-06, RNF-12).
//!
//! En cada ronda se lanza el binario `rotten clean --yes` sobre una carpeta
//! personal temporal con la Papelera de prueba (`ROTTEN_TRASH_DIR`) y se mata
//! con `SIGKILL` en un punto aleatorio; después se ejecuta `Core::recover` y
//! se comprueban las invariantes. Luego se hace lo mismo con
//! `rotten undo --yes`. Otra prueba hace lo mismo con
//! `rotten clean --permanent --yes`: lo que no está en su sitio está borrado
//! y anotado.
//!
//! Cada ronda elige al azar el tamaño del lote de `fsync`
//! (`ROTTEN_FSYNC_BATCH`, de 1 a `ITEMS + 1`), para que los cortes caigan
//! con varias intenciones pendientes y en los bordes entre lotes (T-08).
//!
//! El punto de corte se elige por número de registros del journal: el
//! proceso se mata cuando el journal tiene `k` registros nuevos, más un
//! retardo aleatorio (`jitter`, hasta `JITTER_MAX_US`). Así los cortes caen
//! dentro de la ejecución, en cualquier punto entre intenciones y
//! resultados, y no antes (el arranque y el escaneo son la mayor parte del
//! tiempo del proceso) ni después.
//!
//! Variables:
//!
//! - `ROTTEN_CRASH_ROUNDS`: número de rondas (50 por defecto; 1 000 en la
//!   nocturna).
//! - `ROTTEN_CRASH_SEED`: semilla base. Sin ella, se toma del reloj y se
//!   imprime. La semilla de una ronda que falla reproduce esa ronda con
//!   `ROTTEN_CRASH_SEED=<semilla> ROTTEN_CRASH_ROUNDS=1` (mismo `k` y mismo
//!   retardo; el reparto del planificador del sistema no es reproducible).
//! - `ROTTEN_CRASH_STRICT`: activo por defecto; la prueba falla si un
//!   elemento queda en la Papelera con paradero «desconocido» en el journal
//!   (ver `recovered_trash_item_keeps_its_location`). Con
//!   `ROTTEN_CRASH_STRICT=0` solo se cuentan y se informan.
//! - `ROTTEN_BIN`: binario `rotten` que se lanza. Por defecto, el de
//!   `target/<perfil>/rotten`, que `cargo test --workspace` compila antes de
//!   ejecutar las pruebas. Con `cargo test -p ra-core` hay que compilarlo
//!   antes (`cargo build -p rotten`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use ra_catalog::fixture::{Entry, materialize};
use ra_core::{Action, Core, Env, Selection, TrashBackend};
use ra_history::{
    Event, HistoryPaths, ItemState, Journal, OpKind, OpMode, OpStatus, StepAction, list_ops,
    manifest_for, prepare_undo, read_journal,
};

const FILES: usize = 10;
const DIRS: usize = 4;
const ITEMS: usize = FILES + DIRS;
/// Retardo máximo tras el registro `k`. Cubre algo más de un ciclo
/// intención → resultado → intención (unos 8 ms con `F_FULLFSYNC` en un Mac
/// con SSD), para que el corte caiga en cualquier punto del ciclo y no solo
/// durante el `fsync` del registro `k`.
const JITTER_MAX_US: u64 = 10_000;

/// Generador `splitmix64`: suficiente para elegir puntos de corte.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Entero en `0..=max`.
    fn upto(&mut self, max: u64) -> u64 {
        self.next() % (max + 1)
    }
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().map(|v| {
        v.trim()
            .parse()
            .unwrap_or_else(|e| panic!("{name}={v:?} no es un número: {e}"))
    })
}

fn rotten_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("ROTTEN_BIN") {
        return p.into();
    }
    // target/<perfil>/deps/crash-<hash> → target/<perfil>/rotten
    let exe = std::env::current_exe().expect("current_exe");
    let bin = exe
        .parent()
        .and_then(Path::parent)
        .expect("target/<perfil>")
        .join("rotten");
    assert!(
        bin.is_file(),
        "no está el binario {}: ejecuta `cargo build -p rotten` (o \
         `cargo test --workspace`, que lo compila) o define ROTTEN_BIN",
        bin.display()
    );
    bin
}

struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    trash: PathBuf,
    state: PathBuf,
    core: Core,
    /// Elementos del plan: ruta original e inode.
    items: Vec<(PathBuf, u64)>,
    /// Lo que nunca debe moverse: ruta e inode.
    decoys: Vec<(PathBuf, u64)>,
}

fn entry(path: String, age_days: u32) -> Entry {
    Entry {
        path,
        size: 200,
        age_days,
        dir: false,
    }
}

fn world() -> World {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");
    let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
    for d in [&home, &trash, &state] {
        fs::create_dir_all(d).expect("mkdir");
    }
    let mut entries: Vec<Entry> = (0..FILES)
        .map(|i| entry(format!("Library/Logs/crash-{i:02}.log"), 30))
        .collect();
    for i in 0..DIRS {
        for f in ["a.log", "b.log"] {
            entries.push(entry(format!("Library/Logs/app-{i}/{f}"), 30));
        }
    }
    let decoy_paths = [
        "Library/Logs/current.log", // demasiado reciente
        "Documents/tesis.docx",
        "Library/Preferences/com.example.editor.plist",
    ];
    entries.push(entry(decoy_paths[0].into(), 0));
    entries.push(entry(decoy_paths[1].into(), 30));
    entries.push(entry(decoy_paths[2].into(), 30));
    materialize(&home, &entries, &[], SystemTime::now()).expect("materializar");

    let env = Env::new(
        home.clone(),
        state.clone(),
        TrashBackend::Dir(trash.clone()),
    );
    let core = Core::with_apps(env, Box::new(BTreeSet::<String>::new())).expect("core");
    // El mismo plan que calculará el binario.
    let preview = core
        .plan(&Selection::default(), Action::Trash)
        .expect("plan");
    let items: Vec<(PathBuf, u64)> = preview
        .plan
        .items()
        .iter()
        .map(|i| (i.path.clone(), ino(&i.path).expect("elemento del plan")))
        .collect();
    assert_eq!(items.len(), ITEMS, "fixture inesperado: {items:?}");
    let decoys = decoy_paths
        .iter()
        .map(|p| {
            let p = home.join(p);
            let i = ino(&p).expect("señuelo");
            (p, i)
        })
        .collect();
    World {
        _tmp: tmp,
        home,
        trash,
        state,
        core,
        items,
        decoys,
    }
}

fn ino(p: &Path) -> Option<u64> {
    fs::symlink_metadata(p).ok().map(|m| m.ino())
}

fn journal_files(state: &Path) -> Vec<PathBuf> {
    let dir = HistoryPaths::new(state).journal_dir;
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|x| x == "jsonl"));
    files.sort();
    files
}

/// Registros escritos hasta ahora (líneas del journal).
fn journal_lines(state: &Path) -> usize {
    journal_files(state)
        .iter()
        .map(|f| {
            fs::read(f)
                .map(|b| b.iter().filter(|&&c| c == b'\n').count())
                .unwrap_or(0)
        })
        .sum()
}

#[derive(Debug)]
enum Ending {
    /// Muerto con `SIGKILL` tras ver `seen` registros nuevos.
    Killed { seen: usize },
    /// Terminó antes del corte, con este código.
    Exited(Option<i32>),
}

impl Ending {
    fn describe(&self) -> String {
        match self {
            Ending::Killed { seen } => format!("muerto tras {seen} registros"),
            Ending::Exited(code) => format!("terminó con {code:?}"),
        }
    }
}

/// Lanza `rotten <args>` y lo mata cuando el journal tiene `k` registros
/// nuevos y han pasado `jitter` más.
fn run_and_kill(
    w: &World,
    bin: &Path,
    args: &[&str],
    batch: usize,
    k: usize,
    jitter: Duration,
) -> Ending {
    let base = journal_lines(&w.state);
    let mut child = Command::new(bin)
        .args(args)
        .env("ROTTEN_FSYNC_BATCH", batch.to_string())
        .env("ROTTEN_HOME", &w.home)
        .env("ROTTEN_TRASH_DIR", &w.trash)
        .env("ROTTEN_STATE_DIR", &w.state)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("lanzar rotten");
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().expect("try_wait") {
            return Ending::Exited(st.code());
        }
        let seen = journal_lines(&w.state).saturating_sub(base);
        if seen >= k {
            std::thread::sleep(jitter);
            child.kill().expect("SIGKILL");
            let st = child.wait().expect("wait");
            return match st.signal() {
                Some(9) => Ending::Killed { seen },
                _ => Ending::Exited(st.code()),
            };
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("rotten {args:?} no termina");
        }
        std::thread::sleep(Duration::from_micros(200));
    }
}

/// Operaciones empezadas y sin cerrar.
fn open_ops(state: &Path) -> Vec<String> {
    let (records, _) = read_journal(&HistoryPaths::new(state)).expect("journal");
    list_ops(&records)
        .into_iter()
        .filter(|o| o.status.is_none())
        .map(|o| o.op_id)
        .collect()
}

/// Ruta → (inode, tamaño, mtime, mtime_nsec).
type Tree = BTreeMap<PathBuf, (u64, u64, i64, i64)>;

/// Todo lo que hay bajo `base`, sin seguir enlaces.
fn tree(base: &Path) -> Tree {
    let mut out = BTreeMap::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let m = fs::symlink_metadata(&p).expect("lstat");
            out.insert(p.clone(), (m.ino(), m.size(), m.mtime(), m.mtime_nsec()));
            if m.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

/// Lo que `recover` podría cambiar: ficheros, journal y manifiestos.
fn fingerprint(w: &World) -> (Tree, Vec<(PathBuf, Vec<u8>)>) {
    let mut fs_tree = tree(&w.home);
    fs_tree.extend(tree(&w.trash));
    let paths = HistoryPaths::new(&w.state);
    let mut state_files = Vec::new();
    for dir in [&paths.journal_dir, &paths.manifests_dir] {
        for p in tree(dir).into_keys() {
            if p.is_file() {
                let bytes = fs::read(&p).expect("leer estado");
                state_files.push((p, bytes));
            }
        }
    }
    (fs_tree, state_files)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    Original,
    Trash(PathBuf),
}

/// Comprueba las invariantes tras `recover` y devuelve cuántos elementos
/// están en la Papelera con paradero «desconocido» en el journal.
fn check(w: &World, was_open: &[String], ctx: &str) -> usize {
    let paths = HistoryPaths::new(&w.state);
    let (records, _) = read_journal(&paths).expect("journal");

    // La operación interrumpida queda cerrada como recuperada; ninguna queda
    // abierta.
    for o in list_ops(&records) {
        assert!(o.status.is_some(), "{ctx}: {} sigue abierta", o.op_id);
        if was_open.contains(&o.op_id) {
            assert_eq!(
                o.status,
                Some(OpStatus::Recovered),
                "{ctx}: {} no quedó como recuperada",
                o.op_id
            );
        }
    }

    // Cada elemento está en su ruta original o en la Papelera de prueba, una
    // sola vez y con el mismo inode.
    let trash: BTreeMap<u64, PathBuf> = fs::read_dir(&w.trash)
        .expect("papelera")
        .flatten()
        .map(|e| (ino(&e.path()).expect("lstat"), e.path()))
        .collect();
    let mut places: BTreeMap<PathBuf, Place> = BTreeMap::new();
    for (orig, i) in &w.items {
        let at_orig = ino(orig) == Some(*i);
        let in_trash = trash.get(i);
        let place = match (at_orig, in_trash) {
            (true, None) => Place::Original,
            (false, Some(t)) => Place::Trash(t.clone()),
            (true, Some(_)) => panic!("{ctx}: {orig:?} está a la vez en su sitio y en la Papelera"),
            (false, None) => panic!("{ctx}: {orig:?} se ha perdido"),
        };
        places.insert(orig.clone(), place);
    }
    let in_trash = places
        .values()
        .filter(|p| matches!(p, Place::Trash(_)))
        .count();
    assert_eq!(
        trash.len(),
        in_trash,
        "{ctx}: hay en la Papelera algo que no es del plan: {trash:?}"
    );
    for (p, i) in &w.decoys {
        assert_eq!(ino(p), Some(*i), "{ctx}: se ha tocado {p:?}");
    }

    // Registrado: lo que no está en su sitio tiene intención en el journal
    // y el manifiesto sabe dónde está.
    let mut unknown = 0;
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for o in list_ops(&records) {
        let m = manifest_for(&records, &o.op_id).expect("manifiesto");
        for mi in &m.items {
            assert_ne!(
                mi.state,
                ItemState::Pending,
                "{ctx}: {} elemento {} sin reconciliar",
                o.op_id,
                mi.item_id
            );
        }
        if m.kind != OpKind::Clean {
            continue;
        }
        for mi in &m.items {
            let place = places
                .get(&mi.original_path)
                .unwrap_or_else(|| panic!("{ctx}: {:?} no es del plan", mi.original_path));
            seen.insert(mi.original_path.clone());
            let ok = match (place, &mi.state) {
                (Place::Trash(t), ItemState::Trashed) => mi.trashed_path.as_ref() == Some(t),
                (Place::Trash(_), ItemState::Unknown { .. }) => {
                    unknown += 1;
                    true
                }
                (Place::Original, ItemState::Restored { to, .. }) => *to == mi.original_path,
                (Place::Original, ItemState::Skipped { .. } | ItemState::Failed { .. }) => true,
                _ => false,
            };
            assert!(
                ok,
                "{ctx}: {:?} está en {place:?} pero el journal dice {:?} ({:?})",
                mi.original_path, mi.state, mi.trashed_path
            );
        }
    }
    for (orig, place) in &places {
        if *place != Place::Original {
            assert!(
                seen.contains(orig),
                "{ctx}: {orig:?} se movió sin intención en el journal"
            );
        }
    }
    unknown
}

/// Invariantes tras cortar un `clean --permanent`: nada en la Papelera, los
/// señuelos intactos y cada elemento del plan en su sitio o borrado. Lo que
/// no está en su sitio tiene intención y está anotado como borrado; lo que
/// sigue en su sitio no está anotado como borrado, y si no tiene intención
/// está entero.
fn check_permanent(w: &World, was_open: &[String], ctx: &str) {
    let paths = HistoryPaths::new(&w.state);
    let (records, _) = read_journal(&paths).expect("journal");
    for o in list_ops(&records) {
        assert!(o.status.is_some(), "{ctx}: {} sigue abierta", o.op_id);
        if was_open.contains(&o.op_id) {
            assert_eq!(o.status, Some(OpStatus::Recovered), "{ctx}: {}", o.op_id);
        }
    }
    let trash: Vec<PathBuf> = fs::read_dir(&w.trash)
        .expect("papelera")
        .flatten()
        .map(|e| e.path())
        .collect();
    assert!(trash.is_empty(), "{ctx}: algo fue a la Papelera: {trash:?}");
    for (p, i) in &w.decoys {
        assert_eq!(ino(p), Some(*i), "{ctx}: se ha tocado {p:?}");
    }
    let mut states: BTreeMap<PathBuf, ItemState> = BTreeMap::new();
    for o in list_ops(&records) {
        let m = manifest_for(&records, &o.op_id).expect("manifiesto");
        assert_eq!(m.kind, OpKind::Clean, "{ctx}");
        for mi in m.items {
            states.insert(mi.original_path, mi.state);
        }
    }
    for (orig, i) in &w.items {
        let at_orig = ino(orig) == Some(*i);
        match (at_orig, states.get(orig)) {
            (false, Some(ItemState::Deleted)) => {}
            (false, other) => {
                panic!("{ctx}: {orig:?} ya no está y el journal dice {other:?}")
            }
            (true, None) => {
                // Sin intención: nadie lo ha tocado, tampoco su contenido.
                if orig.is_dir() {
                    let n = fs::read_dir(orig).expect("carpeta").count();
                    assert_eq!(n, 2, "{ctx}: {orig:?} perdió contenido sin intención");
                }
            }
            (true, Some(ItemState::Skipped { .. } | ItemState::Failed { .. })) => {}
            (true, Some(other)) => {
                panic!("{ctx}: {orig:?} sigue en su sitio y el journal dice {other:?}")
            }
        }
    }
}

/// `recover`, invariantes e idempotencia (repetir `recover` no cambia nada).
fn recover_and_check(w: &World, ctx: &str) -> usize {
    recover_then(w, ctx, |w, was_open| check(w, was_open, ctx))
}

fn recover_then(w: &World, ctx: &str, check: impl Fn(&World, &[String]) -> usize) -> usize {
    let mut was_open = open_ops(&w.state);
    let mut recovered = w.core.recover().expect("recover");
    was_open.sort();
    recovered.sort();
    assert_eq!(recovered, was_open, "{ctx}: operaciones recuperadas");
    let unknown = check(w, &was_open);
    let before = fingerprint(w);
    assert!(
        w.core.recover().expect("recover").is_empty(),
        "{ctx}: el segundo recover recupera algo"
    );
    assert!(
        before == fingerprint(w),
        "{ctx}: el segundo recover cambia algo"
    );
    unknown
}

#[derive(Default)]
struct Stats {
    killed: usize,
    exited: usize,
    unknown: usize,
}

impl Stats {
    fn note(&mut self, e: &Ending) {
        match e {
            Ending::Killed { .. } => self.killed += 1,
            Ending::Exited(_) => self.exited += 1,
        }
    }
}

fn round(bin: &Path, seed: u64, clean: &mut Stats, undo: &mut Stats) {
    let mut rng = Rng(seed);
    let w = world();
    let repro = format!("ROTTEN_CRASH_SEED={seed} ROTTEN_CRASH_ROUNDS=1");

    // Limpieza: op_started, intención y resultado por elemento, op_finished.
    let batch = rng.upto(ITEMS as u64) as usize + 1;
    let k = rng.upto(2 * ITEMS as u64 + 2) as usize;
    let jitter = Duration::from_micros(rng.upto(JITTER_MAX_US));
    let ending = run_and_kill(&w, bin, &["clean", "--yes", "--json"], batch, k, jitter);
    if let Ending::Exited(code) = ending {
        assert_eq!(code, Some(0), "clean terminó mal ({repro})");
    }
    clean.note(&ending);
    let ctx = format!(
        "clean lote={batch} k={k} jitter={jitter:?}, {} ({repro})",
        ending.describe()
    );
    clean.unknown += recover_and_check(&w, &ctx);

    // Deshacer lo que se pueda de esa limpieza.
    let (records, _) = read_journal(&HistoryPaths::new(&w.state)).expect("journal");
    let Some(op) = list_ops(&records)
        .into_iter()
        .find(|o| o.kind == ra_history::HistoryKind::Clean)
    else {
        return;
    };
    let plan = prepare_undo(&records, &op.op_id, None).expect("plan de deshacer");
    let ready = plan.ready.len();
    // Con elementos ya no recuperables, un deshacer completo termina con 4.
    let partial = !plan.not_recoverable.is_empty();
    if ready == 0 {
        return;
    }
    let k = rng.upto(2 * ready as u64 + 2) as usize;
    let jitter = Duration::from_micros(rng.upto(JITTER_MAX_US));
    let ending = run_and_kill(&w, bin, &["undo", "--yes", "--json"], batch, k, jitter);
    if let Ending::Exited(code) = ending {
        assert!(
            code == Some(0) || (partial && code == Some(4)),
            "undo terminó con {code:?} ({repro})"
        );
    }
    undo.note(&ending);
    let ctx = format!(
        "undo lote={batch} k={k} jitter={jitter:?}, {} ({repro})",
        ending.describe()
    );
    undo.unknown += recover_and_check(&w, &ctx);
}

#[test]
fn kill9_at_random_points_leaves_everything_recoverable() {
    let bin = rotten_bin();
    let rounds = env_u64("ROTTEN_CRASH_ROUNDS").unwrap_or(50);
    let base = env_u64("ROTTEN_CRASH_SEED").unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("reloj")
            .as_nanos() as u64
    });
    let strict = !std::env::var("ROTTEN_CRASH_STRICT").is_ok_and(|v| v.trim() == "0");
    eprintln!("arnés kill -9: {rounds} rondas, ROTTEN_CRASH_SEED={base}");
    let (mut clean, mut undo) = (Stats::default(), Stats::default());
    let start = Instant::now();
    let mut unknown_seeds = Vec::new();
    for r in 0..rounds {
        let seed = base.wrapping_add(r.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let before = clean.unknown + undo.unknown;
        round(&bin, seed, &mut clean, &mut undo);
        if clean.unknown + undo.unknown > before {
            unknown_seeds.push(seed);
        }
    }
    let unknown = clean.unknown + undo.unknown;
    eprintln!(
        "arnés kill -9: {rounds} rondas en {:.1?}; clean: {} cortes, {} completas; \
         undo: {} cortes, {} completas; elementos con paradero desconocido: {unknown}",
        start.elapsed(),
        clean.killed,
        clean.exited,
        undo.killed,
        undo.exited,
    );
    if !unknown_seeds.is_empty() {
        eprintln!("arnés kill -9: rondas con paradero desconocido: {unknown_seeds:?}");
    }
    assert!(
        !strict || unknown == 0,
        "{unknown} elementos en la Papelera con paradero desconocido en el journal \
         (semillas {unknown_seeds:?})"
    );
}

/// Seguimiento de 08 §2.2: un `clean --permanent` muerto en un punto
/// aleatorio deja cada elemento en su sitio o borrado y anotado.
#[test]
fn kill9_during_permanent_clean_leaves_everything_deleted_or_recorded() {
    let bin = rotten_bin();
    let rounds = env_u64("ROTTEN_CRASH_ROUNDS").unwrap_or(50);
    let base = env_u64("ROTTEN_CRASH_SEED").unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("reloj")
            .as_nanos() as u64
    });
    eprintln!("arnés kill -9 (--permanent): {rounds} rondas, ROTTEN_CRASH_SEED={base}");
    let mut stats = Stats::default();
    let start = Instant::now();
    for r in 0..rounds {
        let seed = base.wrapping_add(r.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut rng = Rng(seed);
        let w = world();
        let repro = format!("ROTTEN_CRASH_SEED={seed} ROTTEN_CRASH_ROUNDS=1");
        let batch = rng.upto(ITEMS as u64) as usize + 1;
        let k = rng.upto(2 * ITEMS as u64 + 2) as usize;
        let jitter = Duration::from_micros(rng.upto(JITTER_MAX_US));
        let args = ["clean", "--permanent", "--yes", "--json"];
        let ending = run_and_kill(&w, &bin, &args, batch, k, jitter);
        if let Ending::Exited(code) = ending {
            assert_eq!(code, Some(0), "clean --permanent terminó mal ({repro})");
        }
        stats.note(&ending);
        let ctx = format!(
            "clean --permanent lote={batch} k={k} jitter={jitter:?}, {} ({repro})",
            ending.describe()
        );
        recover_then(&w, &ctx, |w, was_open| {
            check_permanent(w, was_open, &ctx);
            0
        });
    }
    eprintln!(
        "arnés kill -9 (--permanent): {rounds} rondas en {:.1?}; {} cortes, {} completas",
        start.elapsed(),
        stats.killed,
        stats.exited,
    );
}

/// Una limpieza muere después de mover el elemento a la Papelera y antes de
/// anotar el resultado. La operación registró qué Papelera usaba
/// (`trash: dir:<ruta>`) y el elemento está allí con el mismo inode:
/// `recover_incomplete` lo busca allí por identidad, lo anota como
/// `trashed` con su ruta y el deshacer puede devolverlo.
#[test]
fn recovered_trash_item_keeps_its_location() {
    let w = world();
    let preview = w
        .core
        .plan(&Selection::default(), Action::Trash)
        .expect("plan");
    let item = preview.plan.items()[0].clone();
    let paths = HistoryPaths::new(&w.state);
    let mut j = Journal::open(&paths, "op-clean-crash", SystemTime::now()).expect("journal");
    j.write(
        Event::OpStarted {
            kind: OpKind::Clean,
            mode: OpMode::Trash,
            plan_hash: Some(preview.plan.plan_hash().into()),
            undo_of: None,
            items: 1,
            bytes: item.bytes_allocated,
            by: "test".into(),
            trash: TrashBackend::Dir(w.trash.clone()).describe(),
        },
        true,
    )
    .expect("start");
    j.write(
        Event::ItemIntent {
            item: item.item_id,
            action: StepAction::Trash,
            path: item.path.clone(),
            identity: item.identity,
            target: None,
            category: Some(item.category.clone()),
            regenerable: item.regenerable,
            bytes: item.bytes_allocated,
            parent_mode: None,
            origin: Some(item.origin),
            of: None,
        },
        true,
    )
    .expect("intent");
    let to = w.trash.join(item.path.file_name().expect("nombre"));
    fs::rename(&item.path, &to).expect("mover a la Papelera");
    drop(j); // «Cierre abrupto»: sin item_done ni op_finished.

    assert_eq!(w.core.recover().expect("recover"), vec!["op-clean-crash"]);
    let (records, _) = read_journal(&paths).expect("journal");
    let m = manifest_for(&records, "op-clean-crash").expect("manifiesto");
    assert_eq!(m.items[0].state, ItemState::Trashed);
    assert_eq!(m.items[0].trashed_path.as_ref(), Some(&to));
    let undo = prepare_undo(&records, "op-clean-crash", None).expect("deshacer");
    assert_eq!(undo.ready.len(), 1, "{:?}", undo.not_recoverable);
}
