//! Aceptación de T-04 en el CLI (RF-SEG-08): un elemento que un proceso
//! tiene abierto no se toca; se omite con el motivo `in_use` y el pid del
//! proceso, y la operación termina como parcial (`4`).

mod common;

use std::fs::File;
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
