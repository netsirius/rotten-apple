# rotten-apple

A fast, memory-safe macOS cleaner written in Rust.

Rotten Apple limpia tu Mac **sin miedo**: todo va a la Papelera con un manifiesto propio para deshacer, nada se toca sin revisión previa y cada elemento se vuelve a comprobar justo antes de actuar. El análisis funcional está en `docs/` (rama `docs/analisis-funcional`).

## Estado

Esqueleto del núcleo, *CLI-first* (README de `docs/` §4, punto 1). Plataforma provisional: macOS 26 o posterior en Apple silicon (D-01).

| Crate | Qué hace |
|---|---|
| `ra-platform` | Única capa con `unsafe`: procesos en ejecución vía `libproc`. |
| `ra-safety` | Lista protegida incrustada, exclusiones fail-closed (`config.toml`), identidad `(dev, ino, tipo, tamaño, mtime, ctime)`, huella del contenido de carpetas y comprobación de apps abiertas. |
| `ra-catalog` | Reglas TOML incrustadas desde [`catalog/`](catalog/) (CAT-01, CAT-03, CAT-08, CAT-10) con un fixture por regla. |
| `ra-plan` | Plan inmutable con hash BLAKE3; la aprobación se liga al hash y caduca a los 30 min. |
| `ra-history` | Journal JSONL con escritura anticipada, manifiesto de deshacer e historial. |
| `ra-exec` | Única parte que modifica ficheros: revalidación justo antes de actuar, Papelera con `NSFileManager trashItemAtURL` (vía `objc2`), borrado relativo a descriptor y deshacer. |
| `ra-core` | Fachada que usan el CLI y, más adelante, la GUI. |
| `rotten` | CLI (`clap`). |

## CLI

```
rotten scan [--category CAT-xx]
rotten clean [--dry-run] [--yes] [--permanent] [--category CAT-xx] [--include-review]
rotten history
rotten undo [<op-id>] [--items 1,2] [--dry-run] [--yes]
rotten doctor
rotten exclude add|list|remove <ruta|glob|rule:<id>|cat:<CAT-xx>|bundle:<id>>
rotten update [--dry-run] [--yes]
```

`rotten update` nunca abre conexiones por su cuenta. Mira dónde está el binario (con los enlaces resueltos): en el Cellar de Homebrew ejecuta `brew upgrade netsirius/tap/rotten` con el `brew` de ese prefijo, tras confirmar o con `--yes`; dentro de un `.app`, explica que la app se actualizará con Sparkle (aún no disponible); con `cargo install` o en otra ruta, dice cómo actualizar. `--dry-run --json` da el método (`homebrew`, `app`, `cargo`, `other`) y el comando exacto (`argv`) sin ejecutar nada.

Flags globales: `--json` (automático si la salida no es una terminal), `--yes`, `--quiet`, `--lang es|en`, `--allow-root`. Todo documento JSON lleva `schema_version`.

Códigos de salida: `0` ok · `1` error · `2` uso o falta `--yes` · `3` falta permiso · `4` completado con omisiones · `5` abortado por seguridad o plan cambiado · `6` otro ejecutor activo.

Variables de entorno, pensadas para pruebas y entornos aislados:

- `ROTTEN_HOME`: carpeta personal sobre la que trabajar (por defecto `HOME`).
- `ROTTEN_STATE_DIR`: estado de la app (por defecto `<home>/Library/Application Support/<bundle-id>`).
- `ROTTEN_TRASH_DIR`: mover a esta carpeta (mismo volumen, sin sobrescribir) en lugar de a la Papelera del sistema.
- `ROTTEN_UPDATE_EXE`: ruta que `rotten update` toma como la de su ejecutable en lugar de la real (para probar cada vía de instalación).

Para probar sin tocar tu carpeta personal:

```bash
export ROTTEN_HOME=$(mktemp -d) ROTTEN_TRASH_DIR=$(mktemp -d)
cargo run -p rotten -- clean --dry-run
```

## Desarrollo

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

Las pruebas de propiedades (`crates/ra-core/tests/properties.rs`) corren 48 casos por propiedad; `PROPTEST_CASES=1000 cargo test -p ra-core --test properties` para más. La integración real con la Papelera del sistema tiene una prueba aparte que mueve un fichero temporal a tu Papelera y lo devuelve:

```bash
cargo test -p ra-exec -- --ignored system_trash_roundtrip
```

Licencia MIT. Mole es GPL-3.0: aquí se reimplementan ideas, nunca código.
