# Rotten Apple · Análisis funcional y de requisitos

**Rotten Apple** es una app de escritorio nativa para macOS que limpia, explica y protege. Está inspirada en [tw93/mole](https://github.com/tw93/mole) y tendrá **como mínimo todos sus comandos**, más una GUI limpia, módulos para desarrolladores (artefactos, dependencias vulnerables, secretos filtrados), privacidad e IA generativa opcional. El repositorio se llama `rotten-apple`.

Este directorio recoge el análisis que hizo un equipo de cinco agentes especializados, revisado de forma crítica por un sexto agente advisor (Fable). **Este README es la fuente de verdad**: cuando dos documentos se contradicen, manda lo que dice aquí.

- **Fecha:** 2026-11-01
- **Estado:** análisis completo. Las decisiones de la §3 son provisionales hasta que el autor las confirme.
- **Mockups navegables:** https://claude.ai/artifact/1VZGVWk8RWHEKN1YZbJioT (20 pantallas, mapa de funcionalidades, marca y componentes; fuentes en [`ui/mockups/`](ui/mockups/))

---

## 1. La idea en una frase

> Libera espacio y pon orden en tu Mac **sin miedo**: ves qué se va a tocar y por qué es seguro, y puedes deshacerlo. Si programas, recupera decenas de GB de artefactos, toolchains y cachés sin romper proyectos activos, y descubre qué tienes instalado que es vulnerable.

Principios que nadie discute (son la base del producto y de la primera implementación):

1. **Papelera primero.** Todo va a la Papelera con un manifiesto propio para deshacer. Borrar para siempre es una acción aparte («Liberar ahora») y nunca un efecto secundario.
2. **Previsualización obligatoria.** Ninguna acción destructiva sin revisión previa y sin comprobar justo antes de actuar que cada fichero es el mismo que se analizó (anti-TOCTOU).
3. **Solo se preselecciona lo «Seguro».** «Precaución» y «Revisar» los marca siempre el usuario.
4. **Sin red ni telemetría por defecto.** Cada conexión (OSV, IA, enlaces de marcadores) es opt-in y muestra qué sale del Mac.
5. **La IA solo propone.** Devuelve un plan que pasa por la misma revisión y el mismo deshacer que todo lo demás, y nunca tiene herramientas de escritura.
6. **Honestidad.** Nada de «acelerar el Mac» ni «liberar RAM». Se distingue lo «movido a la Papelera» de lo «liberado», y los tamaños son reales en disco (clones APFS, purgable, iCloud sin descargar).
7. **Clean-room.** Mole es GPL-3.0 y este repo es MIT: se reimplementan ideas, nunca código.

---

## 2. Documentos

| # | Documento | Qué contiene |
|---|---|---|
| 01 | [Análisis competitivo](analisis/01-analisis-competitivo.md) | 16 fichas (Mole, CleanMyMac, Pearcleaner, PureMac, DaisyDisk, OnyX, Hazel, herramientas de IA…), matriz de funcionalidades, huecos de mercado, diferenciadores, lecciones de Mole, riesgos de reputación. |
| 02 | [Requisitos](analisis/02-requisitos.md) | Visión, 3 personas, modos Simple/Experto, 13 casos de uso, catálogo de 19 categorías limpiables con rutas reales y riesgo, ~140 RF del núcleo, 19 RF-SEG (seguridad operativa), 42 RNF. |
| 03 | [Desarrolladores y seguridad](analisis/03-desarrolladores-y-seguridad.md) | Qué significa «sin uso» de verdad (atime no sirve en APFS: escala de evidencia E1–E4), inventario de 20+ ecosistemas, descubrimiento y protección de proyectos, vulnerabilidades (OSV, `brew vulns`, KEV, EPSS, SBOM), higiene de secretos. RF-DEV y RF-VUL. |
| 04 | [IA generativa](analisis/04-ia-generativa.md) | Catálogo de funciones con IA y su alternativa sin IA, proveedores (local, Claude con tu clave, Apple Foundation Models), privacidad, esquemas JSON de los planes, defensa contra prompt injection, coste por operación. RF-IA. |
| 05 | [Arquitectura](analisis/05-arquitectura.md) | Restricciones de macOS (TCC, SIP, sandbox), ADR-01 a 07 (GUI, distribución, núcleo, borrado seguro, catálogo, programación, IA), cadena de suministro, pruebas, RNF-ARQ. |
| 07 | [Comandos (CLI)](analisis/07-comandos-cli.md) | Paridad comando a comando con Mole (`clean`, `uninstall`, `optimize`, `purge`, `installer`, `slim`, `analyze`, `status`, `history`, `touchid`, `completion`, `update`, `remove`…) y comandos nuevos (`scan`, `undo`, `free`, `doctor`, `dev`, `vulns`, `secrets`, `privacy`, `organize`…). Flags globales, códigos de salida y requisitos RF-CLI-20 a 28. |
| 06 | [Revisión del advisor](analisis/06-revision-advisor.md) | Revisión crítica de 01–05 y de los mockups: 7 hallazgos bloqueantes, 16 importantes y 12 menores, 10 decisiones, MVP revisado y afirmaciones verificadas. |

**Precedencia:** este README › 06 › 02 (núcleo, CLI, RF-SEG y lista del MVP) › 03, 04 y 05 (cada uno en su dominio). Las erratas conocidas de 02–05 están en la §8.

---

## 3. Decisiones (provisionales, pendientes de tu confirmación)

Se adoptan las recomendaciones del advisor (06 §3). Cambiar cualquiera de ellas obliga a revisar las secciones citadas.

| # | Decisión | Valor provisional | Alternativa | Afecta a |
|---|---|---|---|---|
| D-01 | macOS mínimo y arquitectura | **macOS 26 + solo Apple silicon (arm64)** | 15 + arm64; 15 + universal | 02 RNF-38/42, 05 ADR-02 |
| D-02 | Vulnerabilidades | **v1**, en modo **online opt-in** con previsualización de lo que sale; el modo offline llega después | Núcleo mínimo en el MVP | 03 §7.2, 05 `ra-vuln` |
| D-03 | Zona Desarrollador en el MVP | **Corte mínimo**: proyectos en raíces sin TCC; `node_modules`, `target/`, `.venv`, DerivedData; protecciones git; cachés globales solo en lectura | Todo lo «Must» del 03 | 03 §7.1 |
| D-04 | Borrado permanente por operación | **No.** Solo «Liberar ahora» tras la operación y la opción «Liberar tras cada limpieza» en Ajustes (desactivada) | Botón aparte en modo Experto | 02 RF-SEG-03, Q-04 |
| D-05 | Proveedor de IA | **Local primero** (Ollama, LM Studio); Claude con tu clave como opción de más calidad. v1 = organizar carpetas + «¿qué es esto?» | Claude primero | 04 §3, §8.2 |
| D-06 | Catálogo de reglas remoto | **No (Won't)** en MVP y v1: las reglas llegan con cada versión firmada de la app | Feed firmado (ADR-05 §6.3) | 05 ADR-05 |
| D-07 | Stack de GUI y apariencia | **App de escritorio nativa** (no web). Spike de 2 semanas SwiftUI + UniFFI frente a Tauri 2. Dirección visual: **limpia, al estilo CleanMyMac pero con la paleta de hectorsantos.dev**, con un único foco por pantalla y botón redondo de acción. Falta la **variante clara** | — | 05 ADR-01 |
| D-08 | Nombre e icono | **Decidido: «Rotten Apple»**. Icono: manzana con mal aspecto en estilo de icono de macOS, **sin** la silueta mordida del logo de Apple. El riesgo de marca por «Apple» sigue anotado (R-ARQ-12) y conviene una consulta legal antes de publicar. Falta fijar el bundle ID antes de la beta | — | Textos de UI, 05 §3.5 |
| D-11 | Paridad con Mole | **Todos los comandos de Mole como mínimo** (doc 07), con las mejoras de seguridad (Papelera, deshacer) | — | 02 RF-CLI, 05 §7 |
| D-12 | Privacidad y rastreo | **Módulo propio en v1**: DNS, cookies, historiales, autocompletado, historial de descargas de macOS, recientes, portapapeles, Wi‑Fi y auditoría de permisos | Dejarlo para v2 (02 Q-05) | 02 Q-05, 07 §3.0 |
| D-13 | Secretos filtrados | **Módulo propio en v1** dentro de Desarrollo: repos, historial de git y carpetas; solo local, valores enmascarados | Solo higiene básica (03 §6) | 03 §6, 07 §3.1 |
| D-14 | Adelgazar apps (`slim`) | **v1** por paridad con Mole | Won't (02 Q-06) | 02 Q-06, 07 §2 |
| D-09 | Telemetría | **Ninguna** en MVP y v1. Informes de fallos solo bajo demanda y con vista previa | Opt-in agregada | 05 §9.5 |
| D-10 | ¿La IA puede proponer mandar a la Papelera? | **No** al organizar. En reglas automáticas (v2), solo tras simulación y activación manual | Sí | 04 §6.2.3 |

---

## 4. MVP único

Para un desarrollador individual, con beta cerrada en 6 meses o menos. Desarrollo **CLI-first**: el núcleo se prueba de punta a punta antes de la GUI. Criterio de salida: 20 o más usuarios durante 4 semanas, métricas M-1 a M-5 de 02 y **cero pérdidas de datos**.

**Entra, en este orden:**

1. Núcleo de seguridad (ADR-04): plan con hash, Papelera, journal *write-ahead*, deshacer, lista protegida incrustada, verificación de identidad justo antes de actuar. Pruebas de propiedades desde el primer día («nunca toca nada fuera del plan»).
2. CLI `rotten` con los comandos de Mole marcados MVP en el doc 07 (`clean`, `uninstall`, `purge`, `installer`, `analyze`, `history`, `completion`, `update`, menú interactivo) y los nuevos `scan`, `undo`, `free`, `trash empty`, `doctor`, `exclude` y `config`. Todos con `--dry-run`, `--json` y los códigos de salida unificados (§8).
3. Catálogo embebido con estas categorías:
   - CAT-01 cachés de usuario: preselección solo para los bundle IDs de una lista permitida ya validada; el resto, «Seguro» pero sin marcar.
   - CAT-03 logs.
   - CAT-08 navegadores.
   - CAT-10 restos de actualizadores.
4. Onboarding y permisos, con comprobación de permisos por módulo y el estado «Bloqueada · requiere permiso».
5. Desinstalador a nivel de usuario. Antes hay que verificar si hace falta el permiso «Gestión de apps».
6. Explorador en lista con «Más grandes» (sin treemap, duplicados ni antiguos).
7. Instaladores en Descargas y Escritorio.
8. Historial y deshacer, más el informe tras cada operación con «Liberar ahora».
9. Exclusiones, modos Simple y Experto, y elementos de arranque en solo lectura.
10. Zona Desarrollador mínima (D-03).
11. Español e inglés y accesibilidad base (VoiceOver, teclado, contraste, apariencia del sistema).
12. Firma, notarización, DMG y *tap* propio de Homebrew.

**v1:** vulnerabilidades (primero el bloque mínimo del 03) y el resto de la Zona Desarrollador (toolchains, Docker, `brew autoremove`, Xcode avanzado, IDE, modelos de IA). También:

- **Privacidad y rastreo** (D-12) y **Secretos** (D-13).
- **Adelgazar apps** (`slim`, D-14), `optimize` con tareas, `status`/Monitor y la **manzana en la barra de menús**, `remove`, lanzadores.
- Programación de limpiezas.
- Duplicados, archivos antiguos y treemap.
- Copias de iOS e instantáneas locales.
- Ayudante privilegiado (`SMAppService`).
- Monitor.
- IA (organizar y «¿qué es esto?») y organizador determinista por reglas.

**v2 o Won't:** catálogo remoto, asistente conversacional con herramientas, escritura directa de marcadores, `touchid`, servidor MCP, edición «lite» para la Mac App Store y telemetría.

> Fuera del MVP por riesgo o por dudas sin verificar: **CAT-02** (cachés en contenedores de otras apps). Una fuente secundaria dice que macOS 27 deniega el acceso por defecto, y no se sabe si el Acceso total al disco lo cubre (verificación V-03). También queda fuera **CAT-05** (`$TMPDIR`): poca ganancia para su riesgo.

---

## 5. Arquitectura en 8 líneas

- **Núcleo Rust** en un workspace de crates (`ra-core` como fachada; `ra-safety`, `ra-plan`, `ra-exec`, `ra-history`, `ra-catalog`, `ra-dev`, `ra-vuln`, `ra-ai`, CLI).
- Flujo **Escaneo → Reglas → Plan → Previsualización → Ejecución → Journal → Deshacer**.
- **GUI fina** sin lógica de negocio. Provisional: SwiftUI + UniFFI, pendiente del spike (D-07).
- **Distribución fuera de la Mac App Store**: Developer ID, hardened runtime, notarización, DMG, cask de Homebrew y Sparkle 2 con EdDSA. La App Sandbox haría imposible casi todo el producto.
- **Sin helper root en el MVP.** En v1, un daemon `SMAppService` con un conjunto cerrado de verbos por XPC.
- **Permisos:** el Acceso total al disco no se puede pedir ni detectar por API, así que se hace un *preflight* por comportamiento y hay un modo degradado sin permiso.
- **IA:** clientes HTTP en Rust; las claves, en el Llavero; Foundation Models solo vía Swift. La IA no depende del ejecutor.
- **Reglas como datos** (TOML versionado) con una prueba de fixture obligatoria por regla.

---

## 6. Pantallas

App de escritorio nativa: cada pantalla es una ventana de macOS de 1280×800 sobre el escritorio, con barra de menús.

**Dirección visual:** limpia, al estilo CleanMyMac pero mejorada y con la paleta de [hectorsantos.dev](https://claude.ai/artifact/8bKcBGen8guJQ2pmDS7ftH).

- Un único degradado azul noche `#000240` → índigo, con brillo lila, que cubre toda la ventana; la barra lateral va integrada.
- Acento `#574EFF`, lila `#E6B7FE` y verde `#5FED83`. Tipografías Instrument Sans y Commit Mono.
- Un solo foco por pantalla, títulos grandes y ligeros, y el **botón redondo de acción** abajo en el centro. A diferencia de CleanMyMac, ese botón nunca actúa a ciegas: abre la revisión.
- **La manzana**, mascota e indicador de estado, pasa de **podrida** a **a medias** y a **sana** con criterios explícitos (tablero «Marca»).

| # | Pantalla | Qué demuestra | Comando |
|---|---|---|---|
| 1 | Bienvenida y permisos | Icono de la app; Acceso total al disco explicado sin jerga; modo limitado | `doctor` |
| 2 | Escaneo inteligente | Portada → resultado; la manzana podrida y por qué; Simple/Desarrollador | `scan` |
| 3 | Limpieza · Sistema | Solo «Seguro» marcado; «Más opciones»; estado «Bloqueada · requiere permiso» | `clean` |
| 4 | Limpieza · Instaladores | DMG, PKG, ISO y XIP de Descargas, Escritorio, Homebrew, Mail y Telegram; «ya instalada» | `installer` |
| 5 | Revisión | Hoja previa: todo a la Papelera, omitidos y por qué, anti-TOCTOU | `--dry-run` |
| 6 | Listo | La manzana, sana; movido ≠ liberado; «Liberar ahora» solo para esta operación | `free` |
| 7 | Historial | Deshacer por operación; qué ya no es reversible | `history`, `undo` |
| 8 | Aplicaciones · Desinstalar | App y restos; tus datos sin marcar; documentos nunca | `uninstall` |
| 9 | Aplicaciones · Adelgazar | Quitar código Intel con verificación de firma | `slim` |
| 10 | Espacio | Mapa y archivos grandes sin preselección; iCloud aparte | `analyze` |
| 11 | Privacidad | Navegadores (los abiertos se omiten), DNS, historial de descargas, recientes, Wi‑Fi y auditoría de permisos | `privacy` |
| 12 | Mantenimiento | Elementos de inicio (restos de apps desinstaladas) y tareas que dicen cuándo sirven | `optimize` |
| 13 | Monitor | Estado de la manzana con criterios; CPU, memoria, disco, batería, red; procesos | `status` |
| 14 | Barra de menús | La manzana como indicador y panel rápido | `status --watch` |
| 15 | Desarrollo | Proyectos, toolchains, cachés, contenedores, Xcode; «Precaución» sin marcar | `purge`, `dev` |
| 16 | Comando oficial | Simulación de `brew cleanup` y aviso de que no pasa por la Papelera | `dev --dry-run` |
| 17 | Dependencias | Prioridad P0–P2 (gravedad + KEV/EPSS), arreglo copiable, qué datos salen | `vulns` |
| 18 | Secretos | Claves y tokens en repos, historial de git y configuración; valores enmascarados | `secrets` |
| 19 | Organizar con IA | Solo visible si se activa; reglas fijas frente a IA; qué se envía exactamente | `organize` |
| 20 | Ajustes | General, Permisos, Exclusiones, Programación, IA, Privacidad, Línea de comandos, Avanzado (Touch ID, desinstalar) | `config`, `exclude`, `schedule`, `completion`, `update`, `touchid`, `remove` |
| — | Mapa de funcionalidades | Cada función → pantalla → comando → fase | — |
| — | Marca | Icono, nombre, estados de la manzana, paleta y tipografía | — |

Las pantallas muestran el producto objetivo, no solo el MVP (cada función lleva su fase en el mapa). **Pendiente:** variante clara (D-07), estados vacío y error, pestaña de Marcadores en Organizar con IA y botón «¿Qué es esto?».

---

## 7. Riesgos y verificaciones pendientes

| Riesgo o verificación | Por qué importa | Cómo cerrarlo |
|---|---|---|
| V-03 · Acceso a contenedores de otras apps en macOS 26/27, con y sin FDA | Decide CAT-02 y los restos visibles del Desinstalador | VM con y sin FDA |
| V-04 · ¿Mover un `.app` ajeno a la Papelera requiere «Gestión de apps»? | Alcance del Desinstalador del MVP | VM y `log stream` de TCC |
| V-01 · URL del panel de FDA en macOS 26/27 | El botón principal del onboarding | `open "x-apple.systempreferences:…"` en VM |
| V-18 · Cachés «con estado» dentro de `~/Library/Caches` | Preseleccionar CAT-01 sin perder sesiones ni descargas | Borrar cada candidata en una VM y observar |
| Reputación de los «cleaners» en macOS | La confianza es el producto | Código abierto, sin telemetría, Papelera primero, textos honestos |
| Alcance | 12–18 meses si se suma todo lo propuesto | Respetar el MVP de la §4 |

**Ya verificado** (06 §5):

- macOS 27 solo funciona en Apple silicon.
- Homebrew 7 trae `brew vulns`, que no cubre los casks.
- Arc deja de funcionar el 22-dic-2026.
- `/usr/bin/fm` existe en macOS 27.
- Los IDs y precios de los modelos Claude coinciden con la referencia vigente.

---

## 8. Erratas conocidas en 02–05 (resueltas aquí)

| Tema | Conflicto | Se aplica |
|---|---|---|
| Códigos de salida del CLI | 02 RF-CLI-05 frente a 05 §7.3 | `0` ok · `1` error · `2` uso · `3` falta permiso · `4` completado con omisiones · `5` abortado por seguridad o plan cambiado · `6` otro ejecutor activo. Un único flag `--permanent`. `doctor` incluye el informe de permisos. |
| macOS mínimo e Intel | 02 (15 + universal) frente a 05 (15/26, arm64) | D-01 |
| Qué entra en el MVP | 02 §10, 03 §7, 04 RF-IA-25 | §4 de este README. Los «MVP» de 03 y 04 que no aparezcan en la §4 se leen como v1. |
| Catálogo remoto | 02 RF-LIM-17 (Won't) frente a 05 ADR-05 §6.3 | Won't en MVP y v1 (D-06); el diseño del ADR-05 queda como opción para v2. |
| Ejecución desatendida | 02 RF-SEG-14 frente a 05 §5.8/§7.2 | Solo con corriente; freno de 2× la mediana o 20 GB **y** tope de 5 GB por ejecución; la previsualización caduca a los 30 min. |
| Journal | 02 RNF-18 (180 días) frente a 05 §5.6 (90 días) | Ruta `~/Library/Application Support/<bundle-id>/journal/`, retención de 180 días. |
| Identidad anti-TOCTOU | 02 RF-SEG-07 no incluye `ctime` | `(dev, ino, tipo, tamaño, mtime, ctime)`, como en 05 §5.4. |
| Clones APFS | 02 RF-SEG-12 los da por no detectables | Usar `ATTR_CMNEXT_PRIVATESIZE` (verificado en 05 §1.5.2). |
| `TCC.db` en macOS 27 | 05 §0 lo afirma como hecho | Es la observación de un usuario de foro, no de Apple. La decisión (preflight por comportamiento) no cambia. |
| `move_to_trash` en reglas de IA | 04 §6.2.3 frente a 02 §8 | D-10 |
| Limpieza de privacidad | 02 Q-05 la dejaba para v2 | v1, módulo «Privacidad» (D-12, 07 §3.0) |
| Adelgazar apps | 02 Q-06 la daba como Won't | v1 por paridad con Mole (D-14) |
| Secretos | 03 §6 limitado a higiene básica | Módulo «Secretos» en v1 (D-13, 07 §3.1) |
| Lista de comandos del CLI | 02 RF-CLI y 05 §7 incompletos frente a Mole | 07 es la fuente de verdad; código de salida `10` para `vulns` y `secrets` con `--fail-on` |

---

## 9. Próximos pasos

1. Confirmar o cambiar las decisiones D-01 a D-10, sobre todo el nombre (D-08) antes de escribir textos definitivos.
2. Cerrar en VM las verificaciones V-01, V-03 y V-04.
3. Arrancar el núcleo CLI-first (`ra-safety`, `ra-plan`, `ra-exec`, `ra-history`) con pruebas de propiedades.
4. Hacer el spike de GUI del ADR-01 y diseñar la variante clara.
