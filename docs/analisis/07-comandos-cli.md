# 07 · Comandos (CLI) — paridad con Mole y comandos nuevos

> **Nota de consolidación (2026-11-01):** el autor ha fijado que Rotten Apple tendrá **como mínimo todos los comandos de Mole**, más los nuevos. Este documento es la fuente de verdad del CLI y prevalece sobre RF-CLI de 02 y sobre §7 de 05 donde difieran. La lista de comandos de Mole sale de su README público (clean-room: no se ha leído su código).

- **Binario:** `rotten`. Es provisional, pendiente del nombre definitivo (D-08). Se podrá añadir el alias corto `ra` si no colisiona con nada en `PATH`.
- **Núcleo:** el CLI y la GUI usan el mismo núcleo Rust (`ra-core`) y las mismas reglas de seguridad (RF-SEG).

---

## 1. Principios del CLI

1. **Mismas garantías que la GUI.** El CLI genera el plan, lo previsualiza, verifica cada elemento justo antes de actuar, lo apunta en el journal y permite deshacer. Por defecto todo va a la Papelera.
2. **`--dry-run` en todos los comandos que cambian algo.** Muestra el plan exacto, con los mismos datos que la hoja de revisión de la GUI.
3. **`--json` en todos los comandos.** Se activa solo si la salida va a una tubería, como en Mole. Cada documento lleva `schema_version`, y los escaneos incluyen `scan_status: complete | partial | unavailable`. Dentro de una versión, los campos nuevos entran como opcionales en el esquema aunque se emitan siempre (p. ej. `config_link` en `doctor` v2), para que un documento de una versión anterior siga validando; añadir un campo obligatorio, quitar uno o cambiar su tipo sube `schema_version`.
4. **Sin interacción solo con `--yes`.** Sin `--yes` y sin terminal interactiva, el comando aborta con código `2`.
5. **Borrado permanente solo con `--permanent`**, en los comandos que lo admiten, nunca por defecto. Ningún comando vacía la Papelera como efecto secundario.
6. **Sin `sudo` salvo que haga falta.** El CLI no se eleva solo: pide la acción con privilegios al helper (v1) o explica por qué no puede hacerla.
7. **Mismos códigos de salida en todos los comandos** (§5).

---

## 2. Paridad con Mole

