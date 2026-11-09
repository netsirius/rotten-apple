//! `rotten`: CLI de Rotten Apple sobre `ra-core` (07).
//!
//! Códigos de salida (README §8, 07 §5): `0` ok · `1` error · `2` uso o
//! falta `--yes` · `3` falta permiso · `4` completado con omisiones · `5`
//! abortado por seguridad o plan cambiado · `6` otro ejecutor activo.
//!
//! Salida: JSON con `--json` o si stdout no es una terminal; si no, texto
//! en el idioma del usuario (`i18n`). `ROTTEN_HUMAN=1` fuerza el texto
//! aunque stdout no sea una terminal (`rotten scan | less`, pruebas).
//!
//! Colores: solo en una terminal, sin `--json`, `--no-color` ni `NO_COLOR`
//! (`style`). Log técnico local y rotado en `~/Library/Logs/<bundle-id>/`
//! (`techlog`); `--debug` lo detalla y lo copia a stderr.

#![forbid(unsafe_code)]

mod args;
mod cmd;
mod i18n;
mod render;
mod style;
mod techlog;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::FromArgMatches;
use fluent_bundle::FluentArgs;
use ra_core::{
    Action, AppsProbe, CategorySummary, ConfigState, Core, CoreError, DoctorReport, Env, EnvError,
    ErrorClass, ExecReport, FdaState, HistoryKind, Outcome, Plan, Risk, ScanStatus, Selection,
    TrashInfo, UndoState, tilde,
};
use serde_json::{Value, json};

use args::{Cli, Cmd, ExcludeCmd};
use i18n::{I18n, Lang, tr};
use style::Style;

pub const SCHEMA_VERSION: u32 = 2;

/// Fuerza la salida para personas aunque stdout no sea una terminal.
const HUMAN_ENV: &str = "ROTTEN_HUMAN";

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
    i18n: I18n,
    /// Carpeta personal, para abreviar rutas con `~` (vacía hasta saberla).
    home: PathBuf,
    /// Colores en stdout y en stderr (`style::enabled`).
    color_out: bool,
    color_err: bool,
}

impl Ui {
    fn tr(&self, id: &str, args: Option<&FluentArgs>) -> String {
        self.i18n.tr(id, args)
    }

    fn size(&self, bytes: u64) -> String {
        self.i18n.size(bytes)
    }

    fn path(&self, p: &Path) -> String {
        tilde(p, &self.home)
    }

