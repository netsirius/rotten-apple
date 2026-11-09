//! El escaneo no altera el `atime` (RNF-ARQ-09) y crear `Core` deja fijadas
//! las políticas de E/S del proceso (T-06, RF-SEG-10).
//!
//! Qué se comprueba de verdad: en APFS el `atime` solo se actualiza si es
//! anterior al `mtime`, y el escaneo no lee el contenido de ningún fichero:
//! lista carpetas (`readdir`), hace `lstat` y abre carpetas con
//! `O_DIRECTORY`. Así que se envejece el `atime` de todo el árbol por debajo
//! de su `mtime` (el caso en que APFS sí lo cambiaría) y se exige que, tras
//! `scan`, ningún `atime` haya cambiado. Lo que de verdad cambiaría sin la
//! política son las carpetas listadas; el control del principio demuestra
//! que en este volumen listar una carpeta sí cambia su `atime`.
//!
//! Las políticas son de proceso, así que todo va en una sola función: el
//! control tiene que ir antes de crear `Core`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, FileTimes};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ra_catalog::fixture::{Entry, materialize};
use ra_core::{Core, Env, TrashBackend};
use ra_platform::iopolicy::{self, ProcessPolicies};

const DAY: Duration = Duration::from_secs(86_400);

/// Pone el `atime` un día antes del `mtime`, sin cambiar el `mtime`.
fn age_atime(p: &Path) {
    let mtime = fs::symlink_metadata(p)
        .expect("lstat")
        .modified()
        .expect("mtime");
    File::open(p)
        .expect("abrir")
        .set_times(
            FileTimes::new()
                .set_accessed(mtime - DAY)
                .set_modified(mtime),
        )
        .expect("fijar tiempos");
}

fn atime(p: &Path) -> SystemTime {
    fs::symlink_metadata(p)
        .expect("lstat")
        .accessed()
        .expect("atime")
}

/// La raíz y todo lo que cuelga de ella (no hay enlaces en el fixture).
fn tree(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let mut i = 0;
    while i < out.len() {
        if out[i].is_dir() {
            let mut kids: Vec<_> = fs::read_dir(&out[i])
                .expect("read_dir")
                .map(|e| e.expect("entrada").path())
                .collect();
            kids.sort();
            out.extend(kids);
        }
        i += 1;
    }
    out
}

fn snapshot(paths: &[PathBuf]) -> BTreeMap<PathBuf, SystemTime> {
    paths.iter().map(|p| (p.clone(), atime(p))).collect()
}

#[test]
fn creating_core_sets_policies_and_scan_keeps_atime() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = fs::canonicalize(tmp.path()).expect("canon");

    // Control, aún con las políticas heredadas.
    let control = root.join("control");
    fs::create_dir(&control).expect("mkdir");
    fs::write(control.join("f"), b"x").expect("escribir");
    age_atime(&control);
    let before = atime(&control);
    assert_eq!(fs::read_dir(&control).expect("read_dir").count(), 1);
    // Si el control falla (política heredada o volumen con `noatime`), la
    // parte del `atime` no es concluyente y se omite; las políticas se
    // comprueban igual.
    let control_ok = atime(&control) != before;
    if !control_ok {
        eprintln!("este volumen no actualiza el atime al listar: atime no concluyente");
    }

    let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
    for d in [&home, &trash, &state] {
        fs::create_dir_all(d).expect("mkdir");
    }
    let entries = [
        "Library/Logs/old-app/app.log",
        "Library/Logs/install.log",
        "Library/Caches/com.example.app/data.bin",
    ]
    .map(|p| Entry {
        path: p.into(),
        size: 4096,
        age_days: 30,
        dir: false,
    });
    materialize(&home, &entries, &[], SystemTime::now()).expect("materializar");

    let env = Env::new(home.clone(), state, TrashBackend::Dir(trash));
    let core = Core::with_apps(env, Box::new(BTreeSet::<String>::new())).expect("core");
    assert_eq!(
        iopolicy::current().expect("getiopolicy_np"),
        ProcessPolicies::TARGET
    );

    let paths = tree(&home);
    for p in &paths {
        age_atime(p);
    }
    let before = snapshot(&paths);

    let report = core.scan(None).expect("scan");
    let found: u64 = report.categories.iter().map(|c| c.items).sum();
    assert!(
        found >= 3,
        "el escaneo debe recorrer el fixture: {report:?}"
    );

    if !control_ok {
        return;
    }
    let after = snapshot(&paths);
    let changed: Vec<_> = paths.iter().filter(|p| before[*p] != after[*p]).collect();
    assert!(changed.is_empty(), "atime cambiado tras scan: {changed:?}");
}
