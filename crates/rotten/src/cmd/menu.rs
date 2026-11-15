//! `rotten` sin subcomando: el menú interactivo (T-16, 07 §2).
//!
//! **Cuándo.** Solo con terminal: stdin **y** stdout son una terminal y no
//! se ha pedido `--json` ([`bare`]). Sin terminal, `rotten` imprime la
//! ayuda por stderr (stdout queda vacío: es el canal del JSON) y sale con
//! `2`. Con `--json` y sin subcomando, el error JSON común
//! (`subcommand_required`, `2`; `schemas/menu.v2.schema.json`): el menú
//! no tiene salida JSON.
//!
//! **Opciones.** Solo comandos que ya existen, y cada una llama a la misma
//! función que su subcomando, sin argumentos: `scan`, `clean`, `undo`,
//! `free` de la última operación que aún tiene algo en la Papelera (la
//! misma que deshace `undo`), `history`, `trash`, `trash empty`, `exclude
//! list`, `doctor` y `config path`. Las que cambian algo (`clean`, `undo`,
//! `free`, `trash empty`) enseñan su plan y piden confirmación como el
//! subcomando. En el menú no se aplican `--yes` ni `--quiet` ([`flags`]):
//! nunca hay un «sí» implícito ni una confirmación sin el plan delante, y
//! [`run`] se niega a abrirse (`terminal_error`) si aun así le llegan, o
//! sin terminal. «Liberar ahora» dice antes del plan qué operación es (id,
//! fecha y tipo). Lo que necesita argumentos (`exclude add`, `config set`,
//! `trash put`) o no limpia (`update`, `completion`) se queda en la línea
//! de órdenes.
//!
//! **Teclas.** Flechas y `hjkl` recorren la lista (las cuatro direcciones,
//! en círculo), Intro elige, `u` deshace la última operación (lo mismo que
//! la opción «Deshacer») y `q`, Esc o Ctrl-C salen. Las letras valen
//! también en mayúsculas (con Bloq Mayús). En modo raw, Ctrl-C llega como
//! tecla y no como señal.
//!
//! **Terminal.** El menú se dibuja en la pantalla alternativa y en modo
//! raw. Antes de ejecutar una acción se sale de los dos ([`choose`] solo
//! devuelve la opción después de soltar la sesión), así que la acción
//! escribe y pregunta como en la línea de órdenes. Al terminar, su salida
//! queda a la vista hasta pulsar Intro (vuelve al menú) o `q`, Esc o
//! Ctrl-C (salen); las demás teclas no hacen nada. La terminal se restaura
//! siempre: con un guard que la restaura al soltarse ([`Session`]) y con
//! un *panic hook* que lo hace antes de imprimir el panic.
//!
//! **Teclas de antemano.** Lo tecleado antes de tiempo no elige ni
//! contesta nada: se descarta la cola de eventos de crossterm antes de
//! ejecutar la acción y antes de esperar tras ella ([`drain`]), y la
//! entrada pendiente de la terminal justo antes de cada confirmación
//! (`Ui::confirm`, `tcflush`).
//!
//! **Señales.** Con el menú abierto (la lista o la espera tras una acción),
//! SIGTERM y SIGHUP marcan una bandera que el bucle mira cada [`TICK`]; se
//! sale por el camino normal, que restaura la terminal. Durante una acción
//! hacen lo de siempre, como con el subcomando: terminan el proceso
//! ([`Signals`]).
//!
//! **Altura.** Si la terminal es baja, primero se quita la descripción y
//! luego la lista se desplaza para que la opción elegida siempre se vea
//! ([`draw`]).
//!
//! **Código de salida.** El de la última acción ejecutada (0 si no se
//! ejecutó ninguna): `rotten; echo $?` dice cómo acabó lo último que se
//! hizo.
//!
//! **Estructura.** [`Menu::on_key`] es una máquina de estados pura (tecla
//! → paso) y [`draw`] solo dibuja: los dos se prueban con
//! `ratatui::backend::TestBackend` y teclas sintéticas, sin pty.
//!
//! **Colores.** Con colores (`Ui::color_out`), el título en negrita y la
//! opción elegida en verde y negrita. Sin ellos (`--no-color`, `NO_COLOR`)
//! no hay ningún estilo: la opción elegida se ve por la marca `›`.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};
use ra_core::Core;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style as Look};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::args::{ConfigCmd, ExcludeCmd, TrashCmd};
use crate::i18n::{I18n, Lang, tr};
use crate::style::Style;
use crate::{Exit, Ui, cmd, history_kind, techlog};

/// Nombre del comando en el JSON de error y en el log técnico.
pub const COMMAND: &str = "menu";