| Mole | Rotten Apple | Qué mejora o cambia | Dónde está en la GUI | Fase |
|---|---|---|---|---|
| `mo` (menú interactivo) | `rotten` (menú interactivo en terminal) | Mismas teclas de navegación (flechas y estilo Vim) | La app entera | MVP |
| `mo clean` · `--dry-run` `--debug` | `rotten clean` · `--dry-run` `--debug` `--category <id>` `--include-review` `--permanent` | **Papelera por defecto** (Mole borra para siempre). **Nunca vacía `~/.Trash`** (Mole la vacía por defecto). Solo lo «Seguro» sin `--include-review`. | Limpieza | MVP |
| `mo clean --whitelist` | `rotten exclude add\|list\|remove <ruta\|regla>` | Exclusiones compartidas con la GUI. Si el fichero está corrupto, se aborta (fail-closed), como Mole. | Ajustes › Exclusiones; «Excluir siempre…» en cada detalle | MVP |
| `mo uninstall` · `--dry-run` `--with-data` `--permanent` `--list` | `rotten uninstall <app…>` · los mismos flags; `--list` devuelve JSON (`name`, `bundle_id`, `source`, `path`, `size`) | `--with-data` nunca incluye Documentos ni fototecas. Igual que Mole, lista las referencias en configuraciones de shell y MCP sin editarlas. | Aplicaciones › Desinstalar | MVP |
| `mo optimize` · `--dry-run` `--whitelist` | `rotten optimize` · `--dry-run` `--task <id>` `--skip <id>` | Cada tarea explica qué hace y qué efecto esperar; nada de «acelerar». Las que requieren cerrar apps o permisos de admin lo dicen antes. | Mantenimiento › Tareas | v1 (arranque en solo lectura: MVP) |
| `mo purge` · `--dry-run` `--yes` `--paths` | `rotten purge` · `--dry-run` `--yes` `--roots <dir…>` `--older-than 30d` | Papelera en vez de borrado permanente. Umbral de inactividad de 30 días (Mole: 7). Protecciones git ampliadas: cambios sin commitear, ramas sin push, stashes, `.env` y claves. | Desarrollo › Proyectos | MVP |
| `mo installer` · `--dry-run` | `rotten installer` · `--dry-run` `--source <downloads\|desktop\|brew\|icloud\|mail\|telegram>` | Papelera. Comprueba la identidad del fichero antes de actuar, como Mole. Si el análisis no termina, muestra lo encontrado como parcial (Mole lo descarta entero). | Limpieza › Instaladores | MVP |
| `mo slim` · `--dry-run` | `rotten slim <app…>` · `--dry-run` | Igual que Mole: trabaja sobre una copia, verifica firma y Gatekeeper y manda el original a la Papelera. Excluye apps de la App Store, de Apple y abiertas. | Aplicaciones › Adelgazar | **v1** (02 Q-06 lo daba como Won't; cambia por paridad) |
| `mo analyze [ruta]` · `--json` (alias `analyse`) | `rotten analyze [ruta]` · `--json` (alias `analyse`) | Tamaño real en disco (clones APFS una vez, `PRIVATESIZE`); no descarga ficheros de iCloud; mismo `scan_status`. | Espacio | MVP (lista) · v1 (mapa) |
| `mo status` · `--json` `--watch` `--interval` `--proc-cpu-threshold` `--proc-cpu-window` `--proc-cpu-alerts=false` | `rotten status` · los mismos flags | En lugar de una puntuación 0–100 opaca, el **estado de la manzana** (sana · a medias · podrida) con los criterios a la vista. NDJSON con `--watch`. | Monitor; icono de la barra de menús | v1 |
| `mo history` · `--json` | `rotten history` · `--json` | Cada operación dice si aún se puede deshacer. | Historial | MVP |
| — | `rotten undo [<op-id>]` · `--items <ids>` | **Nuevo:** deshacer de verdad, entero o por elementos. | Historial › Deshacer | MVP |
| `mo touchid` | `rotten touchid` · `--status` `--enable` `--disable` | Enseña el cambio exacto en `/etc/pam.d/sudo_local` antes de aplicarlo y se puede revertir. | Ajustes › Avanzado | v2 |
| `mo completion` | `rotten completion <zsh\|bash\|fish>` | — | Ajustes › Línea de comandos | MVP |
| `mo update` · `--nightly` | `rotten update` · `--channel <estable\|beta>` | Instalado como app, delega en Sparkle; instalado con Homebrew, en `brew upgrade`. | Ajustes › Actualizaciones | MVP |
| `mo remove` | `rotten remove` · `--keep-data` | Quita también agentes de launchd, helper y entradas de inicio. Avisa de las referencias que deja en la configuración del shell. | Ajustes › Avanzado | v1 |
| `mo --version` · `mo --help` | `rotten --version` · `rotten --help` · `rotten help <comando>` | — | Acerca de | MVP |
| Teclas `k` / `c` / `q` en `status`; `D` en `uninstall`; navegación de `purge` | Mismas teclas, más `u` para deshacer en el menú | — | — | MVP |
| `MO_NO_OPLOG=1` | **Sin equivalente** | El journal sostiene el deshacer y no se puede desactivar. Sí se configura su retención (180 días por defecto). | Ajustes › Privacidad | — |
| `MO_LAUNCHER_APP` | `ROTTEN_TERMINAL` | — | — | v1 |
| `~/.config/mole/whitelist` y `purge_paths` | Un único `config.toml` en `~/Library/Application Support/<bundle-id>/`, con enlace en `~/.config/rotten/config.toml` | Lo comparten GUI y CLI. Si está corrupto, todo aborta (fail-closed). | Ajustes | MVP |
| `~/Library/Logs/mole/operations.log` | Journal en `~/Library/Application Support/<bundle-id>/journal/` | Formato JSONL con manifiesto para deshacer. | Historial | MVP |
| Lanzadores de Raycast y Alfred | `rotten launchers install` | Instalación desde la app, sin `curl \| bash`. | Ajustes › Línea de comandos | v1 |
| Skills para agentes, plugins de Claude Code y Codex, `llms.txt` | Skill + `llms.txt` (v1) y servidor MCP `rotten mcp` (v2) | El agente solo puede ejecutar `--dry-run`, `scan`, `analyze`, `status`, `vulns` y `secrets` sin confirmación humana. | — | v1 / v2 |
| Instalación con `install.sh` (`--prefix`, versión) y Nix | DMG notarizado + cask; fórmula del CLI en un tap; `install.sh` opcional con `--prefix` | Mismo comportamiento con `--prefix` para no pedir contraseña al actualizar. | — | MVP (tap) · v1 (script) |

**Prueba de paridad (RF-CLI-20):** un test de CI ejecuta cada comando de la tabla con `--dry-run --json` y falla si falta alguno o si su JSON no valida contra el esquema.

**`config.toml` entre versiones.** Una clave nueva no rompe una versión anterior: las claves desconocidas fuera de `[exclusions]` se toleran, se avisan (`doctor` y los `warnings` de `scan`) y se conservan al reescribir el fichero. `[exclusions]` sí es estricto: una clave desconocida ahí, o una sección de primer nivel que parezca un `[exclusions]` mal escrito (cualquier nombre que contenga `exclu`), da la configuración por corrupta, porque ignorar una exclusión podría limpiar algo que la persona quiso proteger. `schema_version` del fichero solo sube con un cambio incompatible; una versión que no lo conoce lo da por corrupto (fail-closed). Si `config.toml` es un enlace (dotfiles), se escribe en su destino y el enlace se conserva. Con `ROTTEN_STATE_DIR`, el enlace de `~/.config/rotten/config.toml` no aplica: ni se crea ni se comprueba, y `doctor` y `config path` lo dicen (`state: not_applicable`).

---

## 3. Comandos nuevos (no existen en Mole)

| Comando | Qué hace | Flags principales | GUI | Fase |
|---|---|---|---|---|
| `rotten scan` | Escaneo inteligente: resumen de todas las áreas y estado de la manzana | `--json` `--area <id>` | Escaneo inteligente | MVP |
| `rotten free <op-id>` | «Liberar ahora»: vacía de la Papelera **solo** los elementos de esa operación | `--yes` | Listo · Historial | MVP |
| `rotten trash empty` | Vacía la Papelera entera (CU-08), con confirmación explícita | `--yes` | Escaneo inteligente › Papelera | MVP |
| `rotten doctor` | Permisos (FDA, Gestión de apps), versión de macOS, configuración, integridad del journal | `--json` | Ajustes › Permisos | MVP |
| `rotten dev` | Inventario de desarrollo: proyectos, toolchains, cachés, contenedores y Xcode | `projects\|toolchains\|caches\|containers\|xcode` `--json` `--dry-run` | Desarrollo | MVP (proyectos y cachés en lectura) · v1 |
| `rotten vulns` | Vulnerabilidades conocidas (OSV, `brew vulns`, KEV, EPSS) | `--online` `--fail-on p0\|p1\|p2` `--sbom cyclonedx\|spdx` `--json` | Dependencias | v1 |
| **`rotten secrets`** | **Fugas de credenciales** en repos y árboles de directorios: ficheros `.env` versionados, claves privadas, tokens, variables con secretos, historial de git | `[ruta…]` `--history` `--staged` `--fail-on high` `--json` `--baseline <fichero>` | Desarrollo › Secretos | **v1** (petición del autor) |
| **`rotten privacy`** | **Privacidad y rastreo**. Por navegador: historial, cookies, caché, autocompletado y lista de descargas. Del sistema: caché DNS, historial de descargas de macOS (`QuarantineEventsV2`), elementos recientes, miniaturas de Vista Rápida, portapapeles y redes Wi‑Fi conocidas. Además, auditoría de permisos de apps (cámara, micrófono, ubicación, grabación de pantalla, accesibilidad). | `--browser safari\|chrome\|firefox\|…` `--dns` `--recents` `--downloads-history` `--wifi` `--permissions` `--dry-run` `--json` | Privacidad | **v1** (petición del autor; 02 Q-05 lo dejaba para v2) |
| `rotten organize <carpeta>` | Plan de organización (reglas fijas + IA opcional); nunca borra | `--provider local\|claude` `--plan-only` `--apply <plan.json>` | Organizar con IA | v1 |
| `rotten explain <ruta>` | «¿Qué es esto?» para una carpeta o fichero (IA opcional; sin IA, explica desde el catálogo) | `--provider` | Botón «¿Qué es?» en Espacio y Limpieza | v1 |
| `rotten bookmarks` | Duplicados, enlaces rotos (opt-in) y exportación a HTML de marcadores | `chrome\|safari\|firefox` `--dedupe` `--check-links` `--export <fichero>` | Organizar con IA › Marcadores | v1 (lectura y exportación) |
| `rotten schedule` | Limpiezas programadas (solo «Seguro», con corriente) | `add\|list\|remove` `--weekly` `--monthly` | Ajustes › Programación | v1 |
| `rotten config` | Leer y cambiar la configuración | `get\|set\|path` | Ajustes | MVP |
| `rotten mcp` | Servidor MCP de solo lectura para agentes, con acciones opcionales que siempre requieren confirmación | `--allow-actions` | — | v2 |

### 3.0 `rotten privacy` en detalle (nuevo)

- **Reversible siempre que se pueda:** las bases de datos de cookies, historial y autocompletado son ficheros y **van a la Papelera**, así que se pueden deshacer.
- **No reversible pero inocuo:** vaciar la caché DNS (`dscacheutil -flushcache` y `killall -HUP mDNSResponder`, requiere admin) y el portapapeles.
- **Requisitos:** el navegador debe estar cerrado para tocar sus datos; si está abierto, se omite y se avisa. Safari y el historial de descargas de macOS requieren Acceso total al disco.
- **Riesgo:**
  - Cookies: «Precaución» (cierra sesiones). Si el usuario lo marca, se conservan las de los sitios que elija.
  - Redes Wi‑Fi conocidas: «Revisar» (el Mac dejará de conectarse solo). Se borran con `networksetup`, con admin.
- **Permisos de apps:** solo auditoría. Muestra qué apps tienen cada permiso y abre el panel correspondiente de Ajustes del Sistema; como mucho, ofrece `tccutil reset <servicio> <bundle-id>` con confirmación. Nunca concede permisos.
- **Sin interferir con la sincronización:** si iCloud o Chrome Sync están activos, se avisa de que el historial y los marcadores pueden volver o borrarse en otros dispositivos.

### 3.1 `rotten secrets` en detalle (nuevo)

Amplía la higiene de secretos del doc 03 §6, H1–H9, a petición del autor.

- **Alcance:**
  - Árbol de trabajo de los repos encontrados por `rotten dev projects`.
  - Su **historial de git** (`--history`): commits, stashes y ramas locales.
  - Rutas que indique el usuario, como `~/Downloads` o `~/Desktop` (opt-in, como en 03 §6).
- **Qué detecta:**
  - Claves privadas (SSH, PGP, `.p12`/`.pem`).
  - Tokens con formato conocido (AWS, GitHub, GitLab, Slack, Stripe, OpenAI/Anthropic…).
  - Ficheros `.env` **versionados** o sin entrada en `.gitignore`.
  - Variables con nombre sensible (`*_SECRET`, `*_TOKEN`, `PASSWORD`) y valores de alta entropía.
  - Credenciales en `~/.npmrc`, `~/.pypirc`, `~/.docker/config.json` y `~/.aws/credentials` con permisos laxos.
  - Claves SSH sin passphrase.
- **Garantías:**
  - Solo local; nada sale del Mac.
  - **Nunca valida contra APIs** si un token sigue activo (doc 03).
  - Muestra los valores **enmascarados** (`ghp_…7Qx`).
  - Solo hace cambios mínimos y reversibles, con confirmación y apuntados en el journal: añadir a `.gitignore`, corregir permisos y quitar una línea del historial del shell (con copia previa).
  - **Nunca** reescribe el historial de git ni rota claves por ti. Para eso ofrece guías y comandos que puedes copiar: rotar la clave, `git filter-repo`, mover el secreto al Llavero o a un gestor de secretos.
- **Integración:** `--baseline` permite ignorar hallazgos aceptados con motivo y caducidad, como RF-VUL. `--fail-on` lo hace útil en hooks de pre-commit y en CI.
- **Reutilización:** valorar la integración de reglas de detección ya existentes (gitleaks: MIT; detect-secrets: Apache-2.0) frente a implementarlas en Rust. Hay que revisar las licencias antes de incorporar sus reglas.

---

## 4. Flags globales

| Flag | Efecto |
|---|---|
| `--dry-run` | Muestra el plan sin cambiar nada |
| `--json` | Salida JSON (automática si la salida va a una tubería) |
| `--yes` | Permite ejecutar sin interacción |
| `--permanent` | Borra sin pasar por la Papelera. Solo en `clean`, `uninstall`, `purge` e `installer`; pide `--yes` |
| `--debug` · `--quiet` · `--no-color` | Verbosidad y formato |
| `--lang es\|en` | Idioma de la salida |

## 5. Códigos de salida

`0` ok · `1` error · `2` uso incorrecto o falta `--yes` · `3` falta permiso · `4` completado con omisiones · `5` abortado por seguridad o plan cambiado · `6` otro ejecutor activo · `10` (`vulns` y `secrets` con `--fail-on`) hallazgos por encima del umbral.

Con la configuración corrupta o con otra cosa en la ruta del enlace `~/.config/rotten/config.toml` (RF-SEG-05 c), las acciones destructivas abortan con `5` sin tocar nada, también con `--dry-run`: `clean`, `trash put`, `free`, `trash empty` y los cambios de configuración (`exclude add|remove`, `config set`). `undo` no: devolver algo a su sitio no depende de la configuración. Si `config set` guarda la configuración pero no puede crear el enlace (p. ej. `~/.config/rotten` es un fichero), sale con `4`.

Con `--dry-run`, el código es el que tendría la ejecución: `4` si la simulación ya prevé omisiones (decisión del autor del 2026-11-08, 08 §2.1).

## 6. Requisitos nuevos

| ID | Requisito | Prioridad | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-CLI-20 | Paridad: existe un equivalente de cada comando de Mole de la §2 | Must | MVP (los marcados MVP) / v1 / v2 | El test de paridad pasa en CI |
| RF-CLI-21 | `--dry-run` y `--json` en todos los comandos que cambian algo | Must | MVP | Test por comando; el JSON valida contra el esquema |
| RF-CLI-22 | Ningún comando borra para siempre ni vacía la Papelera sin `--permanent`, `free` o `trash empty` | Must | MVP | Test de propiedades sobre el ejecutor |
| RF-CLI-23 | `rotten secrets` detecta las clases de la §3.1 en el árbol de trabajo y, con `--history`, en el historial | Must | v1 | Fixtures con secretos sintéticos: 100 % detectados y 0 falsos positivos en el conjunto limpio |
| RF-CLI-24 | Los valores de los secretos nunca aparecen en claro en la salida, el journal ni los informes | Must | v1 | Test que busca los valores de los fixtures en todas las salidas |
| RF-CLI-25 | `rotten slim` solo sustituye la app si la copia pasa la verificación de firma y Gatekeeper; el original va a la Papelera | Must | v1 | Test E2E en VM con una app universal firmada |
| RF-CLI-26 | `rotten status --watch` emite NDJSON con el estado de la manzana y los criterios | Should | v1 | Validación del esquema |
| RF-CLI-27 | `rotten privacy` manda a la Papelera los ficheros de cookies, historial y autocompletado, y omite los navegadores abiertos | Must | v1 | Test E2E: navegador cerrado → restaurable con `rotten undo`; navegador abierto → omitido con código `4` |
| RF-CLI-28 | El vaciado de la caché DNS y del portapapeles se marca en el journal como «no reversible, inocuo» | Should | v1 | Inspección del journal |
