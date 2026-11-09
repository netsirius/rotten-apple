//! Políticas de E/S del proceso (T-06, RF-SEG-10, RNF-ARQ-09).
//!
//! Las políticas tienen ámbito de proceso y las pruebas de un mismo binario
//! comparten proceso, así que todo va en una sola función, en orden: primero
//! el control con las políticas heredadas y después `apply`.

use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::time::{Duration, SystemTime};

use ra_platform::iopolicy::{self, ProcessPolicies};

/// `atime` estrictamente anterior al `mtime`: es el caso en que APFS sí lo
/// actualiza al leer.
fn age_dir(dir: &Path) {
    let now = SystemTime::now();
    let day = Duration::from_secs(86_400);
    File::open(dir)
        .expect("abrir carpeta")
        .set_times(
            FileTimes::new()
                .set_accessed(now - day * 2)
                .set_modified(now - day),
        )
        .expect("fijar tiempos");
}

fn atime(p: &Path) -> SystemTime {
    fs::symlink_metadata(p)
        .expect("lstat")
        .accessed()
        .expect("atime")
}

/// Lista la carpeta (lo que hace el escaneo) y dice si cambió su `atime`.
fn listing_changes_atime(dir: &Path) -> bool {
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(dir.join("f"), b"x").expect("escribir");
    age_dir(dir);
    let before = atime(dir);
    let n = fs::read_dir(dir).expect("read_dir").count();
    assert_eq!(n, 1);
    atime(dir) != before
}

#[test]
fn apply_sets_the_process_policies_and_stops_atime_updates() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Control: con la política de `atime` heredada (la por defecto), listar
    // una carpeta sí cambia su `atime` en este volumen. Si no, el resto de la
    // prueba no demostraría nada.
    // Si el control falla (política heredada o volumen con `noatime`), la
    // parte del `atime` no es concluyente y se omite; las políticas se
    // comprueban igual.
    let inherited = iopolicy::current().expect("getiopolicy_np");
    let control_ok = if inherited.atime_updates != 0 {
        eprintln!("el proceso de prueba ya hereda IOPOL_ATIME_UPDATES_OFF: atime no concluyente");
        false
    } else if !listing_changes_atime(&tmp.path().join("control")) {
        eprintln!("este volumen no actualiza el atime al listar: atime no concluyente");
        false
    } else {
        true
    };

    iopolicy::apply().expect("setiopolicy_np");
    assert_eq!(
        iopolicy::current().expect("getiopolicy_np"),
        ProcessPolicies::TARGET
    );

    // Repetir es inocuo (cada `Core` la llama al crearse).
    iopolicy::apply().expect("segunda llamada");
    assert_eq!(
        iopolicy::current().expect("getiopolicy_np"),
        ProcessPolicies::TARGET
    );

    if !control_ok {
        return;
    }
    assert!(
        !listing_changes_atime(&tmp.path().join("con-politica")),
        "con IOPOL_ATIME_UPDATES_OFF listar no debe cambiar el atime"
    );
}
