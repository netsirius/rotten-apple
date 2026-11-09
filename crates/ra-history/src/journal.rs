//! Journal JSONL *append-only* con escritura anticipada (RF-HIS-01/02).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ra_safety::{Identity, Origin};
use serde::{Deserialize, Serialize};

use crate::HistoryPaths;

pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Clean,
    Undo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpMode {
    Trash,
    Permanent,
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    Completed,
    /// Terminó con elementos omitidos o fallidos.
    Partial,
    /// Se interrumpió y se reconcilió al arrancar (05 §5.6).
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    Trash,
    Delete,
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event {
    OpStarted {
        kind: OpKind,
        mode: OpMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_hash: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        undo_of: Option<String>,
        items: u64,
        bytes: u64,
        /// Origen: `cli`, `gui`, `scheduler`.
        by: String,
        /// Papelera usada: `system` o `dir:<ruta>`.
        trash: String,
    },
    /// Se escribe y sincroniza **antes** de actuar.
    ItemIntent {
        item: u32,
        action: StepAction,
        path: PathBuf,
        identity: Identity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default)]
        regenerable: bool,
        #[serde(default)]
        bytes: u64,
        /// Permisos de la carpeta padre, para recrearla al deshacer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_mode: Option<u32>,
        /// De dónde salió el elemento del plan: reglas del catálogo o
        /// selección explícita. Falta en los journals anteriores a T-10 y en
        /// las restauraciones.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<Origin>,
    },
    ItemDone {
        item: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_path: Option<PathBuf>,
        /// Aviso de la comprobación posterior (p. ej. inode distinto).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        post_check: Option<String>,
    },
    ItemSkipped {
        item: u32,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    ItemFailed {
        item: u32,
        error: String,
    },
    /// Resultado deducido tras un cierre abrupto.
    ItemReconciled {
        item: u32,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_path: Option<PathBuf>,
        /// Por qué no se pudo saber dónde está (p. ej. la Papelera no se
        /// puede leer sin Acceso total al disco).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    OpFinished {
        status: OpStatus,
        done: u64,
        skipped: u64,
        failed: u64,
        /// Solo en una reconciliación: elementos que ya no están en su sitio
        /// y no se encontraron en su destino. No cuentan como hechos.
        #[serde(default, skip_serializing_if = "is_zero")]
        unknown: u64,
    },
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub schema_version: u32,
    pub ts: String,
    pub op: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal: {0}")]
    Io(#[from] io::Error),
    #[error("journal: no se puede serializar: {0}")]
    Encode(#[from] serde_json::Error),
}

/// Escritor del journal de una operación.
///
/// Cada registro se anexa con una sola llamada a `write` y sin búfer propio:
/// si el proceso muere, lo escrito sigue en el fichero. Solo un corte de
/// corriente puede perder lo que no se ha sincronizado con [`Journal::sync`].
#[derive(Debug)]
pub struct Journal {
    file: File,
    path: PathBuf,
    op: String,
    /// Registros anexados por este escritor.
    seq: u64,
    /// Registros que ya han llegado al disco (`seq` del último `sync`).
    durable_seq: u64,
}

fn rfc3339(t: SystemTime) -> String {
    humantime::format_rfc3339_millis(t).to_string()
}

impl Journal {
    /// Abre (en modo anexar) el fichero del mes en curso.
    pub fn open(paths: &HistoryPaths, op: &str, now: SystemTime) -> Result<Self, JournalError> {
        fs::create_dir_all(&paths.journal_dir)?;
        let ts = rfc3339(now);
        let path = paths.journal_dir.join(format!("{}.jsonl", &ts[..7]));
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        // Si un cierre abrupto dejó la última línea a medias, se cierra para
        // que el siguiente registro empiece en una línea nueva.
        if !ends_with_newline(&path)? {
            file.write_all(b"\n")?;
        }
        Ok(Self {
            file,
            path,
            op: op.to_owned(),
            seq: 0,
            durable_seq: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn op(&self) -> &str {
        &self.op
    }

    /// Anexa un evento y devuelve su número de orden en este escritor (1, 2…).
    /// Con `durable`, espera a que llegue al disco ([`Journal::sync`]) antes
    /// de volver. Una intención tiene que ser durable antes de actuar sobre
    /// su elemento: se escribe sin `durable` solo si después se sincroniza el
    /// lote entero y se comprueba con [`Journal::is_durable`].
    pub fn write(&mut self, event: Event, durable: bool) -> Result<u64, JournalError> {
        let rec = Record {
            schema_version: JOURNAL_SCHEMA_VERSION,
            ts: rfc3339(SystemTime::now()),
            op: self.op.clone(),
            event,
        };
        let mut line = serde_json::to_vec(&rec)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.seq += 1;
        if durable {
            self.sync()?;
        }
        Ok(self.seq)
    }

    /// Lleva al disco todo lo anexado hasta ahora. En macOS, `sync_data` es
    /// `fcntl(F_FULLFSYNC)`: vacía también la caché del disco.
    pub fn sync(&mut self) -> Result<(), JournalError> {
        if self.durable_seq < self.seq {
            self.file.sync_data()?;
            self.durable_seq = self.seq;
        }
        Ok(())
    }

    /// Si el registro `seq` (lo que devolvió [`Journal::write`]) ya está en
    /// el disco.
    pub fn is_durable(&self, seq: u64) -> bool {
        seq <= self.durable_seq
    }
}

fn ends_with_newline(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    f.seek(SeekFrom::Start(len - 1))?;
    let mut b = [0u8; 1];
    f.read_exact(&mut b)?;
    Ok(b[0] == b'\n')
}

/// Lee todo el journal en orden. Las líneas que no se pueden interpretar se
/// devuelven aparte (`doctor` las informa); la última línea de un fichero
/// puede estar cortada si hubo un cierre abrupto.
pub fn read_journal(paths: &HistoryPaths) -> Result<(Vec<Record>, Vec<String>), JournalError> {
    let mut files: Vec<PathBuf> = match fs::read_dir(&paths.journal_dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((vec![], vec![])),
        Err(e) => return Err(e.into()),
    };
    files.sort();
    let mut records = Vec::new();
    let mut problems = Vec::new();
    for f in files {
        let reader = BufReader::new(File::open(&f)?);
        for (n, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Record>(&line) {
                Ok(r) => records.push(r),
                Err(e) => problems.push(format!("{}:{}: {e}", f.display(), n + 1)),
            }
        }
    }
    Ok((records, problems))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> Identity {
        Identity {
            dev: 1,
            ino: 2,
            kind: ra_safety::FileKind::File,
            size: 3,
            mtime: 4,
            mtime_nsec: 0,
            ctime: 5,
            ctime_nsec: 0,
        }
    }

    #[test]
    fn roundtrip_and_torn_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = HistoryPaths::new(dir.path());
        let mut j = Journal::open(&paths, "op1", SystemTime::now()).expect("open");
        j.write(
            Event::ItemIntent {
                item: 1,
                action: StepAction::Trash,
                path: "/a".into(),
                identity: id(),
                target: None,
                category: Some("CAT-03".into()),
                regenerable: false,
                bytes: 10,
                parent_mode: Some(0o755),
                origin: Some(Origin::ExplicitSelection),
            },
            true,
        )
        .expect("write");
        j.write(
            Event::ItemDone {
                item: 1,
                result_path: Some("/t/a".into()),
                post_check: None,
            },
            true,
        )
        .expect("write");
        // Línea cortada por un cierre abrupto.
        let mut f = OpenOptions::new()
            .append(true)
            .open(j.path())
            .expect("open");
        f.write_all(b"{\"schema_version\":1,\"ts\":\"20")
            .expect("write");
        drop(f);
        // La siguiente operación empieza en una línea nueva.
        let mut j2 = Journal::open(&paths, "op2", SystemTime::now()).expect("open");
        j2.write(
            Event::ItemFailed {
                item: 9,
                error: "x".into(),
            },
            true,
        )
        .expect("write");
        let (recs, problems) = read_journal(&paths).expect("read");
        assert_eq!(recs.len(), 3);
        assert_eq!(problems.len(), 1);
        assert!(matches!(recs[0].event, Event::ItemIntent { item: 1, .. }));
        let line = fs::read_to_string(j.path()).expect("read");
        assert!(line.starts_with("{\"schema_version\":1,"));
        assert!(line.contains("\"ev\":\"item_intent\""));
        assert!(line.contains("\"origin\":\"explicit_selection\""));
    }

    /// Un registro escrito sin `durable` no cuenta como durable hasta el
    /// siguiente `sync`, que cubre todo lo anexado antes.
    #[test]
    fn records_are_durable_only_after_sync() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = HistoryPaths::new(dir.path());
        let mut j = Journal::open(&paths, "op", SystemTime::now()).expect("open");
        let fail = |item| Event::ItemFailed {
            item,
            error: "x".into(),
        };
        let a = j.write(fail(1), false).expect("write");
        let b = j.write(fail(2), false).expect("write");
        assert_eq!((a, b), (1, 2));
        assert!(!j.is_durable(a) && !j.is_durable(b));
        j.sync().expect("sync");
        assert!(j.is_durable(a) && j.is_durable(b));
        let c = j.write(fail(3), false).expect("write");
        assert!(!j.is_durable(c));
        let d = j.write(fail(4), true).expect("write");
        assert!(j.is_durable(c) && j.is_durable(d));
        // Sin `sync`, lo escrito ya está en el fichero (sobrevive a la muerte
        // del proceso, no a un corte de corriente).
        let e = j.write(fail(5), false).expect("write");
        assert!(!j.is_durable(e));
        let (recs, _) = read_journal(&paths).expect("read");
        assert_eq!(recs.len(), 5);
    }

    #[test]
    fn intents_without_origin_still_parse() {
        // Línea de un journal anterior a T-10: sin `origin`.
        let line = r#"{"schema_version":1,"ts":"2026-11-01T10:00:00.000Z","op":"o","ev":"item_intent","item":1,"action":"trash","path":"/a","identity":{"dev":1,"ino":2,"kind":"file","size":3,"mtime":4,"mtime_nsec":0,"ctime":5,"ctime_nsec":0},"category":"CAT-03","regenerable":false,"bytes":10}"#;
        let r: Record = serde_json::from_str(line).expect("parse");
        assert!(matches!(r.event, Event::ItemIntent { origin: None, .. }));
    }
}