    fn say(&self, line: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", line.as_ref());
        }
    }

    /// Una línea con sangría (`level` niveles de dos espacios).
    fn say_in(&self, level: usize, line: impl AsRef<str>) {
        self.say(format!("{}{}", "  ".repeat(level), line.as_ref()));
    }

    /// `text` con color si stdout lo admite.
    fn paint(&self, style: Style, text: impl AsRef<str>) -> String {
        style::paint(self.color_out, style, text.as_ref())
    }

    fn emit(&self, command: &str, body: Value) {
        let mut doc = json!({ "schema_version": SCHEMA_VERSION, "command": command });
        if let (Value::Object(d), Value::Object(b)) = (&mut doc, body) {
            d.extend(b);
        }
        let text = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into());
        println!("{text}");
    }

    /// Error común: en JSON, un código estable y el mensaje en inglés (no
    /// dependen del idioma); para personas, `human` en su idioma.
    fn fail(&self, command: &str, exit: Exit, code: &str, message: &str, human: &str) -> Exit {
        self.fail_detail(command, exit, code, message, None, human)
    }

    /// Como [`Ui::fail`], con un detalle opcional para el JSON
    /// (`error.detail`) que no va en el mensaje ni en el log técnico.
    fn fail_detail(
        &self,
        command: &str,
        exit: Exit,
        code: &str,
        message: &str,
        detail: Option<&str>,
        human: &str,
    ) -> Exit {
        techlog::error(command, code, exit as u8, message);
        if self.json {
            let mut error = json!({ "code": code, "exit_code": exit as u8, "message": message });
            if let Some(d) = detail {
                error["detail"] = json!(d);
            }
            self.emit(command, json!({ "error": error }));
        } else {
            let line = tr!(self, "error-line", message = human);
            eprintln!("{}", style::paint(self.color_err, Style::Red, &line));
        }
        exit
    }

    /// Error propio del CLI: el mensaje `id` del catálogo, en inglés en el
    /// JSON y en el idioma del usuario en la terminal.
    fn fail_with(&self, command: &str, exit: Exit, code: &str, id: &str) -> Exit {
        let english = I18n::with(Lang::En, false).tr(id, None);
        self.fail(command, exit, code, &english, &self.tr(id, None))
    }

    fn core_error(&self, command: &str, e: &CoreError) -> Exit {
        let exit = match e.class() {
            ErrorClass::Usage => Exit::Usage,
            ErrorClass::Permission => Exit::Permission,
            ErrorClass::Safety => Exit::Safety,
            ErrorClass::Locked => Exit::Locked,
            ErrorClass::NotFound | ErrorClass::Other => Exit::Error,
        };
        let human = render::core_error(&self.i18n, e, &self.home);
        let detail = e.detail();
        self.fail_detail(
            command,
            exit,
            e.code(),
            &e.to_string(),
            detail.as_deref(),
            &human,
        )
    }

    /// Confirmación: `--yes`, o una pregunta si hay terminal; si no, error
    /// de uso (07 §1.4).
    fn confirm(&self, command: &str, question: &str) -> Result<(), Exit> {
        if self.yes {
            return Ok(());
        }
        if !self.interactive {
            return Err(self.fail_with(command, Exit::Usage, "needs_yes", "needs-yes"));
        }
        print!("{} ", tr!(self, "confirm-prompt", question = question));
        let _ = io::stdout().flush();
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
        let a = line.trim().to_lowercase();
        if ["s", "si", "sí", "y", "yes"].contains(&a.as_str()) {
            Ok(())
        } else {
            self.say(tr!(self, "cancelled"));
            Err(Exit::Ok)
        }
    }
}

fn main() -> ExitCode {
    // El idioma, antes de analizar los argumentos: la ayuda ya sale en él.
    let argv: Vec<OsString> = std::env::args_os().collect();
    let lossy: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let lang = Lang::detect(args::lang_flag(&lossy).as_deref());
    let i18n = I18n::new(lang);
    let no_color_env = style::no_color_env();
    let mut definition = args::localized(&i18n);
    if args::no_color_flag(&lossy) || no_color_env.as_deref().is_some_and(|v| !v.is_empty()) {
        definition = definition.color(clap::ColorChoice::Never);
    }
    let matches = definition.get_matches_from(argv);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    // Antes que nada más: no necesita la carpeta personal ni el núcleo, y
    // su salida es el script aunque vaya a una tubería.
    if let Cmd::Completion { shell } = cli.cmd {
        return cmd::completion::run(shell, &i18n).into();
    }

    let started = Instant::now();
    techlog::init(cli.debug);
    let piped = !io::stdout().is_terminal();
    let force_human = std::env::var_os(HUMAN_ENV).is_some_and(|v| v == "1");
    let json = cli.json || (piped && !force_human);
    let colors =
        |terminal: bool| style::enabled(terminal, json, cli.no_color, no_color_env.as_deref());
    let ui = Ui {
        json,
        interactive: !json && io::stdin().is_terminal(),
        yes: cli.yes,
        quiet: cli.quiet,
        i18n,
        home: PathBuf::new(),
        color_out: colors(!piped),
        color_err: colors(io::stderr().is_terminal()),
    };
    let command = match &cli.cmd {
        Cmd::Scan { .. } => "scan",
        Cmd::Clean { .. } => "clean",
        Cmd::History => "history",
        Cmd::Undo { .. } => "undo",
        Cmd::Free { .. } => "free",
        Cmd::Doctor => "doctor",
        Cmd::Exclude { .. } => "exclude",
        Cmd::Trash { .. } => "trash",
        Cmd::Config { .. } => "config",
        Cmd::Update { .. } => "update",
        Cmd::Completion { .. } => "completion",
    };
    techlog::start(command);
    let exit = run(ui, cli, command);
    techlog::finish(command, exit as u8, started);
    exit.into()
}