/// Qué hacer con `rotten` sin subcomando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bare {
    /// El menú.
    Menu,
    /// La ayuda por stderr y `2`.
    Help,
    /// El error JSON y `2`.
    JsonError,
}

/// `--yes` y `--quiet` tal como llegan a las acciones: con subcomando, los
/// de la línea de órdenes; en el menú, ninguno de los dos. `--yes` sería
/// un «sí» implícito a todo lo que se elija, y `--quiet` escondería el plan
/// antes de la confirmación (y dejaría en blanco las opciones de lectura).
pub fn flags(yes: bool, quiet: bool, subcommand: bool) -> (bool, bool) {
    (yes && subcommand, quiet && subcommand)
}

/// `json_flag` es `--json` explícito: sin él, una tubería da la ayuda, no
/// JSON (el menú no tiene salida JSON que activar).
pub fn bare(json_flag: bool, stdin_tty: bool, stdout_tty: bool) -> Bare {
    if json_flag {
        Bare::JsonError
    } else if stdin_tty && stdout_tty {
        Bare::Menu
    } else {
        Bare::Help
    }
}

/// Una opción del menú.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Scan,
    Clean,
    Undo,
    Free,
    History,
    Trash,
    TrashEmpty,
    Exclusions,
    Doctor,
    Config,
    Quit,
}

/// Las opciones, en el orden en que salen.
pub const ITEMS: [Item; 11] = [
    Item::Scan,
    Item::Clean,
    Item::Undo,
    Item::Free,
    Item::History,
    Item::Trash,
    Item::TrashEmpty,
    Item::Exclusions,
    Item::Doctor,
    Item::Config,
    Item::Quit,
];

impl Item {
    /// Mensajes del catálogo: título y descripción.
    fn messages(self) -> (&'static str, &'static str) {
        match self {
            Item::Scan => ("menu-scan", "menu-scan-desc"),
            Item::Clean => ("menu-clean", "menu-clean-desc"),
            Item::Undo => ("menu-undo", "menu-undo-desc"),
            Item::Free => ("menu-free", "menu-free-desc"),
            Item::History => ("menu-history", "menu-history-desc"),
            Item::Trash => ("menu-trash", "menu-trash-desc"),
            Item::TrashEmpty => ("menu-trash-empty", "menu-trash-empty-desc"),
            Item::Exclusions => ("menu-exclude", "menu-exclude-desc"),
            Item::Doctor => ("menu-doctor", "menu-doctor-desc"),
            Item::Config => ("menu-config", "menu-config-desc"),
            Item::Quit => ("menu-quit", "menu-quit-desc"),
        }
    }

    /// El comando que ejecuta, para el log técnico.
    fn command(self) -> &'static str {
        match self {
            Item::Scan => "scan",
            Item::Clean => "clean",
            Item::Undo => "undo",
            Item::Free => "free",
            Item::History => "history",
            Item::Trash | Item::TrashEmpty => "trash",
            Item::Exclusions => "exclude",
            Item::Doctor => "doctor",
            Item::Config => "config",
            Item::Quit => COMMAND,
        }
    }
}

/// Lo que pide una tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Seguir en el menú (quizá con otra opción elegida).
    Stay,
    /// Salir de la terminal del menú y ejecutar la opción.
    Run(Item),
    Quit,
}

/// Estado del menú: la opción elegida.
#[derive(Debug, Default)]
pub struct Menu {
    selected: usize,
}

impl Menu {
    pub fn selected(&self) -> Item {
        ITEMS[self.selected]
    }

    fn step_by(&mut self, delta: isize) {
        let n = ITEMS.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }

    /// Una tecla: mueve la selección o pide ejecutar o salir.
    pub fn on_key(&mut self, key: KeyEvent) -> Step {
        if key.kind != KeyEventKind::Press {
            return Step::Stay;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return if key.code == KeyCode::Char('c') {
                Step::Quit
            } else {
                Step::Stay
            };
        }
        match key.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => Step::Quit,
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k' | 'h' | 'K' | 'H') => {
                self.step_by(-1);
                Step::Stay
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j' | 'l' | 'J' | 'L') => {
                self.step_by(1);
                Step::Stay
            }
            KeyCode::Enter => match self.selected() {
                Item::Quit => Step::Quit,
                item => Step::Run(item),
            },
            KeyCode::Char('u' | 'U') => Step::Run(Item::Undo),
            _ => Step::Stay,
        }
    }
}

/// Tras una acción: Intro (`Some(true)`) vuelve al menú; `q`, Esc o
/// Ctrl-C (`Some(false)`) salen. Cualquier otra tecla, o soltar una, sigue
/// esperando (`None`): una tecla perdida no vuelve a la lista.
pub fn after_action(key: KeyEvent) -> Option<bool> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(false);
    }
    match key.code {
        KeyCode::Enter => Some(true),
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => Some(false),
        _ => None,
    }
}

