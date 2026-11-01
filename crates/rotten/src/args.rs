//! Argumentos del CLI (07 §2–§4).

use clap::{Parser, Subcommand};

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
    /// Idioma de la salida para personas.
    #[arg(long, global = true, value_parser = ["es", "en"])]
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
    /// Permisos, plataforma, configuración e integridad del journal.
    Doctor,
    /// Exclusiones: lo que nunca se toca.
    Exclude {
        #[command(subcommand)]
        action: ExcludeCmd,
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
