//! `rotten trash put <ruta…>`: mover a la Papelera rutas elegidas a mano
//! (T-10), por el mismo pipeline que `clean`: plan con hash, revisión,
//! confirmación (o `--yes`), verificación justo antes de actuar, journal y
//! `rotten undo`.
//!
//! Códigos propios: `5` si alguna ruta está protegida, es ancestro de una
//! protegida o no se puede resolver (no se mueve nada); `4` si alguna ruta
//! se queda fuera (excluida, bloqueada) o se omite al ejecutar.

use std::path::PathBuf;

use ra_core::{Core, ExecReport, SkipReason};
use serde_json::{Value, json};

use crate::args::TrashCmd;
use crate::i18n::tr;
use crate::{Exit, Ui, exec_exit, print_plan, print_problems};

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
            Err(e) => {
                let message = format!("{}: {e}", p.display());
                let human = tr!(
                    ui,
                    "invalid-path",
                    path = p.display().to_string(),
                    detail = e.to_string()
                );
                return ui.fail(COMMAND, Exit::Usage, "invalid_path", &message, &human);
            }
        }
    }
    let preview = match core.plan_selection(&abs) {
        Ok(p) => p,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    // Cada ruta la ha pedido el usuario: si alguna se queda fuera del plan,
    // la operación no está completa. Una ruta repetida o dentro de otra
    // elegida no cuenta: se mueve con la que la contiene.
    let left_out = preview
        .plan
        .skipped()
        .iter()
        .any(|s| !matches!(s.reason, SkipReason::Overlaps(_)));

    if dry_run {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "action": "put", "dry_run": true, "plan": preview.plan, "report": preview.dry_run }),
            );
        } else {
            print_plan(ui, core, &preview.plan, &preview.dry_run);
            ui.say(tr!(ui, "dry-run-nothing-changed"));
        }
        // Mismo código que tendría la ejecución: 4 si ya se prevén omisiones.
        return if left_out {
            Exit::Partial
        } else {
            exec_exit(&preview.dry_run)
        };
    }
    if preview.plan.items().is_empty() {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "action": "put", "dry_run": false, "plan": preview.plan, "report": ExecReport::default() }),
            );
        } else {
            print_plan(ui, core, &preview.plan, &preview.dry_run);
            ui.say(tr!(ui, "trash-nothing"));
        }
        return if left_out { Exit::Partial } else { Exit::Ok };
    }
    if !ui.json {
        print_plan(ui, core, &preview.plan, &preview.dry_run);
    }
    let question = tr!(
        ui,
        "confirm-trash",
        count = preview.plan.totals().items,
        size = ui.size(preview.plan.totals().bytes_allocated)
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
        print_problems(ui, &report);
        ui.say(tr!(
            ui,
            "moved-to-trash",
            count = report.done,
            size = ui.size(report.bytes_done)
        ));
        ui.say(tr!(ui, "not-freed-yet"));
        if let Some(op) = &report.op_id
            && report.done > 0
        {
            ui.say(tr!(ui, "to-undo", op = op.as_str()));
            ui.say(tr!(ui, "to-free", op = op.as_str()));
        }
    }
    if left_out {
        Exit::Partial
    } else {
        exec_exit(&report)
    }
}