/// Filas que ocupa `text` partido por palabras en `width` columnas, para
/// reservarle sitio. Cuenta caracteres, no anchos (los textos del catálogo
/// no llevan caracteres dobles).
fn wrapped_rows(text: &str, width: u16) -> u16 {
    let w = usize::from(width.max(1));
    let (mut rows, mut col) = (1usize, 0usize);
    for word in text.split_whitespace() {
        let len = word.chars().count();
        if col > 0 && col + 1 + len <= w {
            col += 1 + len;
            continue;
        }
        if col > 0 {
            rows += 1;
        }
        // Una palabra más larga que la fila se parte en varias.
        rows += len.saturating_sub(1) / w;
        col = (len.saturating_sub(1) % w) + 1;
    }
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Dibuja el menú en `frame`: la lista, una línea en blanco, la
/// descripción de la opción elegida, otra en blanco y las teclas. Si no
/// cabe todo, primero sobra la descripción; si tampoco cabe la lista, se
/// desplaza para que la opción elegida se vea siempre, con las teclas
/// debajo mientras quede al menos una fila para la lista.
pub fn draw(frame: &mut Frame, menu: &Menu, i18n: &I18n, color: bool) {
    let look = |l: Look| if color { l } else { Look::default() };
    let title =
        Line::from(tr!(i18n, "menu-title")).style(look(Look::new().add_modifier(Modifier::BOLD)));
    let block = Block::bordered().title(title);
    let area: Rect = frame.area();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let desc = i18n.tr(menu.selected().messages().1, None);
    let keys = tr!(i18n, "menu-keys");
    let n = ITEMS.len() as u16;
    let h = inner.height;
    let keys_rows = wrapped_rows(&keys, inner.width);
    let desc_rows = wrapped_rows(&desc, inner.width);
    let keys_h = if h > keys_rows { keys_rows } else { 0 };
    let gap = u16::from(h > n + keys_h);
    let list_h = (h - keys_h - gap).min(n);
    let show_desc = h >= n + 1 + desc_rows + 1 + keys_h;

    // La ventana de la lista que contiene la opción elegida.
    let first = (menu.selected + 1).saturating_sub(usize::from(list_h));
    let lines: Vec<Line> = ITEMS
        .iter()
        .enumerate()
        .skip(first)
        .take(usize::from(list_h))
        .map(|(i, item)| {
            let chosen = i == menu.selected;
            let marker = if chosen { "› " } else { "  " };
            let label = i18n.tr(item.messages().0, None);
            let style = if chosen {
                look(Look::new().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Look::default()
            };
            Line::from(vec![Span::raw(marker), Span::styled(label, style)])
        })
        .collect();
    let row = |y: u16, height: u16| Rect::new(inner.x, inner.y + y, inner.width, height);
    frame.render_widget(Paragraph::new(lines), row(0, list_h));
    let mut y = list_h + gap;
    if show_desc {
        let text = Paragraph::new(Text::from(desc)).wrap(Wrap { trim: false });
        frame.render_widget(text, row(y, desc_rows));
        y += desc_rows + 1;
    }
    if keys_h > 0 {
        let text = Paragraph::new(Text::from(keys)).wrap(Wrap { trim: false });
        frame.render_widget(text, row(y, keys_h));
    }
}

/// ¿Está la terminal en la pantalla alternativa? Para que el *panic hook*
/// y el guard no la abandonen dos veces.
static ALTERNATE: AtomicBool = AtomicBool::new(false);

/// Deja la terminal como estaba: sin modo raw y, si hacía falta, fuera de
/// la pantalla alternativa y con el cursor visible. Se puede llamar más de
/// una vez.
fn restore() {
    let _ = terminal::disable_raw_mode();
    if ALTERNATE.swap(false, Ordering::SeqCst) {
        let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen, cursor::Show);
    }
}

/// Modo raw (y la pantalla alternativa si `alternate`) mientras vive. Al
/// soltarse, también al desenrollar un panic, la terminal vuelve a su
/// estado.
struct Session;

impl Session {
    fn start(alternate: bool) -> io::Result<Session> {
        terminal::enable_raw_mode()?;
        // Desde aquí, cualquier fallo restaura al soltar el guard.
        let session = Session;
        if alternate {
            ALTERNATE.store(true, Ordering::SeqCst);
            execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        }
        Ok(session)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        restore();
    }
}

/// Restaura la terminal antes de imprimir un panic (y luego sigue con el
/// *hook* anterior). Con `panic = "abort"` el guard no llega a soltarse:
/// este *hook* sí corre.
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}

