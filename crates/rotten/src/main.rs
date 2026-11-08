//! `rotten`: CLI de Rotten Apple sobre `ra-core` (07).
//!
//! Códigos de salida (README §8, 07 §5): `0` ok · `1` error · `2` uso o
//! falta `--yes` · `3` falta permiso · `4` completado con omisiones · `5`
//! abortado por seguridad o plan cambiado · `6` otro ejecutor activo.

#![forbid(unsafe_code)]

mod args;
mod cmd;
mod text;

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use ra_core::{
    Action, AppsProbe, Core, CoreError, DoctorReport, Env, ExecReport, FdaState, Outcome, Plan,
    ScanStatus, Selection, TrashInfo, UndoState, tilde,
};
use serde_json::{Value, json};

use args::{Cli, Cmd, ExcludeCmd};
use text::{Lang, size};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Exit {
    Ok = 0,
    Error = 1,
    Usage = 2,
    Permission = 3,
    Partial = 4,
    Safety = 5,
    Locked = 6,
}

impl From<Exit> for ExitCode {
    fn from(e: Exit) -> Self {
        ExitCode::from(e as u8)
    }
}

struct Ui {
    json: bool,
    interactive: bool,
    yes: bool,
    quiet: bool,
    lang: Lang,
}

impl Ui {
    fn t<'a>(&self, es: &'a str, en: &'a str) -> &'a str {
        self.lang.t(es, en)
    }

