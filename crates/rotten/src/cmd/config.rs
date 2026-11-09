//! `rotten config get|set|path` (T-13, 07 §3).
//!
//! - `get <clave>`: el valor guardado o el que se aplica por defecto.
//! - `set <clave> <valor>`: valida clave y tipo, escribe de forma atómica y,
//!   sin `--dry-run`, crea el enlace `~/.config/rotten/config.toml` si en
//!   esa ruta no hay nada. Nunca sustituye lo que haya.
//! - `path`: dónde está la configuración y qué hay en la ruta del enlace.
//!   No crea nada.
//!
//! Códigos: `0` ok · `2` clave desconocida, de solo lectura o valor no
//! válido · `5` configuración corrupta (`get` y `set`) u otra cosa en la
//! ruta del enlace (`set`).

use std::path::Path;

use ra_core::{ConfigLink, Core, Expected, LinkFound, LinkState, SettingValue};
use serde_json::json;

use crate::args::ConfigCmd;
use crate::i18n::{I18n, tr};
use crate::{Exit, Ui};

const COMMAND: &str = "config";

/// Qué hay en la ruta del enlace cuando no es el enlace.
pub fn found(i: &I18n, f: &LinkFound, home: &Path) -> String {
    match f {
        LinkFound::File => tr!(i, "config-link-found-file"),
        LinkFound::Directory => tr!(i, "config-link-found-directory"),
        LinkFound::Symlink { target } => tr!(
            i,
            "config-link-found-symlink",
            target = ra_core::tilde(target, home)
        ),
        LinkFound::Other => tr!(i, "config-link-found-other"),
        LinkFound::Unreadable { detail } => {
            tr!(i, "config-link-found-unreadable", detail = detail.as_str())
        }
    }
}

/// Qué valores admite una clave.
pub fn expected(i: &I18n, e: &Expected) -> String {
    match e {
        Expected::OneOf(values) => tr!(i, "config-expected-one-of", values = values.join(", ")),
        Expected::Range { min, max } => tr!(
            i,
            "config-expected-range",
            min = min.to_string(),
            max = max.to_string()
        ),
    }
}

/// Una línea sobre el enlace (también la usa `doctor`).
pub fn link_line(ui: &Ui, link: &ConfigLink, config_path: &Path) -> String {
    let path = ui.path(config_path);
    let at = ui.path(&link.path);
    match &link.state {
        LinkState::Linked => tr!(ui, "config-link-linked", link = at, path = path),
        LinkState::Missing => tr!(ui, "config-link-missing", link = at),
        LinkState::Conflict { found: f } => tr!(
            ui,
            "config-link-conflict",
            link = at,
            found = found(&ui.i18n, f, &ui.home),
            path = path
        ),
    }
}

fn print_value(ui: &Ui, value: &SettingValue) {
    match value {
        SettingValue::Text(s) => ui.say(tr!(ui, "config-value", value = s.as_str())),
        SettingValue::Integer(n) => ui.say(tr!(ui, "config-value", value = n.to_string())),
        SettingValue::List(l) if l.is_empty() => ui.say(tr!(ui, "config-list-empty")),
        SettingValue::List(l) => {
            for v in l {
                ui.say(tr!(ui, "config-value", value = v.as_str()));
            }
        }
    }
}

pub fn run(ui: &Ui, core: &Core, action: ConfigCmd) -> Exit {
    match action {
        ConfigCmd::Get { key } => match core.config_get(&key) {
            Ok(value) => {
                if ui.json {
                    ui.emit(
                        COMMAND,
                        json!({ "action": "get", "key": key, "value": value }),
                    );
                } else {
                    print_value(ui, &value);
                }
                Exit::Ok
            }
            Err(e) => ui.core_error(COMMAND, &e),
        },
        ConfigCmd::Set {
            key,
            new_value,
            dry_run,
        } => match core.config_set(&key, &new_value, dry_run) {
            Ok(r) => {
                if ui.json {
                    let mut body = json!({
                        "action": "set",
                        "key": key,
                        "value": r.value,
                        "changed": r.changed,
                        "dry_run": r.dry_run,
                        "link": r.link,
                        "link_created": r.link_created,
                    });
                    if let Some(err) = &r.link_error {
                        body["link_error"] = json!(err);
                    }
                    ui.emit(COMMAND, body);
                    return Exit::Ok;
                }
                let change = if r.changed { "changed" } else { "unchanged" };
                let value = r.value.to_string();
                ui.say(if dry_run {
                    tr!(
                        ui,
                        "config-set-result-dry",
                        change = change,
                        key = key.as_str(),
                        value = value
                    )
                } else {
                    tr!(
                        ui,
                        "config-set-result",
                        change = change,
                        key = key.as_str(),
                        value = value
                    )
                });
                let config_path = &core.env().config_path;
                if r.link_created {
                    ui.say(tr!(
                        ui,
                        "config-link-created",
                        link = ui.path(&r.link.path),
                        path = ui.path(config_path)
                    ));
                }
                if let Some(err) = &r.link_error {
                    ui.say(tr!(
                        ui,
                        "config-link-error",
                        link = ui.path(&r.link.path),
                        error = err.as_str()
                    ));
                }
                Exit::Ok
            }
            Err(e) => ui.core_error(COMMAND, &e),
        },
        ConfigCmd::Path => {
            let config_path = &core.env().config_path;
            let link = core.config_link();
            if ui.json {
                ui.emit(
                    COMMAND,
                    json!({ "action": "path", "path": config_path, "link": link }),
                );
            } else {
                ui.say(tr!(ui, "config-path", path = ui.path(config_path)));
                ui.say(link_line(ui, &link, config_path));
            }
            Exit::Ok
        }
    }
}
