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
//! válido · `4` `set` guardó la configuración pero no pudo crear el enlace ·
//! `5` configuración corrupta (`get` y `set`) u otra cosa en la ruta del
//! enlace (`set`).
//!
//! También: el idioma de la configuración (RF-AJU-07, [`configured_lang`])
//! y los avisos de `scan` y `doctor` sobre el enlace y las claves que esta
//! versión no conoce.

use std::path::Path;

use ra_core::{
    ConfigLink, ConfigState, Core, Env, Expected, Language, LinkError, LinkFound, LinkState,
    SettingValue,
};
use serde_json::json;

use crate::args::ConfigCmd;
use crate::i18n::{I18n, Lang, tr};
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
    }
}

/// El idioma fijado en la configuración (`language = "es"|"en"`), si la hay
/// y se puede leer; con `system`, sin valor o corrupta, `None` (manda
/// `LANG`).
pub fn configured_lang(env: &Env) -> Option<Lang> {
    match ConfigState::load(&env.config_path, &env.home) {
        ConfigState::Loaded(c) => match c.language {
            Some(Language::Es) => Some(Lang::Es),
            Some(Language::En) => Some(Lang::En),
            Some(Language::System) | None => None,
        },
        _ => None,
    }
}

/// Las claves que esta versión no conoce (se ignoran y se conservan).
pub fn unknown_keys_line(ui: &Ui, config_path: &Path, keys: &[String]) -> String {
    tr!(
        ui,
        "config-unknown-keys",
        path = ui.path(config_path),
        keys = keys.join(", ")
    )
}

/// Avisos de `scan` sobre la configuración, además del de corrupta: otra
/// cosa en la ruta del enlace (no se podrá limpiar) y claves desconocidas.
pub fn scan_notes(ui: &Ui, core: &Core) {
    let link = core.config_link();
    let config_path = &core.env().config_path;
    if matches!(link.state, LinkState::Conflict { .. }) {
        ui.say(link_line(ui, &link, config_path));
    }
    if let ConfigState::Loaded(c) = core.config_state()
        && !c.unknown.is_empty()
    {
        ui.say(unknown_keys_line(ui, config_path, &c.unknown_keys()));
    }
}

fn link_error(ui: &Ui, link: &ConfigLink, e: &LinkError) -> String {
    let at = link.path.as_deref().map(|p| ui.path(p)).unwrap_or_default();
    match e {
        LinkError::ParentNotDirectory { parent } => tr!(
            ui,
            "config-link-parent-not-directory",
            link = at,
            parent = ui.path(parent)
        ),
        LinkError::Io(err) => tr!(ui, "config-link-error", link = at, error = err.as_str()),
    }
}

/// Una línea sobre el enlace (también la usa `doctor`).
pub fn link_line(ui: &Ui, link: &ConfigLink, config_path: &Path) -> String {
    let path = ui.path(config_path);
    let at = link.path.as_deref().map(|p| ui.path(p)).unwrap_or_default();
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
        LinkState::ParentNotDirectory { parent } => tr!(
            ui,
            "config-link-parent-not-directory",
            link = at,
            parent = ui.path(parent)
        ),
        LinkState::NotApplicable { .. } => tr!(ui, "config-link-not-applicable"),
    }
}

fn print_value(ui: &Ui, value: &SettingValue) {
    match value {
        SettingValue::Text(s) => ui.say(tr!(ui, "config-value", value = s.as_str())),
        SettingValue::List(l) if l.is_empty() => ui.say(tr!(ui, "config-list-empty")),
        SettingValue::List(l) => {
            for v in l {
                ui.say(tr!(ui, "config-value", value = v.as_str()));
            }
        }
    }
}

/// `4` si la configuración se guardó pero el enlace no se pudo crear.
fn set_exit(r: &ra_core::ConfigSet) -> Exit {
    if r.link_error.is_some() {
        Exit::Partial
    } else {
        Exit::Ok
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
                        body["link_error"] = json!(err.to_string());
                    }
                    ui.emit(COMMAND, body);
                    return set_exit(&r);
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
                        link = r
                            .link
                            .path
                            .as_deref()
                            .map(|p| ui.path(p))
                            .unwrap_or_default(),
                        path = ui.path(config_path)
                    ));
                }
                if let Some(err) = &r.link_error {
                    ui.say(link_error(ui, &r.link, err));
                }
                set_exit(&r)
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