fn run(mut ui: Ui, cli: Cli, command: &str) -> Exit {
    if ra_core::running_as_root() && !cli.allow_root {
        return ui.fail_with(command, Exit::Safety, "running_as_root", "running-as-root");
    }

    let env = match Env::from_env() {
        Ok(e) => e,
        Err(e) => {
            let code = match e {
                EnvError::NoHome => "no_home",
                EnvError::BadPath { .. } => "invalid_env_path",
            };
            let human = render::env_error(&ui.i18n, &e);
            return ui.fail(command, Exit::Error, code, &e.to_string(), &human);
        }
    };
    ui.home = env.home.clone();
    // RF-AJU-07: sin `--lang`, el idioma de la configuración manda sobre
    // `LANG` (la ayuda de clap ya salió con `LANG`: se genera antes).
    if cli.lang.is_none()
        && let Some(lang) = cmd::config::configured_lang(&env)
    {
        ui.i18n = I18n::new(lang);
    }
    techlog::set_home(&env.home);
    techlog::env(&env, ui.i18n.lang().code(), ui.json);
    // `update` no usa el estado de la app (T-15).
    if let Cmd::Update { dry_run } = cli.cmd {
        return cmd::update::run(&ui, dry_run);
    }
    let core = match Core::new(env) {
        Ok(c) => c,
        Err(e) => return ui.core_error(command, &e),
    };
    match cli.cmd {
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
        Cmd::Free { op_ids, dry_run } => cmd::free::run(&ui, &core, &op_ids, dry_run),
        Cmd::Doctor => doctor(&ui, &core),
        Cmd::Exclude { action } => exclude(&ui, &core, action),
        Cmd::Trash { action } => cmd::trash::run(&ui, &core, action),
        Cmd::Config { action } => cmd::config::run(&ui, &core, action),
        // Ya despachados: `completion` en `main` y `update` antes del núcleo.
        Cmd::Completion { .. } => Exit::Ok,
        Cmd::Update { .. } => unreachable!("se despacha antes de abrir el núcleo"),
    }
}

fn opt_categories(c: Vec<String>) -> Option<Vec<String>> {
    if c.is_empty() { None } else { Some(c) }
}

/// Lo que se liberaría de verdad al vaciar la Papelera: la cifra principal
/// de `scan`. No cuenta lo atrapado en clones APFS o instantáneas ni dos
/// veces los enlaces duros (T-05, RF-SEG-12).
fn reclaimable(c: &CategorySummary) -> u64 {
    c.reclaimable_bytes
}