/// Cada cuánto se mira, mientras se espera una tecla, si ha llegado
/// SIGTERM o SIGHUP.
const TICK: Duration = Duration::from_millis(200);

/// SIGTERM y SIGHUP. Con el menú abierto marcan `stop`, que el bucle mira
/// cada [`TICK`] para salir por el camino normal (y restaurar la
/// terminal). Mientras se ejecuta una acción (`acting`), la señal hace lo
/// que haría sin el menú: terminar el proceso. La terminal ya no está en
/// modo raw, y así una acción que se queda esperando no se vuelve sorda a
/// SIGTERM.
struct Signals {
    stop: Arc<AtomicBool>,
    acting: Arc<AtomicBool>,
}

impl Signals {
    fn register() -> io::Result<Signals> {
        let signals = Signals {
            stop: Arc::default(),
            acting: Arc::default(),
        };
        for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP] {
            // El orden importa: primero la acción por defecto (si se está
            // ejecutando una acción) y después la bandera.
            signal_hook::flag::register_conditional_default(sig, Arc::clone(&signals.acting))?;
            signal_hook::flag::register(sig, Arc::clone(&signals.stop))?;
        }
        Ok(signals)
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// El siguiente evento de la terminal, o `None` si ha llegado SIGTERM o
/// SIGHUP.
fn next_event(signals: &Signals) -> io::Result<Option<Event>> {
    loop {
        if signals.stopped() {
            return Ok(None);
        }
        match event::poll(TICK) {
            Ok(true) => return event::read().map(Some),
            Ok(false) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Descarta los eventos que ya esperan en la cola de crossterm: lo
/// tecleado de antemano no elige ni contesta nada.
fn drain() -> io::Result<()> {
    while event::poll(Duration::ZERO)? {
        event::read()?;
    }
    Ok(())
}

/// El menú hasta que la persona elige algo: `None` si sale. La sesión de
/// la terminal se suelta antes de devolver, así que nunca se ejecuta una
/// acción en modo raw.
fn choose(menu: &mut Menu, ui: &Ui, signals: &Signals) -> io::Result<Option<Item>> {
    let _session = Session::start(true)?;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        term.draw(|f| draw(f, menu, &ui.i18n, ui.color_out))?;
        let Some(event) = next_event(signals)? else {
            return Ok(None);
        };
        if let Event::Key(key) = event {
            match menu.on_key(key) {
                Step::Stay => {}
                Step::Run(item) => {
                    drain()?;
                    return Ok(Some(item));
                }
                Step::Quit => return Ok(None),
            }
        }
    }
}

/// Tras una acción, su salida queda a la vista hasta pulsar Intro (`true`,
/// volver al menú) o `q`, Esc o Ctrl-C (`false`, salir). No pasa por
/// `Ui::say`: con `--quiet` también hay que saber cómo seguir.
fn pause(ui: &Ui, signals: &Signals) -> io::Result<bool> {
    // Sin `println!`, que hace `panic!` si la terminal ya no está.
    let mut out = io::stdout();
    writeln!(out)?;
    write!(out, "{} ", ui.paint(Style::Bold, tr!(ui, "menu-pause")))?;
    out.flush()?;
    let back = {
        let _session = Session::start(false)?;
        // Lo tecleado durante la acción no vuelve al menú ni sale.
        ra_core::discard_pending_input();
        drain()?;
        loop {
            let Some(event) = next_event(signals)? else {
                break false;
            };
            if let Event::Key(key) = event
                && let Some(back) = after_action(key)
            {
                break back;
            }
        }
    };
    // Tras SIGHUP no hay terminal: el salto de línea ya no importa.
    let _ = writeln!(out);
    Ok(back)
}

/// La acción de una opción: la misma función que su subcomando, sin
/// `--yes` (el menú lo descarta) ni `--dry-run`, así que las que cambian
/// algo enseñan su plan y preguntan.
fn act(ui: &Ui, core: &Core, item: Item) -> Exit {
    match item {
        Item::Scan => crate::scan(ui, core, Vec::new()),
        Item::Clean => crate::clean(ui, core, false, false, Vec::new(), false),
        Item::Undo => crate::undo(ui, core, None, Vec::new(), false),
        Item::Free => free_last(ui, core),
        Item::History => crate::history(ui, core),
        Item::Trash => cmd::trash::run(ui, core, None),
        Item::TrashEmpty => cmd::trash::run(ui, core, Some(TrashCmd::Empty { dry_run: false })),
        Item::Exclusions => crate::exclude(ui, core, ExcludeCmd::List),
        Item::Doctor => crate::doctor(ui, core),
        Item::Config => cmd::config::run(ui, core, ConfigCmd::Path),
        Item::Quit => Exit::Ok,
    }
}

/// «Liberar ahora» de la última operación que aún tiene algo en la
/// Papelera (`Core::last_restorable`: la misma que `rotten undo` deshace
/// por defecto). Antes del plan, qué operación es; el plan, la
/// confirmación y los códigos son los de `rotten free <operación>`.
fn free_last(ui: &Ui, core: &Core) -> Exit {
    match core.last_restorable() {
        Err(e) => ui.core_error("free", &e),
        Ok(Some(op)) => {
            ui.say(tr!(
                ui,
                "menu-free-which",
                op = op.op_id.as_str(),
                date = ui.i18n.date(&op.started_at),
                kind = history_kind(op.kind)
            ));
            cmd::free::run(ui, core, &[op.op_id], false)
        }
        Ok(None) => {
            ui.say(tr!(ui, "menu-free-nothing"));
            Exit::Ok
        }
    }
}

fn terminal_error(ui: &Ui, e: &io::Error) -> Exit {
    restore();
    let english = tr!(
        I18n::with(Lang::En, false),
        "menu-terminal-error",
        detail = e.to_string()
    );
    let human = tr!(ui, "menu-terminal-error", detail = e.to_string());
    ui.fail(COMMAND, Exit::Error, "terminal_error", &english, &human)
}

/// El invariante de [`run`]: terminal, sin `--yes`, `--quiet` ni `--json`.
fn may_open(ui: &Ui) -> bool {
    !(ui.yes || ui.quiet || ui.json || !ui.interactive)
}

/// El menú: elegir, ejecutar fuera de la terminal del menú, esperar una
/// tecla y volver. Devuelve el código de la última acción (0 si ninguna).
///
/// Invariante: el menú siempre enseña el plan y pregunta, así que nunca se
/// abre con `--yes`, `--quiet` o `--json` ni sin terminal. `bare` y
/// `flags` ya lo evitan; si aun así llega, falla cerrado.
pub fn run(ui: &Ui, core: &Core) -> Exit {
    if !may_open(ui) {
        let e =
            io::Error::other("the menu always asks: a terminal, without --yes, --quiet or --json");
        return terminal_error(ui, &e);
    }
    install_panic_hook();
    let signals = match Signals::register() {
        Ok(s) => s,
        Err(e) => return terminal_error(ui, &e),
    };
    let mut menu = Menu::default();
    let mut last = Exit::Ok;
    loop {
        let item = match choose(&mut menu, ui, &signals) {
            Ok(Some(item)) => item,
            Ok(None) => return last,
            Err(e) => return terminal_error(ui, &e),
        };
        let started = Instant::now();
        techlog::start(item.command(), None);
        signals.acting.store(true, Ordering::SeqCst);
        last = act(ui, core, item);
        signals.acting.store(false, Ordering::SeqCst);
        techlog::finish(item.command(), last as u8, started);
        match pause(ui, &signals) {
            Ok(true) => {}
            Ok(false) => return last,
            Err(e) => return terminal_error(ui, &e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(menu: &mut Menu, codes: &[KeyCode]) -> Vec<Step> {
        codes.iter().map(|c| menu.on_key(key(*c))).collect()
    }

    #[test]
    fn arrows_and_hjkl_move_around_the_list() {
        let mut m = Menu::default();
        assert_eq!(m.selected(), Item::Scan);
        press(&mut m, &[KeyCode::Down]);
        assert_eq!(m.selected(), Item::Clean);
        press(
            &mut m,
            &[KeyCode::Char('j'), KeyCode::Char('l'), KeyCode::Right],
        );
        assert_eq!(m.selected(), Item::History);
        press(
            &mut m,
            &[KeyCode::Up, KeyCode::Char('k'), KeyCode::Char('h')],
        );
        assert_eq!(m.selected(), Item::Clean);
        press(&mut m, &[KeyCode::Left]);
        assert_eq!(m.selected(), Item::Scan);
        // En círculo, por los dos extremos.
        press(&mut m, &[KeyCode::Up]);
        assert_eq!(m.selected(), Item::Quit);
        press(&mut m, &[KeyCode::Down]);
        assert_eq!(m.selected(), Item::Scan);
        // Lo demás no mueve ni ejecuta.
        let steps = press(&mut m, &[KeyCode::Char('x'), KeyCode::Tab, KeyCode::F(1)]);
        assert!(steps.iter().all(|s| *s == Step::Stay));
        assert_eq!(m.selected(), Item::Scan);
    }

    #[test]
    fn enter_runs_the_selected_option() {
        let mut m = Menu::default();
        for (i, item) in ITEMS.iter().enumerate() {
            let mut m2 = Menu { selected: i };
            let want = if *item == Item::Quit {
                Step::Quit
            } else {
                Step::Run(*item)
            };
            assert_eq!(m2.on_key(key(KeyCode::Enter)), want, "{item:?}");
        }
        press(&mut m, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
        assert_eq!(m.on_key(key(KeyCode::Enter)), Step::Run(Item::Free));
    }

    #[test]
    fn u_undoes_from_anywhere_and_q_esc_ctrl_c_quit() {
        let mut m = Menu { selected: 5 };
        assert_eq!(m.on_key(key(KeyCode::Char('u'))), Step::Run(Item::Undo));
        assert_eq!(m.selected(), Item::Trash, "u no mueve la selección");
        assert_eq!(m.on_key(key(KeyCode::Char('q'))), Step::Quit);
        assert_eq!(m.on_key(key(KeyCode::Esc)), Step::Quit);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(m.on_key(ctrl_c), Step::Quit);
        // Otra combinación con Ctrl no hace nada (Ctrl-U no deshace).
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(m.on_key(ctrl_u), Step::Stay);
        // Soltar una tecla no cuenta.
        let mut release = key(KeyCode::Char('q'));
        release.kind = KeyEventKind::Release;
        assert_eq!(m.on_key(release), Step::Stay);
    }

    #[test]
    fn after_an_action_only_enter_goes_back_and_q_quits() {
        assert_eq!(after_action(key(KeyCode::Enter)), Some(true));
        for c in ['q', 'Q'] {
            assert_eq!(after_action(key(KeyCode::Char(c))), Some(false));
        }
        assert_eq!(after_action(key(KeyCode::Esc)), Some(false));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(after_action(ctrl_c), Some(false));
        // Cualquier otra tecla sigue esperando.
        for code in [
            KeyCode::Char(' '),
            KeyCode::Char('s'),
            KeyCode::Char('y'),
            KeyCode::Char('u'),
            KeyCode::Down,
            KeyCode::Tab,
        ] {
            assert_eq!(after_action(key(code)), None, "{code:?}");
        }
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(after_action(ctrl_q), None);
        let mut release = key(KeyCode::Enter);
        release.kind = KeyEventKind::Release;
        assert_eq!(after_action(release), None);
    }

    /// Con Bloq Mayús las letras llegan en mayúsculas y hacen lo mismo.
    #[test]
    fn capital_letters_do_the_same() {
        let shift = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT);
        let mut m = Menu::default();
        m.on_key(shift('J'));
        assert_eq!(m.selected(), Item::Clean);
        m.on_key(shift('L'));
        assert_eq!(m.selected(), Item::Undo);
        m.on_key(shift('K'));
        m.on_key(shift('H'));
        assert_eq!(m.selected(), Item::Scan);
        assert_eq!(m.on_key(key(KeyCode::Char('U'))), Step::Run(Item::Undo));
        assert_eq!(m.on_key(shift('Q')), Step::Quit);
    }

    #[test]
    fn the_menu_ignores_yes_and_quiet() {
        assert_eq!(flags(true, true, false), (false, false));
        assert_eq!(flags(true, false, false), (false, false));
        assert_eq!(flags(true, true, true), (true, true));
        assert_eq!(flags(false, true, true), (false, true));
    }

    #[test]
    fn without_a_subcommand_the_menu_needs_both_terminals_and_no_json() {
        assert_eq!(bare(false, true, true), Bare::Menu);
        assert_eq!(bare(false, false, true), Bare::Help);
        assert_eq!(bare(false, true, false), Bare::Help);
        assert_eq!(bare(false, false, false), Bare::Help);
        for (i, o) in [(true, true), (true, false), (false, true), (false, false)] {
            assert_eq!(bare(true, i, o), Bare::JsonError);
        }
    }

    fn render(menu: &Menu, i18n: &I18n, color: bool, width: u16) -> Buffer {
        render_sized(menu, i18n, color, width, 20)
    }

    fn render_sized(menu: &Menu, i18n: &I18n, color: bool, width: u16, height: u16) -> Buffer {
        let mut t = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        t.draw(|f| draw(f, menu, i18n, color)).expect("draw");
        t.backend().buffer().clone()
    }

    /// Las filas del buffer como texto, sin los bordes ni los espacios de
    /// los lados.
    fn rows(b: &Buffer) -> Vec<String> {
        (0..b.area.height)
            .map(|y| {
                let row: String = (0..b.area.width).map(|x| b[(x, y)].symbol()).collect();
                row.trim_matches(|c: char| c.is_whitespace() || "│┌┐└┘─".contains(c))
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn draws_every_option_with_the_selected_one_marked() {
        for (lang, scan, clean, keys) in [
            (Lang::Es, "Escanear", "Limpiar", "Intro: elegir"),
            (Lang::En, "Scan", "Clean", "Enter: choose"),
        ] {
            let i18n = I18n::with(lang, false);
            let mut m = Menu::default();
            m.on_key(key(KeyCode::Down));
            let text = rows(&render(&m, &i18n, true, 160));
            assert!(text.iter().any(|r| r == scan), "{text:#?}");
            assert!(text.iter().any(|r| r == &format!("› {clean}")), "{text:#?}");
            assert_eq!(text.iter().filter(|r| r.starts_with('›')).count(), 1);
            assert!(text.iter().any(|r| r.contains("rotten clean")), "{text:#?}");
            assert!(text.iter().any(|r| r.contains(keys)), "{text:#?}");
            // Todas las opciones, con su descripción, existen en el catálogo
            // (un mensaje que falta hace `panic!` en depuración).
            for i in 0..ITEMS.len() {
                render(&Menu { selected: i }, &i18n, false, 100);
            }
        }
    }

    /// RNF-24: cada fila es un mensaje entero del catálogo.
    #[test]
    fn pseudo_locale_finds_no_loose_text_in_the_menu() {
        let i18n = I18n::with(Lang::Es, true);
        for i in 0..ITEMS.len() {
            for row in rows(&render(&Menu { selected: i }, &i18n, false, 240)) {
                let row = row.trim_start_matches(['›', ' ']);
                if row.is_empty() {
                    continue;
                }
                assert!(
                    row.starts_with('⟦') && row.ends_with('⟧') && row.matches('⟦').count() == 1,
                    "texto fuera del catálogo: {row:?}"
                );
            }
        }
    }

    /// `--no-color` y `NO_COLOR`: ningún estilo en ninguna celda; con
    /// colores, la opción elegida sí lo lleva.
    #[test]
    fn without_colors_there_is_no_style_at_all() {
        let i18n = I18n::with(Lang::Es, false);
        let m = Menu::default();
        let plain = render(&m, &i18n, false, 80);
        for cell in plain.content() {
            assert_eq!(
                (cell.fg, cell.bg, cell.modifier),
                (Color::Reset, Color::Reset, Modifier::empty()),
                "celda con estilo: {cell:?}"
            );
        }
        let colored = render(&m, &i18n, true, 80);
        assert!(
            colored
                .content()
                .iter()
                .any(|c| c.fg == Color::Green && c.modifier.contains(Modifier::BOLD))
        );
    }

    /// En una terminal estrecha la descripción se parte, pero todo cabe.
    #[test]
    fn a_narrow_terminal_still_draws() {
        let i18n = I18n::with(Lang::Es, false);
        let text = rows(&render(&Menu { selected: 1 }, &i18n, false, 30)).join("\n");
        assert!(text.contains("› Limpiar"), "{text}");
    }

    /// En una terminal de 10 filas (8 dentro del marco) no cabe la lista
    /// entera: sobra la descripción, la lista se desplaza y la opción
    /// elegida se ve siempre, con las teclas debajo.
    #[test]
    fn a_short_terminal_scrolls_to_the_selected_option() {
        for lang in [Lang::Es, Lang::En] {
            let i18n = I18n::with(lang, false);
            let keys = tr!(i18n, "menu-keys");
            for (i, item) in ITEMS.iter().enumerate() {
                let text = rows(&render_sized(&Menu { selected: i }, &i18n, false, 80, 10));
                let label = i18n.tr(item.messages().0, None);
                assert!(
                    text.iter().any(|r| *r == format!("› {label}")),
                    "{item:?}: {text:#?}"
                );
                assert!(text.contains(&keys), "{item:?}: {text:#?}");
                let desc: String = i18n.tr(item.messages().1, None).chars().take(16).collect();
                assert!(
                    !text.iter().any(|r| r.contains(&desc)),
                    "{item:?}: {text:#?}"
                );
            }
        }
    }

    /// Una terminal de 20 filas sí tiene sitio para la descripción.
    #[test]
    fn the_description_shows_when_it_fits() {
        let i18n = I18n::with(Lang::Es, false);
        for (i, item) in ITEMS.iter().enumerate() {
            let text = rows(&render(&Menu { selected: i }, &i18n, false, 240));
            let desc = i18n.tr(item.messages().1, None);
            assert!(text.contains(&desc), "{item:?}: {text:#?}");
        }
    }

    /// `wrapped_rows` reserva al menos las filas que usa ratatui.
    #[test]
    fn wrapped_rows_is_never_short() {
        let i18n = I18n::with(Lang::Es, false);
        for width in [20u16, 30, 41, 60, 80] {
            for item in ITEMS {
                let desc = i18n.tr(item.messages().1, None);
                let mut t = Terminal::new(TestBackend::new(width, 40)).expect("terminal");
                t.draw(|f| {
                    let p = Paragraph::new(Text::from(desc.clone())).wrap(Wrap { trim: false });
                    f.render_widget(p, f.area());
                })
                .expect("draw");
                let b = t.backend().buffer();
                let used = |y: u16| (0..width).any(|x| b[(x, y)].symbol() != " ");
                let want = (0..40u16)
                    .rev()
                    .find(|y| used(*y))
                    .map_or(0, |y| usize::from(y) + 1);
                let got = usize::from(wrapped_rows(&desc, width));
                assert!(got >= want, "{width} {item:?}: {got} < {want}");
            }
        }
    }

    /// `run` falla cerrado, antes de tocar la terminal, si le llega `--yes`,
    /// `--quiet` o `--json`, o sin terminal. (Se prueba la condición y no
    /// `run`: sin ella, `run` abriría la terminal de verdad.)
    #[test]
    fn the_menu_refuses_to_open_without_its_invariant() {
        let ui = |yes, quiet, json, interactive| Ui {
            json,
            interactive,
            yes,
            quiet,
            i18n: I18n::with(Lang::Es, false),
            home: std::path::PathBuf::new(),
            color_out: false,
            color_err: false,
        };
        assert!(may_open(&ui(false, false, false, true)));
        for (name, ui) in [
            ("--yes", ui(true, false, false, true)),
            ("--quiet", ui(false, true, false, true)),
            ("--json", ui(false, false, true, true)),
            ("sin terminal", ui(false, false, false, false)),
        ] {
            assert!(!may_open(&ui), "{name}");
        }
    }

    /// Sin terminal (`interactive: false`) y sin `--yes`, las opciones que
    /// cambian algo enseñan el plan y acaban en `needs_yes` (2) sin tocar
    /// la carpeta personal ni la Papelera: nunca hay un «sí» implícito.
    #[test]
    fn changing_options_never_proceed_without_an_answer() {
        use ra_catalog::fixture::{Entry, materialize};
        use ra_core::{Env, TrashBackend};
        use std::collections::{BTreeMap, BTreeSet};
        use std::path::{Path, PathBuf};
        use std::time::SystemTime;

        fn snapshot(root: &Path) -> BTreeMap<PathBuf, (bool, u64, Option<SystemTime>)> {
            let mut out = BTreeMap::new();
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                for e in std::fs::read_dir(&dir).expect("read_dir") {
                    let p = e.expect("entry").path();
                    let m = std::fs::symlink_metadata(&p).expect("meta");
                    if m.is_dir() {
                        stack.push(p.clone());
                    }
                    out.insert(p, (m.is_dir(), m.len(), m.modified().ok()));
                }
            }
            out
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canon");
        let (home, trash, state) = (root.join("home"), root.join("trash"), root.join("state"));
        for d in [&home, &trash, &state] {
            std::fs::create_dir_all(d).expect("mkdir");
        }
        let entries = [
            // CAT-03, preseleccionado: lo que haría `clean`.
            ("Library/Logs/old-app/app.log", 3000, 30),
            ("Library/Logs/install.log", 800, 20),
            // Lo que se manda a la Papelera para `undo`, `free` y `trash empty`.
            ("Documents/borrador.txt", 500, 1),
        ]
        .map(|(path, size, age_days)| Entry {
            path: path.into(),
            size,
            age_days,
            dir: false,
        });
        materialize(&home, &entries, &[], SystemTime::now()).expect("materializar");
        let core = Core::with_apps(
            Env::new(home.clone(), state, TrashBackend::Dir(trash.clone())),
            Box::new(BTreeSet::<String>::new()),
        )
        .expect("core");
        let put = core
            .plan_selection(&[home.join("Documents/borrador.txt")])
            .expect("plan");
        let hash = put.plan.plan_hash().to_owned();
        let r = core.execute(put.plan, &hash, "test").expect("trash put");
        assert_eq!(r.done, 1);
        assert!(core.last_restorable().expect("historial").is_some());

        let ui = Ui {
            json: false,
            interactive: false,
            yes: false,
            quiet: false,
            i18n: I18n::with(Lang::Es, false),
            home: home.clone(),
            color_out: false,
            color_err: false,
        };
        let before = (snapshot(&home), snapshot(&trash));
        assert!(!before.1.is_empty(), "la Papelera tiene algo que vaciar");
        for item in [Item::Clean, Item::Undo, Item::Free, Item::TrashEmpty] {
            assert_eq!(act(&ui, &core, item), Exit::Usage, "{item:?}");
            assert_eq!((snapshot(&home), snapshot(&trash)), before, "{item:?}");
        }
    }
}
