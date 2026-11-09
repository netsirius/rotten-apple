//! Argumentos del CLI (07 §2–§4).
//!
//! Los comentarios de documentación son la referencia; la ayuda que ve el
//! usuario sale del catálogo de mensajes en su idioma ([`localized`]).

use std::path::PathBuf;

use clap::{Command, CommandFactory, Parser, Subcommand};

use crate::i18n::I18n;

#[derive(Debug, Parser)]
#[command(
    name = "rotten",
    version,
    about = "Rotten Apple: limpia tu Mac sin miedo. Todo va a la Papelera y se puede deshacer.",
    disable_help_subcommand = false
)]
pub struct Cli {
    /// Salida JSON (automática si la salida no es una terminal).
    #[arg(long, global = true)]
    pub json: bool,
    /// Ejecutar sin preguntar.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Menos texto.
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Sin colores (la salida actual no usa colores).
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Más detalle en los errores.
    #[arg(long, global = true)]
    pub debug: bool,
    /// Idioma de la salida para personas (es o en).
    #[arg(long, global = true, value_parser = ["es", "en"], hide_possible_values = true)]
    pub lang: Option<String>,
    /// Permitir ejecutar como root (RF-CLI-06).
    #[arg(long, global = true)]
    pub allow_root: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Escaneo inteligente: qué se puede limpiar, por categoría.
    Scan {
        /// Solo estas categorías (CAT-01, CAT-03, CAT-08, CAT-10).
        #[arg(long = "category", value_name = "ID")]
        categories: Vec<String>,
    },
    /// Mueve a la Papelera lo seleccionado tras revisarlo.
    Clean {
        /// Muestra el plan exacto sin cambiar nada.
        #[arg(long)]
        dry_run: bool,
        /// Borra sin pasar por la Papelera (irreversible; exige --yes).
        #[arg(long)]
        permanent: bool,
        /// Todo lo «Seguro» de estas categorías, no solo lo preseleccionado.
        #[arg(long = "category", value_name = "ID")]
        categories: Vec<String>,
        /// Incluye también lo que no es «Seguro» de esas categorías.
        #[arg(long)]
        include_review: bool,
    },
    /// Operaciones anteriores y si aún se pueden deshacer.
    History,
    /// Devuelve a su sitio lo que una operación mandó a la Papelera.
    Undo {
        /// Operación (por defecto, la última que se puede deshacer).
        op_id: Option<String>,
        /// Solo estos elementos (ids separados por comas).
        #[arg(long, value_delimiter = ',', value_name = "IDS")]
        items: Vec<u32>,
        /// Muestra qué se restauraría sin mover nada.
        #[arg(long)]
        dry_run: bool,
    },
    /// Borra para siempre de la Papelera lo que mandaron allí estas operaciones.
    Free {
        /// Operaciones cuyos elementos liberar.
        #[arg(required = true, value_name = "OP")]
        op_ids: Vec<String>,
        /// Muestra el plan exacto sin borrar nada.
        #[arg(long)]
        dry_run: bool,
    },
    /// Permisos, plataforma, configuración e integridad del journal.
    Doctor,
    /// Exclusiones: lo que nunca se toca.
    Exclude {
        #[command(subcommand)]
        action: ExcludeCmd,
    },
    /// Papelera: sin subcomando, lo que hay por volumen y lo que mandó Rotten Apple.
    Trash {
        #[command(subcommand)]
        action: Option<TrashCmd>,
    },
    /// Actualiza Rotten Apple por la vía con la que se instaló (Homebrew, app o cargo).
    Update {
        /// Dice qué se haría y con qué comando, sin ejecutar nada.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum TrashCmd {
    /// Mueve a la Papelera las rutas indicadas, tras revisarlas; se puede deshacer.
    Put {
        /// Ficheros o carpetas que mover a la Papelera.
        #[arg(required = true, value_name = "PATH")]
        paths: Vec<PathBuf>,
        /// Muestra el plan exacto sin cambiar nada.
        #[arg(long)]
        dry_run: bool,
    },
    /// Vacía la Papelera entera, tras revisarla (no se puede deshacer).
    Empty {
        /// Muestra el plan exacto sin borrar nada.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ExcludeCmd {
    /// Añade una exclusión: ruta, patrón glob, rule:<id>, cat:<CAT-xx> o bundle:<id>.
    Add {
        value: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Lista las exclusiones.
    List,
    /// Quita una exclusión.
    Remove {
        value: String,
        #[arg(long)]
        dry_run: bool,
    },
}

/// El idioma pedido con `--lang`, antes de analizar los argumentos (la
/// ayuda ya tiene que salir en ese idioma).
pub fn lang_flag(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        if a == "--lang" {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix("--lang=") {
            return Some(v.to_owned());
        }
    }
    None
}

/// La definición de [`Cli`] con la ayuda en el idioma de `i18n`: título de
/// cada comando (`cli-cmd-<ruta>`), ayuda de cada argumento
/// (`cli-cmd-<ruta>-arg-<id>`, o `cli-arg-<id>` si es global), nombres de
/// valores (`cli-value-<id>`) y encabezados. Los mensajes de error de clap
/// siguen en inglés.
pub fn localized(i18n: &I18n) -> Command {
    let mut cmd = Cli::command();
    // Con la definición completa: incluye `--help`, `--version`, el
    // subcomando `help` y las copias de los argumentos globales.
    cmd.build();
    localize(cmd, i18n, "")
}

fn localize(mut cmd: Command, i18n: &I18n, path: &str) -> Command {
    let name = cmd.get_name().to_owned();
    let key = if path.is_empty() {
        "cli-about".to_owned()
    } else if name == "help" {
        "cli-cmd-help".to_owned()
    } else {
        format!("cli-cmd-{path}")
    };
    cmd = cmd
        .about(i18n.tr(&key, None))
        .long_about(None)
        .subcommand_help_heading(i18n.tr("cli-heading-commands", None))
        .help_template(format!(
            "{{about-with-newline}}\n{} {{usage}}\n\n{{all-args}}{{after-help}}",
            i18n.tr("cli-heading-usage", None)
        ));
    let ids: Vec<(String, bool, bool, bool)> = cmd
        .get_arguments()
        .map(|a| {
            (
                a.get_id().to_string(),
                a.is_global_set() || ["help", "version"].contains(&a.get_id().as_str()),
                a.is_positional(),
                a.get_action().takes_values(),
            )
        })
        .collect();
    let options = i18n.tr("cli-heading-options", None);
    let arguments = i18n.tr("cli-heading-arguments", None);
    for (id, global, positional, takes_values) in ids {
        let help_key = if global {
            format!("cli-arg-{id}")
        } else if name == "help" {
            format!("cli-cmd-help-arg-{id}")
        } else {
            format!("{key}-arg-{id}")
        };
        let help = i18n.tr(&help_key, None);
        let value = takes_values.then(|| i18n.tr(&format!("cli-value-{id}"), None));
        let heading = if positional { &arguments } else { &options }.clone();
        cmd = cmd.mut_arg(&id, |a| {
            let a = a.help(help).long_help(None).help_heading(heading);
            match value {
                Some(v) => a.value_name(v),
                None => a,
            }
        });
    }
    if name == "help" {
        // Sus subcomandos solo existen para analizar `help <comando>`: no
        // tienen ayuda propia.
        return cmd;
    }
    let subs: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_owned())
        .collect();
    for sub in subs {
        let sub_path = if path.is_empty() {
            sub.clone()
        } else {
            format!("{path}-{sub}")
        };
        cmd = cmd.mut_subcommand(&sub, |s| localize(s, i18n, &sub_path));
    }
    cmd
}