/// Lo mismo para lo preseleccionado.
fn preselected_reclaimable(c: &CategorySummary) -> u64 {
    c.preselected_reclaimable_bytes
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
        return Exit::Ok;
    }
    let status = match report.scan_status {
        ScanStatus::Complete => "complete",
        ScanStatus::Partial => "partial",
        ScanStatus::Unavailable => "unavailable",
    };
    ui.say(ui.paint(
        Style::Bold,
        tr!(
            ui,
            "scan-header",
            version = report.catalog_version.as_str(),
            status = status
        ),
    ));
    // Estado del volumen (RF-PAN-01): «disponible» es la cifra de Finder.
    if let Some(v) = &report.volume {
        let line = match (v.available_bytes, v.purgeable_bytes) {
            (Some(available), Some(purgeable)) => tr!(
                ui,
                "scan-volume",
                used = ui.size(v.used_bytes),
                capacity = ui.size(v.capacity_bytes),
                free = ui.size(v.free_bytes),
                purgeable = ui.size(purgeable),
                available = ui.size(available),
            ),
            _ => tr!(
                ui,
                "scan-volume-basic",
                used = ui.size(v.used_bytes),
                capacity = ui.size(v.capacity_bytes),
                free = ui.size(v.free_bytes),
            ),
        };
        ui.say_in(1, line);
    }
    let (mut pre_items, mut pre_bytes) = (0, 0);
    for c in &report.categories {
        let title = match ui.i18n.lang() {
            Lang::Es => &c.title_es,
            Lang::En => &c.title_en,
        };
        ui.say_in(
            1,
            tr!(
                ui,
                "scan-category",
                id = c.id.as_str(),
                title = format!("{title:<32}"),
                reclaimable = ui.size(reclaimable(c)),
                count = c.items,
                preselected = ui.size(preselected_reclaimable(c)),
            ),
        );
        for n in &c.notes {
            ui.say_in(3, tr!(ui, "scan-note", note = n.as_str()));
        }
        pre_items += c.preselected_items;
        pre_bytes += preselected_reclaimable(c);
    }
    // El aviso del informe (`warnings`) es el del JSON; aquí, traducido.
    if let ConfigState::Corrupt { path, reason } = core.config_state() {
        ui.say(tr!(
            ui,
            "scan-config-corrupt",
            path = ui.path(&path),
            reason = reason
        ));
    }
    cmd::config::scan_notes(ui, core);
    ui.say(ui.paint(
        Style::Green,
        tr!(
            ui,
            "scan-summary",
            reclaimable = ui.size(pre_bytes),
            count = pre_items
        ),
    ));
    Exit::Ok
}

fn risk_code(r: Risk) -> &'static str {
    match r {
        Risk::Safe => "safe",
        Risk::Caution => "caution",
        Risk::Review => "review",
    }
}

fn print_plan(ui: &Ui, core: &Core, plan: &Plan, preview: &ExecReport) {
    let home = plan.home();
    let hash = &plan.plan_hash()[..plan.plan_hash().len().min(19)];
    let action = match plan.action() {
        Action::Trash => "trash",
        Action::Delete => "delete",
    };
    ui.say(ui.paint(
        Style::Bold,
        tr!(
            ui,
            "plan-header",
            hash = hash,
            count = plan.totals().items,
            size = ui.size(plan.totals().bytes_allocated),
            action = action
        ),
    ));
    for (item, r) in plan.items().iter().zip(&preview.items) {
        let id = item.item_id.to_string();
        let path = tilde(&item.path, home);
        let size = ui.size(item.bytes_allocated);
        let line = match &r.outcome {
            Outcome::Skipped { cause, .. } => tr!(
                ui,
                "plan-item-would-skip",
                id = id,
                path = path,
                category = item.category.as_str(),
                size = size,
                reason = render::skip_cause(&ui.i18n, cause, home)
            ),
            _ => tr!(
                ui,
                "plan-item",
                id = id,
                path = path,
                category = item.category.as_str(),
                size = size
            ),
        };
        ui.say_in(1, line);
    }
    if !plan.skipped().is_empty() {
        ui.say(tr!(ui, "plan-skipped", count = plan.skipped().len()));
        for s in plan.skipped() {
            ui.say_in(
                1,
                tr!(
                    ui,
                    "plan-skipped-item",
                    path = tilde(&s.path, home),
                    reason = render::skip_reason(&ui.i18n, &s.reason, home)
                ),
            );
        }
    }
    for (cat, ns) in plan.not_selected() {
        ui.say(tr!(
            ui,
            "plan-not-selected",
            category = cat.as_str(),
            count = ns.items,
            size = ui.size(ns.bytes_allocated)
        ));
    }
    // RF-LIM-04: qué es cada cosa del plan, qué pasa si se borra, si se
    // regenera y por qué tiene ese riesgo. Una vez por regla.
    let lang = ui.i18n.lang().code();
    let mut seen = BTreeSet::new();
    let rules: Vec<_> = plan
        .items()
        .iter()
        .filter_map(|i| core.catalog().rule(&i.rule.id))
        .filter(|r| seen.insert(r.id.clone()))
        .collect();
    if !rules.is_empty() {
        ui.say(tr!(ui, "plan-rules"));
    }
    for r in rules {
        ui.say_in(
            1,
            tr!(
                ui,
                "plan-rule",
                title = r.title.get(lang),
                risk = risk_code(r.risk)
            ),
        );
        ui.say_in(2, tr!(ui, "plan-rule-what", text = r.explain.get(lang)));
        ui.say_in(
            2,
            tr!(ui, "plan-rule-if-deleted", text = r.if_deleted.get(lang)),
        );
        ui.say_in(
            2,
            tr!(ui, "plan-rule-regenerates", text = r.regenerates.get(lang)),
        );
        ui.say_in(
            2,
            tr!(ui, "plan-rule-why-risk", text = r.why_risk.get(lang)),
        );
    }
}

