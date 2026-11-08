//! `rotten trash put <ruta…>`: mover a la Papelera rutas elegidas a mano
//! (T-10), por el mismo pipeline que `clean`: plan con hash, revisión,
//! confirmación (o `--yes`), verificación justo antes de actuar, journal y
//! `rotten undo`.
//!
//! Códigos propios: `5` si alguna ruta está protegida, es ancestro de una
//! protegida o no se puede resolver (no se mueve nada); `4` si alguna ruta
//! se queda fuera (excluida, bloqueada) o se omite al ejecutar.

use std::path::PathBuf;

use ra_core::{Core, ExecReport, Outcome, tilde};
use serde_json::{Value, json};

use crate::args::TrashCmd;
use crate::text::size;
use crate::{Exit, Ui, exec_exit, print_plan};

const COMMAND: &str = "trash";

pub fn run(ui: &Ui, core: &Core, action: TrashCmd) -> Exit {
    match action {
        TrashCmd::Put { paths, dry_run } => put(ui, core, &paths, dry_run),
    }
}

fn put(ui: &Ui, core: &Core, paths: &[PathBuf], dry_run: bool) -> Exit {
    // Las rutas relativas lo son al directorio actual. El núcleo resuelve
    // `..` y los enlaces de las carpetas padre, nunca el último componente.
    let mut abs = Vec::with_capacity(paths.len());
    for p in paths {
        match std::path::absolute(p) {
            Ok(a) => abs.push(a),
            Err(e) => return ui.fail(COMMAND, Exit::Usage, &format!("{}: {e}", p.display())),
        }
    }
    let preview = match core.plan_selection(&abs) {
        Ok(p) => p,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    // Cada ruta la ha pedido el usuario: si alguna se queda fuera del plan,
    // la operación no está completa.
    let left_out = !preview.plan.skipped().is_empty();

    if dry_run {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "action": "put", "dry_run": true, "plan": preview.plan, "preview": preview.dry_run }),
            );
        } else {
            print_plan(ui, &preview.plan, &preview.dry_run);
            ui.say(ui.t(
                "Simulación: no se ha cambiado nada.",
                "Dry run: nothing was changed.",
            ));
        }
        return Exit::Ok;
    }
    if preview.plan.items().is_empty() {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "action": "put", "dry_run": false, "plan": preview.plan, "report": ExecReport::default() }),
            );
        } else {
            print_plan(ui, &preview.plan, &preview.dry_run);
            ui.say(ui.t(
                "Nada que mover a la Papelera.",
                "Nothing to move to the Trash.",
            ));
        }
        return if left_out { Exit::Partial } else { Exit::Ok };
    }
    if !ui.json {
        print_plan(ui, &preview.plan, &preview.dry_run);
    }
    let question = format!(
        "{} {} {} ({})?",
        ui.t("¿Mover a la Papelera", "Move to the Trash"),
        preview.plan.totals().items,
        ui.t("elementos", "items"),
        size(preview.plan.totals().bytes_allocated, ui.lang)
    );
    if let Err(code) = ui.confirm(COMMAND, &question) {
        return code;
    }
    let hash = preview.plan.plan_hash().to_owned();
    let plan_json = serde_json::to_value(&preview.plan).unwrap_or(Value::Null);
    let report = match core.execute(preview.plan, &hash, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    if ui.json {
        ui.emit(
            COMMAND,
            json!({ "action": "put", "dry_run": false, "plan": plan_json, "report": report }),
        );
    } else {
        for r in &report.items {
            if let Outcome::Skipped { message, .. } | Outcome::Failed { error: message } =
                &r.outcome
            {
                ui.say(format!(
                    "  ! {} — {message}",
                    tilde(&r.path, &core.env().home)
                ));
            }
        }
        ui.say(format!(
            "{} {} {} ({}). {}",
            ui.t("Movido a la Papelera:", "Moved to the Trash:"),
            report.done,
            ui.t("elementos", "items"),
            size(report.bytes_done, ui.lang),
            ui.t(
                "Aún no se ha liberado espacio: eso ocurre al vaciar la Papelera.",
                "No space is freed yet: that happens when the Trash is emptied.",
            ),
        ));
        if let Some(op) = &report.op_id {
            ui.say(format!(
                "{} rotten undo {op}",
                ui.t("Para deshacer:", "To undo:")
            ));
        }
    }
    if left_out {
        Exit::Partial
    } else {
        exec_exit(&report)
    }
}
