//! `rotten trash put <ruta…>`: mover a la Papelera rutas elegidas a mano
//! (T-10), por el mismo pipeline que `clean`: plan con hash, revisión,
//! confirmación (o `--yes`), verificación justo antes de actuar, journal y
//! `rotten undo`.
//!
//! Códigos propios: `5` si alguna ruta está protegida, es ancestro de una
//! protegida o no se puede resolver (no se mueve nada); `4` si alguna ruta
//! se queda fuera (excluida, bloqueada) o se omite al ejecutar.

use std::path::PathBuf;

use ra_core::{Core, EmptyPreview, ExecReport, Outcome, SkipReason, SpaceReport};
use serde_json::{Value, json};

use crate::args::TrashCmd;
use crate::cmd::free::print_space;
use crate::i18n::tr;
use crate::{Exit, Ui, exec_exit, print_plan, print_problems, render};

const COMMAND: &str = "trash";

pub fn run(ui: &Ui, core: &Core, action: Option<TrashCmd>) -> Exit {
    match action {
        None => list(ui, core),
        Some(TrashCmd::Put { paths, dry_run }) => put(ui, core, &paths, dry_run),
        Some(TrashCmd::Empty { dry_run }) => empty(ui, core, dry_run),
    }
}

/// `rotten trash`: lo que hay en la Papelera, por volumen. Sin Acceso total
/// al disco se dice y se muestra solo lo que anotó el journal (CU-08 A1).
fn list(ui: &Ui, core: &Core) -> Exit {
    let view = match core.trash_view() {
        Ok(v) => v,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    if ui.json {
        ui.emit(
            COMMAND,
            json!({ "action": "list", "readable": view.readable, "volumes": view.volumes }),
        );
        return Exit::Ok;
    }
    let mut any = false;
    for v in &view.volumes {
        let path = ui.path(&v.path);
        match v.total {
            Some(t) => {
                any |= t.items > 0;
                ui.say(tr!(
                    ui,
                    "trash-volume",
                    path = path,
                    count = t.items,
                    size = ui.size(t.bytes)
                ));
            }
            None => ui.say(tr!(ui, "trash-volume-unreadable", path = path)),
        }
        for o in &v.by_operation {
            ui.say_in(
                1,
                tr!(
                    ui,
                    "trash-by-op",
                    op = o.op_id.as_str(),
                    count = o.items,
                    size = ui.size(o.bytes)
                ),
            );
        }
        if let Some(o) = v.others {
            ui.say_in(
                1,
                tr!(ui, "trash-others", count = o.items, size = ui.size(o.bytes)),
            );
        }
        if v.partial {
            ui.say_in(1, tr!(ui, "trash-partial"));
        }
    }
    if any {
        ui.say(tr!(ui, "to-empty"));
    }
    Exit::Ok
}

fn print_empty_plan(ui: &Ui, p: &EmptyPreview) {
    ui.say(tr!(
        ui,
        "empty-header",
        count = p.plan.totals.items,
        size = ui.size(p.plan.totals.bytes)
    ));
    for (it, r) in p.plan.items.iter().zip(&p.dry_run.items) {
        let id = it.item_id.to_string();
        let path = ui.path(&it.path);
        let size = ui.size(it.bytes);
        let line = match (&r.outcome, &it.of) {
            (Outcome::Skipped { cause, .. }, _) => tr!(
                ui,
                "empty-item-would-skip",
                id = id,
                path = path,
                size = size,
                reason = render::skip_cause(&ui.i18n, cause, &ui.home)
            ),
            (_, Some(of)) => tr!(
                ui,
                "empty-item-op",
                id = id,
                path = path,
                size = size,
                op = of.op.as_str()
            ),
            (_, None) => tr!(ui, "empty-item", id = id, path = path, size = size),
        };
        ui.say_in(1, line);
    }
}

/// `rotten trash empty`: vacía la Papelera entera tras revisarla. Si no se
/// puede leer, termina con 3 y sugiere `rotten free`.
fn empty(ui: &Ui, core: &Core, dry_run: bool) -> Exit {
    let preview = match core.prepare_empty() {
        Ok(p) => p,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    if dry_run {
        if ui.json {
            ui.emit(
                COMMAND,
                json!({ "action": "empty", "dry_run": true, "plan": preview.plan, "report": preview.dry_run }),
            );
        } else {
            print_empty_plan(ui, &preview);
            ui.say(tr!(ui, "dry-run-nothing-changed"));
        }
        return exec_exit(&preview.dry_run);
    }
    if preview.plan.items.is_empty() {
        if ui.json {
            let space = SpaceReport {
                estimated_bytes: 0,
                measured_bytes: None,
                volumes: vec![],
            };
            ui.emit(
                COMMAND,
                json!({ "action": "empty", "dry_run": false, "plan": preview.plan, "report": ExecReport::default(), "space": space }),
            );
        } else {
            ui.say(tr!(ui, "empty-nothing"));
        }
        return Exit::Ok;
    }
    if !ui.json {
        print_empty_plan(ui, &preview);
    }
    let question = tr!(
        ui,
        "confirm-empty",
        count = preview.plan.totals.items,
        size = ui.size(preview.plan.totals.bytes)
    );
    if let Err(code) = ui.confirm(COMMAND, &question) {
        return code;
    }
    let hash = preview.plan.plan_hash.clone();
    let result = match core.empty_trash(&preview.plan, &hash, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error(COMMAND, &e),
    };
    if ui.json {
        ui.emit(
            COMMAND,
            json!({ "action": "empty", "dry_run": false, "plan": preview.plan, "report": result.report, "space": result.space }),
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
    exec_exit(&result.report)
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
