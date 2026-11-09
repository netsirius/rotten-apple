//! Tamaños honestos en `scan` (T-05: RF-SEG-12, RNF-ARQ-28, RF-PAN-04).
//!
//! Cada caso vive en `raíz/{home,trash,state}` dentro de un directorio
//! temporal: nunca se toca el `~` real. Los elementos son registros de
//! `~/Library/Logs` (CAT-03, preseleccionados, ≥ 7 días).

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use ra_core::{CategorySummary, Core, Env, ScanReport, TrashBackend};

const MB: usize = 1 << 20;

struct World {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    core: Core,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = fs::canonicalize(tmp.path()).expect("canon");
        let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
        for d in [&home, &trash, &state, &home.join("Library/Logs")] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let env = Env::new(home.clone(), state, TrashBackend::Dir(trash));
        let core = Core::with_apps(env, Box::new(BTreeSet::<String>::new())).expect("core");
        World {
            _tmp: tmp,
            home,
            core,
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn write(&self, rel: &str, len: usize) {
        let p = self.p(rel);
        fs::create_dir_all(p.parent().expect("padre")).expect("mkdir");
        fs::write(&p, vec![7u8; len]).expect("write");
    }

    fn clone_file(&self, from: &str, to: &str) {
        let to = self.p(to);
        fs::create_dir_all(to.parent().expect("padre")).expect("mkdir");
        let st = Command::new("cp")
            .arg("-c")
            .arg(self.p(from))
            .arg(&to)
            .status()
            .expect("cp -c");
        assert!(st.success(), "cp -c falló");
    }

    fn hard_link(&self, from: &str, to: &str) {
        let to = self.p(to);
        fs::create_dir_all(to.parent().expect("padre")).expect("mkdir");
        fs::hard_link(self.p(from), to).expect("link");
    }

    /// Envejece todo lo de `~/Library/Logs` para que no lo bloquee la
    /// antigüedad mínima, y escanea CAT-03.
    fn scan_logs(&self) -> ScanReport {
        age(
            &self.p("Library/Logs"),
            SystemTime::now() - Duration::from_secs(30 * 86_400),
        );
        self.core.scan(Some(&["CAT-03".to_string()])).expect("scan")
    }

    fn allocated(&self, rel: &str) -> u64 {
        fs::symlink_metadata(self.p(rel)).expect("lstat").blocks() * 512
    }
}

fn age(dir: &Path, when: SystemTime) {
    for e in fs::read_dir(dir).expect("read_dir") {
        let path = e.expect("entrada").path();
        if path.is_dir() {
            age(&path, when);
        }
        File::open(&path)
            .and_then(|f| f.set_modified(when))
            .expect("set_modified");
    }
}

fn logs(r: &ScanReport) -> &CategorySummary {
    r.categories
        .iter()
        .find(|c| c.id == "CAT-03")
        .expect("CAT-03")
}

fn clone_note(c: &CategorySummary) -> bool {
    c.notes.iter().any(|n| n.contains("clones APFS"))
}

#[test]
fn a_clone_in_the_selection_counts_once_and_warns() {
    let w = World::new();
    w.write("Library/Logs/a.log", MB);
    w.clone_file("Library/Logs/a.log", "Library/Logs/b.log");
    let one = w.allocated("Library/Logs/a.log");
    let r = w.scan_logs();
    let c = logs(&r);
    assert_eq!((c.items, c.preselected_items), (2, 2), "{c:?}");
    // En disco ocupan dos veces lo que se liberaría: los bloques son los
    // mismos.
    assert_eq!(c.bytes_allocated, 2 * one);
    assert_eq!(c.reclaimable_bytes, one, "{c:?}");
    assert_eq!(c.preselected_reclaimable_bytes, one, "{c:?}");
    assert_eq!(c.cloned_entries, 2);
    assert!(clone_note(c), "{:?}", c.notes);
}

#[test]
fn a_clone_outside_the_selection_frees_nothing() {
    let w = World::new();
    w.write("Library/Logs/a.log", MB);
    w.clone_file("Library/Logs/a.log", "Documents/copia.log");
    let r = w.scan_logs();
    let c = logs(&r);
    assert_eq!(c.items, 1, "{c:?}");
    assert_eq!(c.bytes_allocated, w.allocated("Library/Logs/a.log"));
    assert_eq!(c.reclaimable_bytes, 0, "{c:?}");
    assert!(clone_note(c), "{:?}", c.notes);
}

#[test]
fn a_hard_link_in_two_items_counts_once() {
    let w = World::new();
    w.write("Library/Logs/x/data.bin", MB);
    w.hard_link("Library/Logs/x/data.bin", "Library/Logs/y/data.bin");
    let r = w.scan_logs();
    let c = logs(&r);
    assert_eq!((c.items, c.preselected_items), (2, 2), "{c:?}");
    let once = w.allocated("Library/Logs/x/data.bin");
    // Las dos carpetas pueden ocupar algún bloque propio; el fichero, una vez.
    let dirs = w.allocated("Library/Logs/x") + w.allocated("Library/Logs/y");
    assert_eq!(c.bytes_allocated, once + dirs, "{c:?}");
    assert_eq!(c.preselected_bytes, once + dirs, "{c:?}");
    assert_eq!(c.reclaimable_bytes, once + dirs, "{c:?}");
    assert_eq!(c.hardlinked_entries, 2);
    assert!(!clone_note(c));
}

#[test]
fn plain_files_report_no_clones_and_the_volume() {
    let w = World::new();
    w.write("Library/Logs/a.log", MB);
    let r = w.scan_logs();
    let c = logs(&r);
    assert_eq!(c.reclaimable_bytes, c.bytes_allocated);
    assert_eq!((c.cloned_entries, c.hardlinked_entries), (0, 0));
    let v = r.volume.expect("estado del volumen");
    assert!(v.capacity_bytes > 0);
    assert_eq!(v.used_bytes + v.free_bytes, v.capacity_bytes);
    if let Some(a) = v.available_bytes {
        assert!(a >= v.free_bytes);
        assert_eq!(v.purgeable_bytes, Some(a - v.free_bytes));
    } else {
        assert_eq!(v.purgeable_bytes, None);
    }
}