/// Los elementos omitidos o fallidos de un informe de ejecución.
fn print_problems(ui: &Ui, report: &ExecReport) {
    for r in &report.items {
        if let Some(problem) = render::problem(&ui.i18n, &r.outcome, &ui.home) {
            ui.say_in(
                1,
                ui.paint(
                    Style::Yellow,
                    tr!(
                        ui,
                        "item-problem",
                        path = ui.path(&r.path),
                        problem = problem
                    ),
                ),
            );
        }
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
        return ui.fail_with(
            "clean",
            Exit::Usage,
            "permanent_needs_yes",
            "permanent-needs-yes",
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
                json!({ "dry_run": true, "plan": preview.plan, "report": preview.dry_run }),
            );
        } else {
            print_plan(ui, core, &preview.plan, &preview.dry_run);
            ui.say(tr!(ui, "dry-run-nothing-changed"));
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
            print_plan(ui, core, &preview.plan, &preview.dry_run);
            ui.say(tr!(ui, "clean-nothing"));
        }
        return Exit::Ok;
    }
    if !ui.json {
        print_plan(ui, core, &preview.plan, &preview.dry_run);
    }
    let count = preview.plan.totals().items;
    let size = ui.size(preview.plan.totals().bytes_allocated);
    let question = match action {
        Action::Trash => tr!(ui, "confirm-trash", count = count, size = size.as_str()),
        Action::Delete => tr!(ui, "confirm-delete", count = count, size = size.as_str()),
    };
    if let Err(code) = ui.confirm("clean", &question) {
        return code;
    }
    let hash = preview.plan.plan_hash().to_owned();
    let plan_json = serde_json::to_value(&preview.plan).unwrap_or(Value::Null);
    let report = match core.execute(preview.plan, &hash, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error("clean", &e),
    };
    techlog::report("clean", &report);
    if ui.json {
        ui.emit(
            "clean",
            json!({ "dry_run": false, "plan": plan_json, "report": report }),
        );
    } else {
        print_problems(ui, &report);
        let size = ui.size(report.bytes_done);
        match action {
            Action::Trash => {
                ui.say(ui.paint(
                    Style::Green,
                    tr!(ui, "moved-to-trash", count = report.done, size = size),
                ));
                ui.say(tr!(ui, "not-freed-yet"));
            }
            Action::Delete => ui.say(tr!(
                ui,
                "deleted-permanently",
                count = report.done,
                size = size
            )),
        }
        if let Some(op) = &report.op_id
            && action == Action::Trash
            && report.done > 0
        {
            ui.say(tr!(ui, "to-undo", op = op.as_str()));
            // «Liberar ahora» (CU-08): solo los elementos de esta operación.
            ui.say(tr!(ui, "to-free", op = op.as_str()));
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
        ui.say(tr!(ui, "history-empty"));
    }
    for o in &ops {
        let state = match o.undo_state {
            UndoState::Restorable => "restorable",
            UndoState::Partial => "partial",
            UndoState::Irreversible => "irreversible",
            UndoState::Restored => "restored",
            UndoState::PartlyRestored => "partly_restored",
            UndoState::NotApplicable => "not_applicable",
        };
        let kind = match o.kind {
            HistoryKind::Clean => "clean",
            HistoryKind::TrashPut => "trash_put",
            HistoryKind::Undo => "undo",
            HistoryKind::Free => "free",
            HistoryKind::TrashEmpty => "trash_empty",
        };
        ui.say(tr!(
            ui,
            "history-op",
            op = o.op_id.as_str(),
            date = ui.i18n.date(&o.started_at),
            kind = kind,
            of = o.undo_of.as_deref().unwrap_or_default(),
            count = o.items_done,
            size = ui.size(o.bytes_done),
            state = state
        ));
    }
    for p in &problems {
        ui.say(tr!(ui, "history-journal-problem", problem = p.as_str()));
    }
    Exit::Ok
}

