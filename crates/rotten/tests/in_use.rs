//! Aceptación de T-04 en el CLI (RF-SEG-08): un elemento que un proceso
//! tiene abierto no se toca; se omite con el motivo `in_use` y el pid del
//! proceso, y la operación termina como parcial (`4`). La simulación ya lo
//! prevé y sale con el mismo código.

mod common;

use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use common::fixture;

/// Proceso hijo que mantiene abierto un fichero. Se mata siempre al salir
/// de la prueba, también si una aserción falla.
struct Holder(Child);

impl Holder {
    fn open(path: &Path) -> Holder {
        let file = File::open(path).expect("abrir el fichero que se retiene");
        let child = Command::new("/bin/sleep")
            .arg("120")
            .stdin(Stdio::from(file))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("lanzar sleep");
        Holder(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn clean_skips_an_item_held_open_by_a_process() {
    let fx = fixture();
    // Preseleccionado: entra en `clean` sin `--category`.
    let held = fx.p("Library/Logs/install.log");
    let holder = Holder::open(&held);

    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 4, "{doc:#}");

    let items = doc["report"]["items"].as_array().expect("items");
    let held_str = held.to_str().expect("ruta UTF-8");
    let item = items
        .iter()
        .find(|i| i["path"] == held_str)
        .unwrap_or_else(|| panic!("{held_str} no está en el informe: {doc:#}"));
    assert_eq!(item["outcome"], "skipped", "{item:#}");
    assert_eq!(item["reason"], "in_use", "{item:#}");
    assert_eq!(item["detail"]["kind"], "held", "{item:#}");
    assert_eq!(item["detail"]["pid"], holder.pid(), "{item:#}");
    assert_eq!(item["detail"]["process"], "sleep", "{item:#}");
    assert!(held.exists(), "el fichero en uso no debe moverse");

    // El resto del plan sí se ejecuta.
    assert_eq!(doc["report"]["skipped"], 1, "{doc:#}");
    assert!(
        doc["report"]["done"].as_u64().expect("done") >= 1,
        "{doc:#}"
    );
    assert!(items.iter().all(|i| i == item || i["outcome"] == "done"));

    // Sin el proceso, la siguiente limpieza ya lo mueve.
    drop(holder);
    let (code, doc) = fx.run(&["clean", "--yes", "--json"]);
    assert_eq!(code, 0, "{doc:#}");
    assert!(!held.exists(), "{doc:#}");
}

/// `--dry-run` sale con 4 si la simulación ya prevé omisiones, como la
/// ejecución; sin omisiones, 0. No se mueve nada.
#[test]
fn dry_run_exits_4_when_it_foresees_an_item_in_use() {
    let fx = fixture();
    let held = fx.p("Library/Logs/install.log");
    let held_str = held.to_str().expect("ruta UTF-8").to_owned();
    let (code, doc) = fx.run(&["clean", "--dry-run", "--json"]);
    assert_eq!(code, 0, "sin omisiones: {doc:#}");

    let holder = Holder::open(&held);
    for args in [
        vec!["clean", "--dry-run", "--json"],
        vec!["trash", "put", "--dry-run", "--json", &held_str],
    ] {
        let (code, doc) = fx.run(&args);
        assert_eq!(code, 4, "{args:?}: {doc:#}");
        let item = doc["preview"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|i| i["path"] == held_str.as_str())
            .unwrap_or_else(|| panic!("{held_str} no está en la simulación: {doc:#}"))
            .clone();
        assert_eq!(item["outcome"], "skipped", "{item:#}");
        assert_eq!(item["reason"], "in_use", "{item:#}");
        assert_eq!(item["detail"]["pid"], holder.pid(), "{item:#}");
        assert_eq!(doc["preview"]["skipped"], 1, "{doc:#}");
    }
    assert!(held.exists());
    assert!(fs::read_dir(&fx.trash).expect("trash").next().is_none());
}

/// I-1: con otra caja en la ruta pedida, un fichero en uso dentro de la
/// carpeta se sigue detectando (la detección compara por prefijo con la
/// ruta del plan, que ahora lleva el nombre real).
#[test]
fn a_held_file_is_found_inside_a_folder_typed_with_another_case() {
    let fx = fixture();
    let typed = fx.p("Library/Caches/COM.EXAMPLE.EDITOR");
    if !typed.exists() {
        eprintln!("volumen con distinción de mayúsculas: no aplica");
        return;
    }
    let real = fx.p("Library/Caches/com.example.editor");
    let holder = Holder::open(&real.join("data.bin"));
    let typed = typed.to_str().expect("ruta UTF-8");
    let (code, doc) = fx.run(&["trash", "put", "--dry-run", "--json", typed]);
    assert_eq!(code, 4, "{doc:#}");
    let item = &doc["preview"]["items"][0];
    assert_eq!(item["path"], real.to_str().expect("ruta UTF-8"), "{item:#}");
    assert_eq!(item["reason"], "in_use", "{item:#}");
    assert_eq!(item["detail"]["pid"], holder.pid(), "{item:#}");
}
