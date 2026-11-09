//! `rotten free <op-id…>` («Liberar ahora», T-11, CU-08): borra para
//! siempre de la Papelera **solo** los elementos que mandaron allí esas
//! operaciones, tras revisar el plan y confirmar (o `--yes`).
//!
//! Códigos: `0` todo liberado · `1` operación desconocida o que no mandó
//! nada a la Papelera · `2` falta `--yes` sin terminal · `3` la Papelera no
//! se puede leer (Acceso total al disco) · `4` algún elemento se omite
//! (modificado o sustituido dentro de la Papelera, ya no está…) · `5` el
//! plan cambió entre la revisión y la ejecución · `6` cerrojo. Con
//! `--dry-run`, el código que tendría la ejecución.

use ra_core::{Core, ExecReport, FreePreview, Outcome, SpaceReport};
use serde_json::{Value, json};

use crate::i18n::tr;
use crate::{Exit, Ui, exec_exit, print_problems, render};

const COMMAND: &str = "free";

fn print_plan(ui: &Ui, p: &FreePreview) {
    ui.say(tr!(
        ui,
        "free-header",
        count = p.plan.totals.items,
        size = ui.size(p.plan.totals.bytes)
    ));
    for (it, r) in p.plan.ready.iter().zip(&p.dry_run.items) {
        let id = it.item_id.to_string();
        let path = ui.path(&it.path);
        let size = ui.size(it.bytes);
        let line = match &r.outcome {
            Outcome::Skipped { cause, .. } => tr!(
                ui,
                "free-item-would-skip",
                id = id,
                path = path,
                size = size,
                reason = render::skip_cause(&ui.i18n, cause, &ui.home)
            ),
            _ => tr!(
                ui,
                "free-item",
                id = id,
                path = path,
                original = ui.path(&it.original_path),
                size = size
            ),
        };
        ui.say_in(1, line);
    }
    for n in &p.plan.not_freeable {
        ui.say_in(
            1,
            tr!(
                ui,
                "free-not-freeable",
                op = n.op_id.as_str(),
                item = n.item_id.to_string(),
                reason = render::not_recoverable(&ui.i18n, &n.reason)
            ),
        );
    }
}

pub(crate) fn print_space(ui: &Ui, s: &SpaceReport) {
    let estimated = ui.size(s.estimated_bytes);
    match s.measured_bytes {
        Some(m) => {
            ui.say(tr!(
                ui,
                "freed-space",
                measured = ui.size(u64::try_from(m).unwrap_or(0)),
                estimated = estimated
            ));
            if ra_core::far_below(s.estimated_bytes, m) {
                ui.say(tr!(ui, "freed-space-less"));
            }
        }
        None => ui.say(tr!(ui, "freed-space-unmeasured", estimated = estimated)),
    }
}

pub fn run(ui: &Ui, core: &Core, ops: &[String], dry_run: bool) -> Exit {
    let preview = match core.prepare_free(ops) {
        Ok(p) => p,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    // Algo de lo pedido sigue sin liberar y no se puede liberar ahora (ya
    // no está en la Papelera tal como se anotó, paradero incierto, otra
    // Papelera, volumen sin montar): la operación no queda completa (4). Lo
    // ya borrado o restaurado no deja nada pendiente.
    let left_out = preview
        .plan
        .not_freeable
        .iter()
        .any(|n| !n.reason.is_settled());
    if !ui.json {
        print_plan(ui, &preview);
    }
    if dry_run {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "dry_run": true, "plan": preview.plan, "report": preview.dry_run }),
            );
        } else {
            ui.say(tr!(ui, "dry-run-nothing-changed"));
        }
        return if left_out {
            Exit::Partial
        } else {
            exec_exit(&preview.dry_run)
        };
    }
    if preview.plan.ready.is_empty() {
        if ui.json {
            // M6: no se ha medido nada.
            let space = SpaceReport {
                estimated_bytes: 0,
                measured_bytes: None,
                volumes: vec![],
            };
            ui.emit(
                COMMAND,
                json!({ "dry_run": false, "plan": preview.plan, "report": ExecReport::default(), "space": space }),
            );
        } else {
            ui.say(tr!(ui, "free-nothing"));
        }
        return if left_out { Exit::Partial } else { Exit::Ok };
    }
    // RF-SEG-03: número de elementos, tamaño y «no se puede deshacer»; por
    // defecto, «No».
    let question = tr!(
        ui,
        "confirm-free",
        count = preview.plan.totals.items,
        size = ui.size(preview.plan.totals.bytes)
    );
    if let Err(code) = ui.confirm(COMMAND, &question) {
        return code;
    }
    let hash = preview.plan.plan_hash.clone();
    let plan_json = serde_json::to_value(&preview.plan).unwrap_or(Value::Null);
    let result = match core.free(&preview.plan, &hash, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    crate::techlog::report(COMMAND, &result.report);
    if ui.json {
        ui.emit(
            COMMAND,
            json!({ "dry_run": false, "plan": plan_json, "report": result.report, "space": result.space }),
        );
    } else {
        print_problems(ui, &result.report);
        ui.say(tr!(
            ui,
            "freed",
            count = result.report.done,
            size = ui.size(result.report.bytes_done)
        ));
        print_space(ui, &result.space);
    }
    if left_out {
        Exit::Partial
    } else {
        exec_exit(&result.report)
    }
}