fn undo(ui: &Ui, core: &Core, op: Option<String>, items: Vec<u32>, dry_run: bool) -> Exit {
    let items = if items.is_empty() { None } else { Some(items) };
    let preview = match core.prepare_undo(op.as_deref(), items.as_deref()) {
        Ok(p) => p,
        Err(e) => return ui.core_error("undo", &e),
    };
    if !ui.json {
        ui.say(tr!(
            ui,
            "undo-header",
            op = preview.plan.undo_of.as_str(),
            count = preview.plan.ready.len()
        ));
        for u in &preview.plan.ready {
            ui.say_in(
                1,
                tr!(
                    ui,
                    "undo-ready",
                    id = u.item_id.to_string(),
                    path = ui.path(&u.to)
                ),
            );
        }
        for n in &preview.plan.not_recoverable {
            ui.say_in(
                1,
                tr!(
                    ui,
                    "undo-not-recoverable",
                    id = n.item_id.to_string(),
                    reason = render::not_recoverable(&ui.i18n, &n.reason)
                ),
            );
        }
    }
    if dry_run {
        if ui.json {
            ui.emit(
                "undo",
                json!({ "dry_run": true, "plan": preview.plan, "report": preview.dry_run }),
            );
        } else {
            ui.say(tr!(ui, "dry-run-nothing-moved"));
        }
        return Exit::Ok;
    }
    // Lo ya restaurado no deja nada pendiente; lo demás que no se puede
    // restaurar, sí (4).
    let partial_upfront = preview
        .plan
        .not_recoverable
        .iter()
        .any(|n| n.reason != ra_core::NotRecoverableReason::AlreadyRestored);
    if preview.plan.ready.is_empty() {
        if ui.json {
            ui.emit(
                "undo",
                json!({ "dry_run": false, "plan": preview.plan, "report": ExecReport::default() }),
            );
        } else {
            ui.say(tr!(ui, "undo-nothing"));
        }
        return if partial_upfront {
            Exit::Partial
        } else {
            Exit::Ok
        };
    }
    let question = tr!(ui, "undo-confirm", count = preview.plan.ready.len());
    if let Err(code) = ui.confirm("undo", &question) {
        return code;
    }
    let report = match core.undo(&preview.plan, "cli") {
        Ok(r) => r,
        Err(e) => return ui.core_error("undo", &e),
    };
    techlog::report("undo", &report);
    if ui.json {
        ui.emit(
            "undo",
            json!({ "dry_run": false, "plan": preview.plan, "report": report }),
        );
    } else {
        for r in &report.items {
            let id = r.item_id.to_string();
            match &r.outcome {
                Outcome::Done {
                    result_path: Some(p),
                    ..
                } if p != &r.path => {
                    ui.say_in(1, tr!(ui, "undo-restored-as", id = id, path = ui.path(p)));
                }
                o => {
                    if let Some(problem) = render::problem(&ui.i18n, o, &ui.home) {
                        ui.say_in(1, tr!(ui, "undo-item-problem", id = id, problem = problem));
                    }
                }
            }
        }
        ui.say(ui.paint(Style::Green, tr!(ui, "undo-done", count = report.done)));
    }
    if partial_upfront {
        Exit::Partial
    } else {
        exec_exit(&report)
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn doctor(ui: &Ui, core: &Core) -> Exit {
    let r: DoctorReport = core.doctor();
    // Con la configuración corrupta u otra cosa en la ruta de su enlace, no
    // se puede limpiar (RF-SEG-05 c): el mismo veredicto aquí.
    let link_conflict = matches!(r.config_link.state, ra_core::LinkState::Conflict { .. });
    let code = if r.config == "corrupt" || link_conflict {
        Exit::Safety
    } else if r.full_disk_access == FdaState::Denied {
        Exit::Permission
    } else {
        Exit::Ok
    };
    if ui.json {
        // El veredicto está en `report` (`config`, `full_disk_access`) y en
        // el código de salida; `exit_code` solo va en el documento de error.
        ui.emit("doctor", json!({ "report": r }));
        return code;
    }
    let fda = match r.full_disk_access {
        FdaState::Granted => "granted",
        FdaState::Denied => "denied",
        FdaState::Undetermined => "undetermined",
    };
    ui.say(tr!(
        ui,
        "doctor-version",
        version = r.version.as_str(),
        catalog = r.catalog_version.as_str()
    ));
    ui.say(tr!(
        ui,
        "doctor-platform",
        macos = r.macos_version.as_deref().unwrap_or("?"),
        arch = r.arch.as_str(),
        supported = yes_no(r.platform_supported)
    ));
    ui.say(tr!(ui, "doctor-fda", state = fda));
    ui.say(tr!(
        ui,
        "doctor-home",
        path = r.home.display().to_string(),
        source = r.home_source.as_str()
    ));
    ui.say(tr!(
        ui,
        "doctor-state",
        path = r.state_dir.display().to_string()
    ));
    let config_path = r.config_path.display().to_string();
    match &r.config_error {
        Some(e) => ui.say(tr!(
            ui,
            "doctor-config-error",
            path = config_path,
            error = e.as_str()
        )),
        None => ui.say(tr!(
            ui,
            "doctor-config",
            path = config_path,
            state = r.config.as_str()
        )),
    }
    ui.say(cmd::config::link_line(ui, &r.config_link, &r.config_path));
    if !r.config_unknown_keys.is_empty() {
        ui.say(cmd::config::unknown_keys_line(
            ui,
            &r.config_path,
            &r.config_unknown_keys,
        ));
    }
    ui.say(tr!(
        ui,
        "doctor-journal",
        path = r.journal_dir.display().to_string(),
        records = r.journal_records,
        problems = r.journal_problems.len()
    ));
    match &r.trash {
        TrashInfo::System => ui.say(tr!(ui, "doctor-trash-system")),
        TrashInfo::Dir { path } => ui.say(tr!(
            ui,
            "doctor-trash-dir",
            path = path.display().to_string()
        )),
    }
    if !r.trash_unreadable_ops.is_empty() {
        ui.say_in(
            1,
            tr!(
                ui,
                "doctor-trash-unreadable",
                ops = r.trash_unreadable_ops.join(", ")
            ),
        );
    }
    ui.say(tr!(ui, "doctor-lock", free = yes_no(r.exec_lock_free)));
    match &r.running_apps {
        AppsProbe::Ok { count } => ui.say(tr!(ui, "doctor-apps", count = *count)),
        AppsProbe::Unavailable { detail } => {
            ui.say(tr!(ui, "doctor-apps-unavailable", detail = detail.as_str()))
        }
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
                        ui.say(tr!(ui, "exclude-none"));
                    } else {
                        for e in list {
                            ui.say(tr!(ui, "exclude-item", value = e.to_string()));
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
                let change = match (add, changed) {
                    (true, true) => "added",
                    (true, false) => "present",
                    (false, true) => "removed",
                    (false, false) => "absent",
                };
                let value = ex.to_string();
                ui.say(if dry_run {
                    tr!(ui, "exclude-result-dry", change = change, value = value)
                } else {
                    tr!(ui, "exclude-result", change = change, value = value)
                });
            }
            Exit::Ok
        }
        Err(e) => ui.core_error("exclude", &e),
    }
}