    fn say(&self, line: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", line.as_ref());
        }
    }

    fn emit(&self, command: &str, body: Value) {
        let mut doc = json!({ "schema_version": SCHEMA_VERSION, "command": command });
        if let (Value::Object(d), Value::Object(b)) = (&mut doc, body) {
            d.extend(b);
        }
        let text = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into());
        println!("{text}");
    }

    fn fail(&self, command: &str, code: Exit, msg: &str) -> Exit {
        if self.json {
            self.emit(
                command,
                json!({ "error": { "exit_code": code as u8, "message": msg } }),
            );
        } else {
            eprintln!("rotten: {msg}");
        }
        code
    }

    fn core_error(&self, command: &str, e: &CoreError) -> Exit {
        let code = match e {
            CoreError::Usage(_) => Exit::Usage,
            CoreError::Permission(_) => Exit::Permission,
            CoreError::Safety(_) => Exit::Safety,
            CoreError::Locked => Exit::Locked,
            CoreError::NotFound(_) | CoreError::Other(_) => Exit::Error,
        };
        self.fail(command, code, &e.to_string())
    }

    /// Confirmación: `--yes`, o una pregunta si hay terminal; si no, error
    /// de uso (07 §1.4).
    fn confirm(&self, command: &str, question: &str) -> Result<(), Exit> {
        if self.yes {
            return Ok(());
        }
        if !self.interactive {
            return Err(self.fail(
                command,
                Exit::Usage,
                self.t(
                    "sin terminal interactiva hace falta --yes (o --dry-run para solo revisar)",
                    "no interactive terminal: pass --yes (or --dry-run to only review)",
                ),
            ));
        }
        print!("{question} [{}] ", self.t("s/N", "y/N"));
        let _ = io::stdout().flush();
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
        let a = line.trim().to_lowercase();
        if ["s", "si", "sí", "y", "yes"].contains(&a.as_str()) {
            Ok(())
        } else {
            self.say(self.t(
                "Cancelado; no se ha tocado nada.",
                "Cancelled; nothing was touched.",
            ));
            Err(Exit::Ok)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let lang = Lang::detect(cli.lang.as_deref());
    let piped = !io::stdout().is_terminal();
    let json = cli.json || piped;
    let ui = Ui {
        json,
        interactive: !json && io::stdin().is_terminal(),
        yes: cli.yes,
        quiet: cli.quiet,
        lang,
    };
    let command = match &cli.cmd {
        Cmd::Scan { .. } => "scan",
        Cmd::Clean { .. } => "clean",
        Cmd::History => "history",
        Cmd::Undo { .. } => "undo",
        Cmd::Doctor => "doctor",
        Cmd::Exclude { .. } => "exclude",
        Cmd::Trash { .. } => "trash",
    };

    if ra_core::running_as_root() && !cli.allow_root {
        return ui
            .fail(
                command,
                Exit::Safety,
                ui.t(
                    "no se ejecuta como root: Rotten Apple trabaja con tus permisos (usa --allow-root si de verdad lo necesitas)",
                    "refusing to run as root: Rotten Apple works with your own permissions (use --allow-root if you really need it)",
                ),
            )
            .into();
    }

    let env = match Env::from_env() {
        Ok(e) => e,
        Err(e) => return ui.fail(command, Exit::Error, &e.to_string()).into(),
    };
    let core = match Core::new(env) {
        Ok(c) => c,
        Err(e) => return ui.core_error(command, &e).into(),
    };

    let exit = match cli.cmd {
        Cmd::Scan { categories } => scan(&ui, &core, categories),
        Cmd::Clean {
            dry_run,
            permanent,
            categories,
            include_review,
        } => clean(&ui, &core, dry_run, permanent, categories, include_review),
        Cmd::History => history(&ui, &core),
        Cmd::Undo {
            op_id,
            items,
            dry_run,
        } => undo(&ui, &core, op_id, items, dry_run),
        Cmd::Doctor => doctor(&ui, &core),
        Cmd::Exclude { action } => exclude(&ui, &core, action),
        Cmd::Trash { action } => cmd::trash::run(&ui, &core, action),
    };
    exit.into()
}

fn opt_categories(c: Vec<String>) -> Option<Vec<String>> {
    if c.is_empty() { None } else { Some(c) }
}

fn scan(ui: &Ui, core: &Core, categories: Vec<String>) -> Exit {
    let cats = opt_categories(categories);
    let report = match core.scan(cats.as_deref()) {
        Ok(r) => r,
        Err(e) => return ui.core_error("scan", &e),
    };
    if ui.json {
        // El informe va al nivel superior: `scan_status`, `categories`…
        let mut body = serde_json::to_value(&report).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut body {
            m.insert("home".into(), json!(core.env().home));
        }
        ui.emit("scan", body);
    } else {
        let status = match report.scan_status {
            ScanStatus::Complete => ui.t("completo", "complete"),
            ScanStatus::Partial => ui.t("parcial", "partial"),
            ScanStatus::Unavailable => ui.t("no disponible", "unavailable"),
        };
        ui.say(format!(
            "{} · {} {} · {status}",
            ui.t("Escaneo", "Scan"),
            ui.t("catálogo", "catalog"),
            report.catalog_version
        ));
        let (mut pre_items, mut pre_bytes) = (0, 0);
        for c in &report.categories {
            let title = ui.t(&c.title_es, &c.title_en);
            ui.say(format!(
                "  {}  {title:<34} {:>5} {}  {:>9}   {} {}",
                c.id,
                c.items,
                ui.t("elem.", "items"),
                size(c.bytes_allocated, ui.lang),
                ui.t("preseleccionado:", "preselected:"),
                size(c.preselected_bytes, ui.lang),
            ));
            for n in &c.notes {
                ui.say(format!("        ! {n}"));
            }
            pre_items += c.preselected_items;
            pre_bytes += c.preselected_bytes;
        }
        for w in &report.warnings {
            ui.say(format!("! {w}"));
        }
        ui.say(format!(
            "{} {} ({pre_items} {}). {}",
            ui.t("Preseleccionado:", "Preselected:"),
            size(pre_bytes, ui.lang),
            ui.t("elementos", "items"),
            ui.t(
                "Revisa con `rotten clean --dry-run`.",
                "Review with `rotten clean --dry-run`."
            ),
        ));
    }
    Exit::Ok
}

fn print_plan(ui: &Ui, plan: &Plan, preview: &ExecReport) {
    let home = plan.home();
    let dest = match plan.action() {
        Action::Trash => ui.t("a la Papelera", "to the Trash"),
        Action::Delete => ui.t("BORRADO PERMANENTE", "PERMANENT DELETE"),
    };
    ui.say(format!(
        "{} {} · {} {} · {} {dest}",
        ui.t("Plan", "Plan"),
        &plan.plan_hash()[..plan.plan_hash().len().min(19)],
        plan.totals().items,
        ui.t("elementos", "items"),
        size(plan.totals().bytes_allocated, ui.lang),
    ));
    for (item, r) in plan.items().iter().zip(&preview.items) {
        let note = match &r.outcome {
            Outcome::Skipped { message, .. } => {
                format!("  ({} {message})", ui.t("se omitiría:", "would skip:"))
            }
            _ => String::new(),
        };
        ui.say(format!(
            "  [{}] {}  {}  {}{note}",
            item.item_id,
            tilde(&item.path, home),
            item.category,
            size(item.bytes_allocated, ui.lang),
        ));
    }
    if !plan.skipped().is_empty() {
        ui.say(format!(
            "{} ({}):",
            ui.t("Omitidos", "Skipped"),
            plan.skipped().len()
        ));
        for s in plan.skipped() {
            let why = serde_json::to_string(&s.reason).unwrap_or_default();
            ui.say(format!("  {} — {why}", tilde(&s.path, home)));
        }
    }
    for (cat, ns) in plan.not_selected() {
        ui.say(format!(
            "{} {cat}: {} {} ({}) — {} --category {cat}",
            ui.t("Sin marcar en", "Not selected in"),
            ns.items,
            ui.t("elementos", "items"),
            size(ns.bytes_allocated, ui.lang),
            ui.t("añádelos con", "add them with"),
        ));
    }
}

fn exec_exit(report: &ExecReport) -> Exit {
    if report.skipped + report.failed > 0 {
        Exit::Partial
    } else {
        Exit::Ok
    }
}

fn clean(
    ui: &Ui,
    core: &Core,
    dry_run: bool,
    permanent: bool,
    categories: Vec<String>,
    include_review: bool,
) -> Exit {
    if permanent && !ui.yes {
        return ui.fail(
            "clean",
            Exit::Usage,
            ui.t(
                "--permanent borra sin pasar por la Papelera y exige --yes",
                "--permanent deletes without the Trash and requires --yes",
            ),
        );
    }
    let selection = Selection {
        categories: opt_categories(categories),
        include_review,
    };
    let action = if permanent {
        Action::Delete
    } else {
        Action::Trash
    };
    let preview = match core.plan(&selection, action) {
        Ok(p) => p,
        Err(e) => return ui.core_error("clean", &e),
    };
    if dry_run {
        if ui.json {
            ui.emit(
                "clean",
                json!({ "dry_run": true, "plan": preview.plan, "preview": preview.dry_run }),
            );
        } else {
            print_plan(ui, &preview.plan, &preview.dry_run);
            ui.say(ui.t(
                "Simulación: no se ha cambiado nada.",
                "Dry run: nothing was changed.",
            ));
        }
        // Mismo código que tendría la ejecución: 4 si ya se prevén omisiones.
        return exec_exit(&preview.dry_run);
    }
    if preview.plan.items().is_empty() {
        if ui.json {
            ui.emit(
                "clean",
                json!({ "dry_run": false, "plan": preview.plan, "report": ExecReport::default() }),
            );
        } else {
            print_plan(ui, &preview.plan, &preview.dry_run);
            ui.say(ui.t("Nada que limpiar.", "Nothing to clean."));
        }
        return Exit::Ok;
    }
    if !ui.json {
        print_plan(ui, &preview.plan, &preview.dry_run);
    }
    let question = format!(
        "{} {} {} ({})?",
        match action {
            Action::Trash => ui.t("¿Mover a la Papelera", "Move to the Trash"),
            Action::Delete => ui.t("¿BORRAR PARA SIEMPRE", "DELETE FOREVER"),
        },
        preview.plan.totals().items,
        ui.t("elementos", "items"),
        size(preview.plan.totals().bytes_allocated, ui.lang)
    );
    if let Err(code) = ui.confirm("clean", &question) {
        return code;
    }
    let hash = preview.plan.plan_hash().to_owned();
    let plan_json = serde_json::to_value(&preview.plan).unwrap_or(Value::Null);
    let report = match core.execute(preview.plan, &hash, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error("clean", &e),
    };
    if ui.json {
        ui.emit(
            "clean",
            json!({ "dry_run": false, "plan": plan_json, "report": report }),
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
        match action {
            Action::Trash => ui.say(format!(
                "{} {} {} ({}). {}",
                ui.t("Movido a la Papelera:", "Moved to the Trash:"),
                report.done,
                ui.t("elementos", "items"),
                size(report.bytes_done, ui.lang),
                ui.t(
                    "Aún no se ha liberado espacio: eso ocurre al vaciar la Papelera.",
                    "No space is freed yet: that happens when the Trash is emptied.",
                ),
            )),
            Action::Delete => ui.say(format!(
                "{} {} {} ({}).",
                ui.t("Borrado permanentemente:", "Permanently deleted:"),
                report.done,
                ui.t("elementos", "items"),
                size(report.bytes_done, ui.lang),
            )),
        }
        if let Some(op) = &report.op_id
            && action == Action::Trash
        {
            ui.say(format!(
                "{} rotten undo {op}",
                ui.t("Para deshacer:", "To undo:")
            ));
        }
    }
    exec_exit(&report)
}

fn history(ui: &Ui, core: &Core) -> Exit {
    let (ops, problems) = match core.history() {
        Ok(h) => h,
        Err(e) => return ui.core_error("history", &e),
    };
    if ui.json {
        ui.emit(
            "history",
            json!({ "operations": ops, "journal_problems": problems }),
        );
        return Exit::Ok;
    }
    if ops.is_empty() {
        ui.say(ui.t("Aún no hay operaciones.", "No operations yet."));
    }
    for o in &ops {
        let state = match o.undo_state {
            UndoState::Restorable => ui.t("se puede deshacer", "can be undone"),
            UndoState::Partial => ui.t("se puede deshacer en parte", "can be partly undone"),
            UndoState::Irreversible => ui.t("ya no es reversible", "no longer reversible"),
            UndoState::Restored => ui.t("restaurada", "restored"),
            UndoState::PartlyRestored => ui.t("restaurada en parte", "partly restored"),
            UndoState::NotApplicable => "—",
        };
        let kind = match o.undo_of {
            Some(ref of) => format!("{} {of}", ui.t("deshacer", "undo")),
            None => ui.t("limpieza", "clean").to_owned(),
        };
        ui.say(format!(
            "{}  {}  {kind:<28} {:>4} {}  {:>9}  {state}",
            o.op_id,
            o.started_at,
            o.items_done,
            ui.t("elem.", "items"),
            size(o.bytes_done, ui.lang),
        ));
    }
    for p in &problems {
        ui.say(format!("! journal: {p}"));
    }
    Exit::Ok
}

fn undo(ui: &Ui, core: &Core, op: Option<String>, items: Vec<u32>, dry_run: bool) -> Exit {
    let items = if items.is_empty() { None } else { Some(items) };
    let preview = match core.prepare_undo(op.as_deref(), items.as_deref()) {
        Ok(p) => p,
        Err(e) => return ui.core_error("undo", &e),
    };
    let home = &core.env().home;
    let not_recoverable: Vec<Value> = preview
        .plan
        .not_recoverable
        .iter()
        .map(|(id, why)| json!({ "item_id": id, "reason": why }))
        .collect();
    if !ui.json {
        ui.say(format!(
            "{} {} · {} {}",
            ui.t("Deshacer", "Undo"),
            preview.plan.undo_of,
            preview.plan.ready.len(),
            ui.t("elementos para restaurar", "items to restore"),
        ));
        for u in &preview.plan.ready {
            ui.say(format!("  [{}] {}", u.item_id, tilde(&u.to, home)));
        }
        for (id, why) in &preview.plan.not_recoverable {
            ui.say(format!(
                "  [{id}] {} — {why}",
                ui.t("no recuperable", "not recoverable")
            ));
        }
    }
    if dry_run {
        if ui.json {
            ui.emit(
                "undo",
                json!({ "dry_run": true, "undo_of": preview.plan.undo_of, "plan": preview.plan.ready, "not_recoverable": not_recoverable, "preview": preview.dry_run }),
            );
        } else {
            ui.say(ui.t(
                "Simulación: no se ha movido nada.",
                "Dry run: nothing was moved.",
            ));
        }
        return Exit::Ok;
    }
    let partial_upfront = !preview.plan.not_recoverable.is_empty();
    if preview.plan.ready.is_empty() {
        if ui.json {
            ui.emit(
                "undo",
                json!({ "dry_run": false, "undo_of": preview.plan.undo_of, "not_recoverable": not_recoverable, "report": ExecReport::default() }),
            );
        } else {
            ui.say(ui.t("Nada que restaurar.", "Nothing to restore."));
        }
        return if partial_upfront {
            Exit::Partial
        } else {
            Exit::Ok
        };
    }
    let question = format!(
        "{} {} {}?",
        ui.t("¿Restaurar", "Restore"),
        preview.plan.ready.len(),
        ui.t("elementos", "items")
    );
    if let Err(code) = ui.confirm("undo", &question) {
        return code;
    }
    let report = match core.undo(&preview.plan, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error("undo", &e),
    };
    if ui.json {
        ui.emit(
            "undo",
            json!({ "dry_run": false, "undo_of": preview.plan.undo_of, "not_recoverable": not_recoverable, "report": report }),
        );
    } else {
        for r in &report.items {
            match &r.outcome {
                Outcome::Done {
                    result_path: Some(p),
                    ..
                } if p != &r.path => ui.say(format!(
                    "  [{}] {} {}",
                    r.item_id,
                    ui.t("restaurado como", "restored as"),
                    tilde(p, home)
                )),
                Outcome::Skipped { message, .. } | Outcome::Failed { error: message } => {
                    ui.say(format!("  ! [{}] {message}", r.item_id));
                }
                _ => {}
            }
        }
        ui.say(format!(
            "{} {} {}.",
            ui.t("Restaurados:", "Restored:"),
            report.done,
            ui.t("elementos", "items")
        ));
    }
    if partial_upfront {
        Exit::Partial
    } else {
        exec_exit(&report)
    }
}

fn doctor(ui: &Ui, core: &Core) -> Exit {
    let r: DoctorReport = core.doctor();
    let code = if r.config == "corrupt" {
        Exit::Safety
    } else if r.full_disk_access == FdaState::Denied {
        Exit::Permission
    } else {
        Exit::Ok
    };
    if ui.json {
        ui.emit("doctor", json!({ "report": r, "exit_code": code as u8 }));
        return code;
    }
    let yes_no = |b: bool| {
        if b {
            ui.t("sí", "yes")
        } else {
            ui.t("no", "no")
        }
    };
    let fda = match r.full_disk_access {
        FdaState::Granted => ui.t("concedido", "granted"),
        FdaState::Denied => ui.t(
            "NO concedido (concédelo a tu terminal en Ajustes del Sistema › Privacidad y seguridad)",
            "NOT granted (grant it to your terminal in System Settings › Privacy & Security)",
        ),
        FdaState::Undetermined => ui.t("no determinable", "undetermined"),
    };
    ui.say(format!(
        "rotten {} · {} {}",
        r.version,
        ui.t("catálogo", "catalog"),
        r.catalog_version
    ));
    ui.say(format!(
        "macOS {} · {} · {} {}",
        r.macos_version.as_deref().unwrap_or("?"),
        r.arch,
        ui.t("plataforma soportada (D-01):", "supported platform (D-01):"),
        yes_no(r.platform_supported)
    ));
    ui.say(format!(
        "{} {fda}",
        ui.t("Acceso total al disco:", "Full Disk Access:")
    ));
    ui.say(format!(
        "{} {} ({})",
        ui.t("Carpeta personal:", "Home:"),
        r.home.display(),
        r.home_source
    ));
    ui.say(format!(
        "{} {}",
        ui.t("Estado de la app:", "App state:"),
        r.state_dir.display()
    ));
    ui.say(format!(
        "{} {} ({}{})",
        ui.t("Configuración:", "Config:"),
        r.config_path.display(),
        r.config,
        r.config_error
            .as_deref()
            .map(|e| format!(": {e}"))
            .unwrap_or_default()
    ));
    ui.say(format!(
        "Journal: {} ({} {}, {} {})",
        r.journal_dir.display(),
        r.journal_records,
        ui.t("registros", "records"),
        r.journal_problems.len(),
        ui.t("líneas ilegibles", "unreadable lines")
    ));
    let trash = match &r.trash {
        TrashInfo::System => ui.t("la del sistema", "the system one").to_owned(),
        TrashInfo::Dir { path } => path.display().to_string(),
    };
    ui.say(format!("{} {trash}", ui.t("Papelera:", "Trash:")));
    if !r.trash_unreadable_ops.is_empty() {
        ui.say(format!(
            "  ! {} {}",
            ui.t(
                "Operaciones interrumpidas con elementos que no se pudieron buscar en la Papelera (falta Acceso total al disco):",
                "Interrupted operations with items that could not be looked up in the Trash (Full Disk Access missing):"
            ),
            r.trash_unreadable_ops.join(", ")
        ));
    }
    ui.say(format!(
        "{} {}",
        ui.t("Cerrojo de ejecución libre:", "Execution lock free:"),
        yes_no(r.exec_lock_free)
    ));
    match &r.running_apps {
        AppsProbe::Ok { count } => ui.say(format!(
            "{} {count}",
            ui.t("Apps abiertas detectadas:", "Running apps detected:")
        )),
        AppsProbe::Unavailable { detail: e } => ui.say(format!(
            "{} {e}",
            ui.t(
                "No se pueden detectar las apps abiertas:",
                "Cannot detect running apps:"
            )
        )),
    }
    code
}

fn exclude(ui: &Ui, core: &Core, action: ExcludeCmd) -> Exit {
    let (raw, add, dry_run) = match action {
        ExcludeCmd::List => {
            return match core.exclusions() {
                Ok(list) => {
                    if ui.json {
                        let items: Vec<String> = list.iter().map(ToString::to_string).collect();
                        ui.emit("exclude", json!({ "action": "list", "exclusions": items }));
                    } else if list.is_empty() {
                        ui.say(ui.t("No hay exclusiones.", "No exclusions."));
                    } else {
                        for e in list {
                            ui.say(e.to_string());
                        }
                    }
                    Exit::Ok
                }
                Err(e) => ui.core_error("exclude", &e),
            };
        }
        ExcludeCmd::Add { value, dry_run } => (value, true, dry_run),
        ExcludeCmd::Remove { value, dry_run } => (value, false, dry_run),
    };
    match core.edit_exclusion(&raw, add, dry_run) {
        Ok((ex, changed)) => {
            if ui.json {
                ui.emit(
                    "exclude",
                    json!({ "action": if add { "add" } else { "remove" }, "exclusion": ex.to_string(), "changed": changed, "dry_run": dry_run }),
                );
            } else {
                let msg = match (add, changed) {
                    (true, true) => ui.t("Añadida:", "Added:"),
                    (true, false) => ui.t("Ya estaba:", "Already present:"),
                    (false, true) => ui.t("Quitada:", "Removed:"),
                    (false, false) => ui.t("No estaba:", "Not present:"),
                };
                let suffix = if dry_run && changed {
                    ui.t(" (simulación)", " (dry run)")
                } else {
                    ""
                };
                ui.say(format!("{msg} {ex}{suffix}"));
            }
            Exit::Ok
        }
        Err(e) => ui.core_error("exclude", &e),
    }
}
