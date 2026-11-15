# 01 · Análisis competitivo y funcional

> **Nota de consolidación (2026-11-01):** las decisiones y el MVP están en [`docs/README.md`](../README.md). Verificado por el advisor: `brew vulns` (Homebrew 7) **no** cubre casks; macOS 27 solo funciona en Apple silicon (ver [06](06-revision-advisor.md) §5).

> **Proyecto:** rotten-apple, un limpiador para macOS, rápido y con memoria segura, escrito en Rust (licencia MIT).
> **Documento:** 01 de la serie de análisis. Sirve de entrada a 02 (requisitos), 03 (Zona Desarrollador y vulnerabilidades), 04 (IA) y 05 (arquitectura).
> **Fecha de investigación:** 2026-11-01 · **Plataforma de referencia:** macOS 27 "Golden Gate".
> **Estado:** borrador para revisión.

---

## 0. Metodología y alcance

- **Fuentes.** Solo información pública: README de repositorios, páginas oficiales de producto, fichas de la App Store, notas de versión, notas de prensa y reseñas. Todo se consultó el 2026-11-01. Las estrellas de GitHub, las versiones y los precios valen **a esa fecha** y cambian rápido.
- **Clean-room.** Mole (GPL-3.0), DevCleaner (GPL-3.0), GrandPerspective (GPL) y AI File Sorter (AGPL-3.0) se han analizado **solo por su descripción funcional** (README, web, notas de versión y artículos del autor). No se ha leído ni portado código fuente de ninguno. Las secciones "Qué copiar" describen **ideas de producto**, no implementaciones.
- **Grado de confianza.** Cuando un dato viene de una fuente secundaria (agregadores, blogs, tiendas de terceros) se marca con **(sec.)**. Lo que no se ha podido verificar se marca con **(?)** y se indica cómo comprobarlo.
- **Limitaciones conocidas.**
  - La tienda de MacPaw devolvió HTTP 403, así que los precios de CleanMyMac salen de Macworld (estructura de julio de 2025).
  - Varias herramientas de IA son muy recientes y apenas tienen reseñas independientes.
  - Las cifras de estrellas de algunos repositorios salen de agregadores.
- **Leyenda de las matrices:** ✅ lo cubre · ⚠️ parcial, limitado, solo CLI o solo en el plan de pago · ❌ no lo cubre · **?** no verificado.

---

## 1. Resumen ejecutivo

1. **Mercado polarizado y sin una GUI open source madura.** Por un lado hay suites comerciales pulidas pero caras y cerradas: CleanMyMac cuesta entre 47,50 y 71,40 $/año, o 119,95–195,95 $ en pago único. Por otro, herramientas OSS de un solo propósito o solo para terminal. Ninguna GUI open source (licencia OSI) es a la vez completa, mantenida y permisiva:
   - La GUI de Mole es **propietaria** (19 $).
   - Pearcleaner usa Apache 2.0 + **Commons Clause** (no es OSI) y su desarrollo está **en pausa desde finales de 2025**.
   - PureMac (MIT) es el competidor más directo, pero es muy joven y borra de forma permanente en la mayoría de sus flujos.
2. **Hay demanda.** Mole tiene unas 73,5k estrellas y publica varias versiones por semana. Su autor cuenta que muchos de los primeros usuarios pedían una versión que no exigiera teclear comandos. MacPaw ha lanzado su propia **CleanMyMac CLI** (beta pública, julio de 2026), que trae `clean dev/ai` y `purge` y que la propia MacPaw compara directamente con Mole. **El segmento desarrollador ya está en disputa.**
3. **La limpieza para desarrolladores está fragmentada:**
   - npkill cubre sobre todo `node_modules`.
   - kondo cubre más de 20 ecosistemas, pero solo como CLI que borra con `rm -rf`.
   - DevCleaner cubre solo Xcode y depende de estructuras sin documentar.
   - Mole `purge` cubre artefactos de proyecto.

   Nadie une en una sola vista los artefactos, las cachés de gestores, los toolchains y versiones viejos, Docker, los modelos de IA, las **dependencias globales sin uso** y su **estado de seguridad**.
4. **Seguridad de dependencias instaladas: la necesidad está validada, pero la cobertura es parcial.** Homebrew 7.0.0 (13-sep-2026, fecha según la prensa) ya trae `brew vulns` contra OSV, aunque **solo para fórmulas de Homebrew**. `osv-scanner` analiza lockfiles proyecto a proyecto y sin GUI. El hueco es una **vista unificada y multiecosistema** de lo instalado globalmente (brew, `npm -g`, pipx/pip de usuario, `cargo install`, gem, `go install`, imágenes Docker…) con base de datos OSV offline y explicaciones comprensibles.
5. **La organización con IA ya es una categoría en 2026, pero casi todo es en la nube o de pago:**
   - Eney de MacPaw (15-oct-2026).
   - Gemini Spark de Google (exige Google AI Ultra, 100 $/mes).
   - Sparkle de Every (en la nube).
   - Cosmos (on-device, 24,99 $).
   - AI File Sorter (AGPL, con LLM local).

   Nadie combina limpieza con organización IA **local-first** y **reglas deterministas generadas por IA** (al estilo Hazel). Para los **marcadores** solo hay extensiones inmaduras, con pocos usuarios y permisos amplios.
6. **El hueco transversal es el historial con deshacer.** Prácticamente ningún limpiador ofrece un deshacer real. Mole (en `clean`, `purge` e `installer`), PureMac (limpieza por categorías y programada), DaisyDisk y kondo borran de forma **permanente**. Solo algunos organizadores IA (Cosmos y AI File Sorter) traen deshacer.
7. **La confianza es la principal barrera de la categoría.** Pesan el antecedente de MacKeeper (acuerdo colectivo de 2 M$ por falsos avisos), los "optimizadores" catalogados como PUP y una comunidad de soporte de Apple escéptica con los limpiadores. La posición ganadora es: **honestidad** (no prometer "acelerar"), **transparencia** (reglas públicas y un "por qué es seguro" para cada elemento), **reversibilidad por defecto**, **open source MIT, sin telemetría y notarizado**.
8. **Ventana de plataforma.** macOS 27 solo funciona en Apple silicon (sec.) y Homebrew ha bajado Intel a Tier 3. Apuntar solo a Apple silicon simplifica el producto. Además, Pearcleaner deja usuarios huérfanos y OnyX necesita una versión distinta por cada macOS.

---

## 2. Panorama del mercado

| Segmento | Herramientas analizadas | Modelo dominante | Observación |
|---|---|---|---|
| Suites todo en uno | CleanMyMac, Mole CLI, Mole for Mac, PureMac | Suscripción o pago único; OSS solo en CLI | Convergen en limpieza + desinstalación + monitor + mantenimiento |
| Desinstaladores | Pearcleaner, AppCleaner (también Hazel App Sweep) | Gratuito | Producto maduro y comoditizado; se diferencian por la detección de restos |
| Análisis de espacio | DaisyDisk, GrandPerspective | Pago único barato o GPL | La visualización (treemap o sunburst) es un estándar ya resuelto |
| Mantenimiento | OnyX (también Mole `optimize`) | Gratuito | Una versión por macOS; muy técnico |
| Limpieza de desarrollo | DevCleaner, npkill, kondo, Mole `purge`, CleanMyMac CLI | OSS, CLI | Fragmentado por ecosistema |
| Seguridad de dependencias | `brew vulns`, osv-scanner | OSS, CLI | Sin GUI ni vista unificada de lo instalado |
| Organización por reglas | Hazel | Pago único | Determinista y sin IA |
| Organización con IA | Eney, Gemini Spark, Sparkle, Cosmos, AI File Sorter, LlamaFS | Suscripción o créditos en la nube; OSS local minoritario | Patrón común: proponer → revisar → aplicar |
| Marcadores con IA | Extensiones Chrome/Firefox (KeepOrganizedAI, Stash, NeuroMark…) | Gratuito o BYO-key | Muy inmaduro; ninguna solución de escritorio multinavegador |
| Monitor | Stats (además de Mole `status` y CleanMyMac Menu) | OSS MIT | Stats (unas 43k ★) marca el listón en la barra de menús |

---

## 3. Fichas de competidores

### 3.1 Mole (CLI) — tw93

| Campo | Detalle |
|---|---|
| Qué es | CLI/TUI todo en uno (`mo`) para limpiar, desinstalar, optimizar, analizar y monitorizar macOS. También tiene edición para Windows. |
| Licencia / precio | GPL-3.0, gratuito. Unas 73,5k ★ y 2,6k forks [1]. |
| Plataforma / stack | Shell + Go. macOS 12+, Intel y Apple silicon. Se instala con Homebrew o con script [1]. |
| Estado | Muy activo: V1.66.0 (29-oct-2026) y V1.67.0 (31-oct-2026) [2]. |

**Funciones.** Cada comando se detalla en el brief del proyecto y en [1]:
- `clean`: cachés, logs, temporales, artefactos de desarrollo y restos de apps; además **vacía ~/.Trash**.
- `uninstall`: app, LaunchAgents, preferencias y restos. Conserva los datos personales, manda a la Papelera y lista referencias en configuraciones de shell/MCP sin editarlas.
- `optimize`, `analyze` (top 30, a la Papelera), `status` (health score de 0 a 100).
- `purge`: agrupa por proyecto, desmarca los proyectos activos en los últimos 7 días y protege repos git anidados, ficheros trackeados y keypairs.
- `installer`, `slim` (verifica firma y Gatekeeper), `touchid`, `history` y `update`.

**Fortalezas**
- Amplitud funcional y mecanismos de seguridad bien pensados:
  - `--dry-run` en todos los comandos destructivos.
  - Whitelist **fail-closed**: si no puede leerse, `clean`, `purge` y `optimize` se detienen.
  - Log de operaciones (`~/Library/Logs/mole/operations.log`) consultable con `mo history`.
  - Sin sudo salvo cuando hace falta.
  - Validación de rutas y protección de ubicaciones del sistema [1].
- Conocimiento de casos límite que se refina en cada versión [2]:
  - Se salta las cachés de Chrome o Brave mientras esas apps están abiertas.
  - Conserva los modelos de llama.cpp y las versiones de herramientas de IA en uso.
  - Ya no borra paquetes de `npx`/`pnpm dlx`.
  - Limpia los simuladores iOS apagados.
- Salida JSON, integración con Raycast/Alfred y skills para agentes de IA.

**Debilidades**
- `clean`, `purge` e `installer` **borran de forma permanente**, y `clean` vacía la Papelera del usuario [1].
- Interfaz solo de terminal. La GUI existe, pero es un producto aparte y propietario (ver 3.2).
- Sin seguridad de dependencias ni organización con IA.
- `optimize` ha llegado a tocar preferencias del usuario: V1.67.0 corrige que reactivara "Show Related Content" de Spotlight [2]. Es un riesgo propio de cualquier tarea de "optimización".
- La GPL-3.0 impide reutilizar código en un proyecto MIT (de ahí el clean-room).

**Qué copiar (como idea):** dry-run universal, whitelist fail-closed, log legible con vista de historial, health score, la heurística de proyecto activo en `purge`, la protección de git y keypairs, avisar de referencias rotas en configuraciones sin editarlas, saltarse las cachés de apps abiertas y salida JSON para automatización.
**Qué evitar:** borrar de forma permanente por defecto, vaciar la Papelera de forma implícita y "optimizaciones" que cambian preferencias del usuario sin una forma explícita de revertirlas.

### 3.2 Mole for Mac (app nativa)

| Campo | Detalle |
|---|---|
| Qué es | App nativa separada del CLI, con limpieza, software, mantenimiento, análisis y estado [3]. |
| Licencia / precio | **Propietaria.** Licencia de 19 $ en pago único para 2 Macs, sin suscripción, con 14 días de reembolso. Escanear y revisar es gratis; cada herramienta de pago puede usarse 2 veces antes de pedir licencia [3]. |
| Plataforma | macOS 14+, Apple silicon e Intel. Distribución directa con autoactualización [3]. |

**Funciones** [3]:
- **Clean:** cachés, desarrollo, apps y System Data. Tiene un grupo "Manage Separately" que no suma al total.
- **AI Cleanup & Care:** versiones antiguas de herramientas de IA, cachés y sesiones. Las sesiones vienen desmarcadas hasta que el usuario confirma.
- **Software:** actualizaciones, elementos de arranque y desinstalación con revisión de restos (más de 800 apps según [1]).
- **Optimize, Analyze y Status:** treemap; CPU, GPU, memoria, disco, red, batería, temperatura y ventiladores; HUD en la barra de menús.
- **Utilidades:** Privacy Check (uso de cámara y micrófono) y Clean Screen.

**Comportamiento de borrado** [3]:
- La limpieza de cachés es **permanente por defecto** (en Ajustes hay opción de enviar a la Papelera).
- Desinstalar y Analyze envían a la Papelera.
- Vaciar la Papelera y algunas tareas de Optimize son permanentes.
- Comparte whitelist y log de operaciones con el CLI.

**Lecciones que publica su autor** [4]:
- Los usuarios pedían no tener que usar comandos.
- El modo oscuro forzado excluía a quien usa el modo claro.
- Clasifica el riesgo en tres niveles: cachés renovables, descargas costosas de rehacer y datos irreemplazables.
- En la desinstalación usa *ownership matching*, no coincidencia por nombre, para no borrar carpetas padre compartidas.
- Vuelve a verificar cada elemento en el momento de borrar.
- La app debe ser silenciosa: sin notificaciones molestas y con layouts estables que respeten "reducir movimiento".
- El precio pesa en la decisión de compra mucho más de lo que esperaba.

**Lectura competitiva.** Es la prueba más clara del hueco: el proyecto OSS más popular de la categoría eligió **cerrar su GUI**. Además, incluso con "revisar antes de borrar", su opción por defecto sigue siendo el borrado permanente de cachés.

### 3.3 CleanMyMac (MacPaw)

| Campo | Detalle |
|---|---|
| Qué es | La suite comercial de referencia. Se llama "CleanMyMac" sin la X desde la versión 5. |
| Licencia / precio | Propietaria. La web oficial anuncia "desde 3,33 $/mes" [5]. Estructura de julio de 2025 según Macworld [6]: **Basic** 9,95 $/mes, 47,50 $/año o 119,95 $ en pago único; **Plus** 15,95 $/mes, 71,40 $/año o 195,95 $. Una licencia por Mac (hay planes para 2 y 5) y 7 días de prueba. La versión de la Mac App Store trae menos funciones. El pago único puede no incluir futuras versiones mayores [6]. |
| Plataforma | macOS 11+. Versión 5.5.7 (22-jul-2026). Notarizado por Apple [5]. |
| Novedad 2026 | **CleanMyMac CLI**, beta pública gratuita y propietaria desde julio de 2026 (`brew install --cask cleanmymac-cli`), con los comandos `clean` (opciones dev, ai, junk y trash), `purge`, `analyze`, `optimize`, `config` e `ignore` [7]. La fuente es el blog de MacPaw (parte interesada). |

**Funciones** [5][6]:
- Smart Care (cinco tareas en un clic), limpieza de basura y malware (motor Moonlock).
- Rendimiento: RAM y CPU, login items, reindexado de Spotlight y Mail, DNS.
- Desinstalador con actualizador.
- Duplicados, fotos similares, ficheros grandes y antiguos.
- Space Lens (solo Plus), Cloud Cleanup (solo Plus: iCloud, Google Drive y OneDrive) y Purgeable Space (desde 5.0.5).
- Compatible con VoiceOver desde 5.0.7.

**Fortalezas**
- UX pulida con un flujo "un botón → resultados por categoría → revisar".
- Marca fuerte, accesibilidad y amplitud de funciones.
- Ahora también ataca el segmento desarrollador desde la terminal.

**Debilidades**
- Precio y suscripción, con funciones clave reservadas a Plus [6].
- Macworld valora el antimalware como "limitado": dejó partes de AdWind instaladas y se le escaparon apps dudosas [6].
- Muchas tareas de "rendimiento" equivalen a lo que ya se puede hacer desde Ajustes del Sistema, y la reseña no mide ninguna mejora real de velocidad [6].
- Código cerrado. Cloud Cleanup exige dar acceso a las nubes del usuario.

**Qué copiar:** el flujo guiado de un clic con resultados categorizados, el lenguaje claro para el usuario común, la accesibilidad con VoiceOver y la detección de copias locales redundantes de ficheros que ya están en la nube.
**Qué evitar:** el upsell agresivo, el escaneo de "amenazas" como gancho comercial y los mensajes de "rendimiento" sin evidencia.

**Relación con la IA:** MacPaw lanzó **Eney** (ver 3.14). En 2025 anunció una integración con CleanMyMac, pero no está confirmada en la cobertura del lanzamiento de 2026 [20].

### 3.4 PureMac

| Campo | Detalle |
|---|---|
| Qué es | Limpiador y utilidad de sistema en SwiftUI. Se presenta como "CleanMyMac alternative with zero telemetry" [8]. |
| Licencia / precio | **MIT**, gratuito. Unas 7,6k ★ y 441 forks [8]. |
| Plataforma / stack | Swift/SwiftUI, macOS 13+. DMG firmado y notarizado, cask de Homebrew y CLI `puremac` [8]. |
| Estado | Proyecto muy reciente: una reseña lo describe con unas dos semanas de vida al publicarse (sec.) [31]. La fecha de la primera versión no está verificada **(?)**; puede consultarse en la pestaña Releases. |

**Funciones** [8]:
- Desinstalador con preferencias, cachés y launch agents.
- Buscador de huérfanos.
- Limpieza de cachés, logs, Papelera, DerivedData de Xcode, Homebrew, npm/pnpm y Docker.
- Space Explorer, duplicados por SHA-256 y fotos similares.
- Auditoría de configuración (Gatekeeper, FileVault, firewall y SIP; no es un antivirus).
- Monitor de rendimiento, comprobación de actualizaciones de casks y limpieza programada.

**Borrado** [8]:
- **A la Papelera:** desinstalaciones, huérfanos y duplicados exactos.
- **Permanente:** limpieza por categorías, limpieza automática programada, vaciado de Papelera, Docker/Xcode, snapshots y CLI.

**Seguridad:** confirmación previa, dry-run y JSON en la CLI, allow-list de rutas, borrado resistente a symlinks (el propio proyecto admite que reduce, pero no elimina, las condiciones de carrera), protección de apps de Apple y exclusiones del usuario que también se aplican a las ejecuciones programadas.

**Fortalezas:** es el producto más parecido a la propuesta de rotten-apple: GUI nativa, MIT, sin telemetría y notarizado.
**Debilidades:**
- Muy joven, sin historial de fiabilidad.
- La limpieza programada borra de forma permanente y **sin supervisión**.
- Cobertura de desarrollo superficial (solo cachés, no artefactos por proyecto).
- Sin seguridad de dependencias ni IA.

**Qué copiar:** el borrado resistente a symlinks, la allow-list de rutas, los duplicados con una copia protegida y verificación justo antes de mover, y que las exclusiones afecten también a lo programado.
**Qué evitar:** borrados irreversibles sin supervisión.

### 3.5 Pearcleaner

| Campo | Detalle |
|---|---|
| Qué es | Desinstalador avanzado con herramientas de gestión de apps [9]. |
| Licencia / precio | Gratuito. **Apache 2.0 + Commons Clause**, que prohíbe monetizar la app o versiones modificadas, así que **no es OSI** [9]. Unas 15k ★. |
| Plataforma / stack | Swift/SwiftUI, macOS 13–26 (no consta soporte para 27). Versión 5.4.3 [9]. |
| Estado | **En pausa.** El autor indica que el desarrollo se ha detenido prácticamente desde finales de 2025 y que issues, PRs y versiones quedan en espera indefinida [9]. |

**Funciones** [9]:
- Desinstalación y búsqueda de huérfanos.
- Gestores de entornos de desarrollo, Homebrew, PKG, plugins y servicios; actualizador de apps; búsqueda de ficheros.
- *App lipo* (eliminar arquitecturas sobrantes) y poda de traducciones.
- **Sentinel Monitor**, que limpia automáticamente al tirar una app a la Papelera.
- Extensión de Finder, CLI, deep links y exportación de listas.

Necesita Acceso total al disco y un helper privilegiado. El autor avisa de webs falsas que distribuyen la app [9].

**Fortalezas:** profundidad en la gestión de apps, integración con Finder y la automatización de Sentinel.
**Debilidades:** proyecto parado, licencia restrictiva, dependencia de un único mantenedor y suplantaciones del canal de distribución.
**Qué copiar:** el concepto Sentinel (limpiar restos cuando la app se tira a la Papelera), la extensión de Finder ("Desinstalar con rotten-apple") y el inventario de PKG y servicios.
**Qué evitar:** el *bus factor* de 1 y un canal de descarga ambiguo. rotten-apple necesita un canal oficial único, firma y notarización, y builds reproducibles.

### 3.6 AppCleaner (FreeMacSoft)

| Campo | Detalle |
|---|---|
| Qué es | Desinstalador de arrastrar y soltar [10]. |
| Licencia / precio | Freeware de código cerrado. Es su distribución histórica, pero **no está verificado en la web oficial**, que no indica precio ni licencia [10]. |
| Plataforma | Versión **3.7 para macOS 15+**; 3.6.8 para macOS 10.14–15.6 [10]. |

**Fortalezas:** simplicidad extrema, porque hace una sola cosa, y una reputación muy extendida.
**Debilidades:** alcance mínimo y criterio de detección de restos opaco **(?)**.
**Qué copiar:** soltar una app sobre la ventana o el icono del Dock para iniciar la desinstalación, y una UI de tarea única para el usuario común.
**Qué evitar:** la opacidad sobre qué se borra y por qué.

### 3.7 DaisyDisk

| Campo | Detalle |
|---|---|
| Qué es | Analizador visual de espacio en disco [11]. |
| Licencia / precio | Comercial. **9,99 $ en pago único**, hasta 5 Macs personales, 30 días de garantía. Tienda directa (FastSpring) o Mac App Store [11]. |
| Plataforma | macOS 10.13+, Apple silicon e Intel. Versión 4.34.2 (10-jul-2026) [11]. |

**Funciones** [11]:
- Escaneo como administrador para revelar espacio oculto.
- Desglose de "Datos del sistema" y gestión de snapshots.
- **Cálculo preciso que tiene en cuenta hard links y clones APFS.**
- Escaneo de nubes (Dropbox, Google Drive, OneDrive y Box) y de discos externos.
- Vista previa con la barra espaciadora y protección de ficheros del sistema.
- El borrado es **permanente**.

**Fortalezas:** precisión, velocidad, un precio justo y una UX de referencia.
**Debilidades:** borrado permanente, ninguna orientación sobre qué conviene borrar y sin funciones de limpieza ni de desarrollo.
**Qué copiar:** el **espacio real recuperable** (no contar dos veces clones ni hard links), el desglose de System Data y snapshots, y la vista previa inmediata.
**Qué evitar:** el borrado irreversible.

### 3.8 GrandPerspective

| Campo | Detalle |
|---|---|
| Qué es | Visualizador de uso de disco con treemap [12]. |
| Licencia / precio | **GPL**. Gratis en SourceForge; 2,99 $ en la Mac App Store (es la misma app) [12]. |
| Plataforma | macOS 14+ (para macOS 11–13, la 3.7.2). Versión 3.8.1 (13-sep-2026) [12]. |

**Funciones** [12]:
- Treemap con color por nombre, tipo, tamaño, fechas y otros atributos.
- **Filtros avanzados** por nombre, ruta, tamaño, tipo, hard links y paquetes.
- Borrado desde la vista, búsqueda, Quick Look y apertura en Finder.
- **Guardar escaneos** y exportarlos como imagen o texto.
- Soporte de backups de Time Machine y ficheros de nube remotos.

**Fortalezas:** gratuito, ligero, filtros potentes y escaneos persistentes.
**Debilidades:** UI utilitaria, poco guiada para el usuario común.
**Qué copiar:** filtros combinables y **guardar escaneos para compararlos en el tiempo** ("¿qué ha crecido desde el mes pasado?"), algo que ningún competidor explota.
**Qué evitar:** exigir que el usuario interprete solo el treemap, sin recomendaciones.

### 3.9 OnyX (Titanium Software)

| Campo | Detalle |
|---|---|
| Qué es | Utilidad multifunción de mantenimiento, limpieza y parámetros ocultos [13]. |
| Licencia / precio | Gratuito, con donaciones opcionales (la web no usa la palabra "freeware"). Código cerrado [13]. |
| Plataforma | **Una versión por cada macOS.** OnyX 5.1.1 para macOS 27 "Golden Gate", solo Apple silicon (11-oct-2026); 5.0.6 para macOS 26 [13]. |

**Funciones** [13]: verificación de la estructura de ficheros del sistema; limpieza de cachés; reconstrucción de bases de datos e índices; desinstalación de apps; parámetros de Finder, Dock, Safari y otras apps de Apple.

**Fortalezas:** profundidad técnica, gratuidad y una trayectoria muy larga, desde Mac OS X 10.2 [13].
**Debilidades:**
- UI densa, pensada para expertos.
- Cada versión de macOS exige una versión nueva de OnyX (la web avisa de no usar versiones antiguas).
- Riesgo al tocar parámetros del sistema.

**Qué copiar:** presentar las tareas de mantenimiento con nombre y explicación (el equivalente gráfico de un comando), y **bloquear las acciones no validadas en una versión de macOS nueva**.
**Qué evitar:** cambiar ajustes del sistema sin explicación ni reversión.

### 3.10 DevCleaner for Xcode

| Campo | Detalle |
|---|---|
| Qué es | Limpiador de ficheros autogenerados de Xcode [14]. |
| Licencia / precio | **GPL-3.0**. Gratuito en la Mac App Store, con propinas dentro de la app [14]. Unas 1,6k ★. |
| Plataforma | El README dice "probado con macOS 14 y Xcode 15" y el copyright llega a 2025 [14]. Versión 2.8.0 del 11-nov-2025 (sec.) [38]. |

**Categorías** [14]: Device Support (por defecto conserva la versión más reciente de cada sistema), Archives, Derived Data, caché de documentación, logs antiguos de simulador y dispositivo, y descargas de documentación antiguas. El README no menciona los runtimes de simulador.

**Fortalezas:** preselección inteligente y conocimiento específico de Xcode.
**Debilidades:** el propio autor avisa de que depende de **estructuras internas no documentadas** y recomienda hacer copia de seguridad; además, el soporte declarado está desfasado [14].
**Qué copiar:** reglas de preselección por defecto (conservar la última versión y los archives publicados).
**Qué evitar:** depender de estructuras sin documentar sin pruebas por versión de Xcode. Conviene preferir interfaces oficiales cuando existan (detalle en el doc 03).

### 3.11 npkill

| Campo | Detalle |
|---|---|
| Qué es | CLI interactiva para localizar y borrar `node_modules` [15]. |
| Licencia / precio | **MIT**, gratuito. Unas 9,5k ★ [15]. |
| Stack | TypeScript/Node [15]. |

**Funciones** [15]: listado con tamaño y columna `last_mod`; selección múltiple; `-t` para otros nombres de directorio y perfiles de objetivos (por defecto, "node"); regex, exclusiones, ordenación, `--dry-run` y JSON; marca con ⚠️ las carpetas que podrían necesitar apps instaladas. El README **no aclara si el borrado es permanente (?)**.

**Fortalezas:** muy rápida de usar, interfaz interactiva clara, dry-run y JSON, y muy popular entre desarrolladores web.
**Debilidades:** centrada en Node; sin GUI nativa; no sabe si un proyecto está activo más allá de la fecha de modificación; permanencia del borrado no documentada.
**Qué copiar:** la columna de última modificación, la advertencia ⚠️ para `node_modules` que pertenecen a apps (no a proyectos) y los perfiles de objetivos.
**Qué evitar:** el sesgo hacia un único ecosistema.

### 3.12 kondo

| Campo | Detalle |
|---|---|
| Qué es | CLI, con GUI opcional, que limpia artefactos de proyectos [16]. |
| Licencia / precio | **MIT**, gratuito. Unas 2,6k ★ [16]. |
| Stack | **Rust**. La GUI `kondo-ui` se distribuye por winget, pacman, código fuente o releases; no se menciona un paquete para macOS [16]. |

**Funciones** [16]: detección por tipo de proyecto en más de 20 ecosistemas (Cargo, CMake, Composer, Elixir, Godot, Gradle, Jupyter, Pixi, Maven, Node, Pub, Python, SBT, Stack, Cabal, Swift, Unity, Unreal, Zig, .NET, Turborepo, Terraform y React Native) y filtro por antigüedad (`--older 3M`). No se menciona dry-run. El README advierte: *"essentially `rm -rf` with a prompt"*.

**Fortalezas:** la mayor amplitud de ecosistemas del segmento, el filtro por antigüedad y un binario en Rust rápido.
**Debilidades:** borrado permanente sin dry-run documentado, sin protección explícita de git ni de proyectos activos, y GUI sin distribución para macOS.
**Qué copiar:** el modelo **"marcador de proyecto → artefactos regenerables"**, el filtro por antigüedad y la amplitud de ecosistemas. Además, comparte stack con rotten-apple.
**Qué evitar:** el borrado permanente sin vista previa.

### 3.13 Hazel 6 (Noodlesoft)

| Campo | Detalle |
|---|---|
| Qué es | Organización automática de ficheros mediante reglas por carpeta [17]. |
| Licencia / precio | Comercial, pago único: 42 $ por usuario, 65 $ el Family Pack (5 Macs) y 20 $ la actualización (sec.) [18]. |
| Plataforma | Hazel 6.1 exige macOS 13+ y adopta el aspecto Liquid Glass en macOS 26 (sec.) [18]. |

**Funciones** [17]:
- Vigilancia de carpetas y condiciones por nombre, fecha, tipo, sitio de origen y otros atributos.
- Acciones: mover, renombrar, abrir, archivar, etiquetar y subir.
- Gestión de la Papelera (borrar lo antiguo o vaciarla cuando crece demasiado).
- **App Sweep**: ofrece borrar los restos al eliminar una app.
- Integración con Spotlight, Fotos, Música, Shortcuts, AppleScript y Automator.
- **La web no menciona funciones de IA.**

**Fortalezas:** determinismo, madurez (20 años) y funcionamiento silencioso en segundo plano.
**Debilidades:** la curva de aprendizaje para escribir reglas; no tiene IA.
**Qué copiar:** un **motor de reglas declarativas** con vista previa de lo que afectaría cada regla, y App Sweep.
**Oportunidad:** que la IA **redacte reglas tipo Hazel** a partir de una frase en lenguaje natural y que luego se ejecuten de forma determinista, auditable y sin enviar nada a la nube.

### 3.14 Organizadores de ficheros con IA (2025-2026)

| Herramienta | Tipo / licencia / precio | Modelo de IA y privacidad | Revisión y deshacer | Notas |
|---|---|---|---|---|
| **Eney** (MacPaw) [19][20] | Asistente "proactivo" para Mac e iPhone. Lanzado el **15-oct-2026**. Prueba de 14 días con 10.000 créditos; los planes de pago no aparecen en el anuncio **(?)**. | Modelos locales y en la nube (colaboración con Liquid AI) según MacPaw y la prensa. El anuncio no detalla qué se procesa dónde. | "Nada se mueve hasta que confirmes"; el usuario regula su autonomía. Deshacer **(?)**. | Ordena Descargas, organiza el Escritorio en subcarpetas y renombra capturas. Avisa de poco disco, presión de memoria y apps que fallan. La integración con CleanMyMac anunciada en 2025 no está confirmada. **Es el competidor más directo del Organizador con IA.** |
| **Gemini Spark** (Google) [21] | Agente en la app Gemini para Mac, en beta (jul-2026). Requiere **Google AI Ultra (100 $/mes)**. Solo EE. UU. y mayores de 18. | Infraestructura de Google Cloud; accede solo a los ficheros autorizados. | **(?)** | Clasifica Descargas y convierte una carpeta de recibos en una hoja de cálculo. Muy potente, pero caro y en la nube. |
| **Sparkle** (Every) [22] | App de Mac. Incluida en la suscripción de Every (20 $/mes o 200 $/año según el **anuncio original de 2024**); un directorio indica 9,25 $/mes (sec.). **Hay que verificar en makeitsparkle.co.** | En el anuncio original: GPT-4 en la nube; **solo se envían nombres de fichero**, que se guardan 30 días. | Revisión y deshacer **(?)**. | Escritorio, Documentos y Descargas se ordenan en Recents (menos de 3 días), AI Library y Manual Library (esta última no se toca). |
| **Cosmos – AI Files Assistant** [23] | Mac App Store, **24,99 $**, macOS 14+. Versión 2.0.1. | **On-device:** modelo de unos 1,8 GB o Apple Intelligence (v2.0). Funciona offline. | Aprobación previa ✅ y **deshacer** ✅. | Búsqueda en lenguaje natural, agrupación por tipo, tema o proyecto, análisis de PDFs, Smart Rename, reglas propias y aprendizaje a partir de lo que el usuario acepta o rechaza. |
| **AI File Sorter** [24] | **AGPL-3.0**, C++20/Qt 6. macOS 15+ en Apple silicon, Windows y Linux. v1.9.2 (14-ago-2026). Unas 1,9k ★. | Local con llama.cpp (Gemma 3 4B por defecto, modelos GGUF propios) o, de forma opcional, OpenAI, Gemini o endpoints compatibles (Ollama, LM Studio). Sin telemetría. | Tabla de revisión editable y dry-run. "Undo last run" de tipo *best-effort*. | Analiza nombres y, si se activa, el contenido de imágenes, documentos y metadatos. Es la referencia OSS del patrón "proponer → revisar → aplicar". |
| **LlamaFS** [25] | OSS; el README no indica licencia **(?)**. Unas 5,7k ★ (sec.). | Llama 3 vía Groq (nube) o Ollama (modo "incognito" local). | Modo batch con propuesta de estructura; deshacer **(?)**. | Un daemon vigila carpetas y aprende de los renombrados manuales. |
| **Local-File-Organizer** [26] | MIT/Apache (sec.). | 100 % on-device (Llama 3.2 3B + LLaVA). | **(?)** | Aparentemente inactivo desde octubre de 2024 (sec.). |

**Patrones observados:**
- Todos los productos serios convergen en **proponer → revisar → aplicar**, y los mejores añaden **deshacer**.
- Hay una tensión clara entre la potencia de la nube (Spark, Sparkle) y la privacidad local (Cosmos, AI File Sorter).
- **Ninguno integra organización y limpieza en la misma app con IA local.** Eney apunta en esa dirección, pero en la nube, con créditos y de forma propietaria.

**Qué copiar:**
- **Cosmos:** deshacer que devuelve cada fichero a su sitio original, aprendizaje a partir de lo que el usuario acepta o rechaza, y uso del modelo de Apple Intelligence cuando está disponible para no descargar otro.
- **AI File Sorter:** tabla de revisión editable, dry-run, modelos locales por defecto y BYO-key opcional para proveedores remotos.
- **Eney:** "nada se mueve hasta que confirmes" y un nivel de autonomía que el usuario regula.
- **Sparkle:** zona "Manual Library" que la IA no toca nunca y separación entre lo reciente y lo archivado.

**Qué evitar:**
- **Sparkle:** enviar nombres de fichero a la nube sin alternativa local.
- **Gemini Spark:** depender solo de la nube y un precio prohibitivo para el usuario común.
- **LlamaFS:** usar la nube como opción por defecto.
- En general, mover ficheros sin un plan visible y prometer un deshacer que solo funciona "en lo posible".

### 3.15 Organizadores de marcadores con IA

| Herramienta | Estado | Privacidad / modelo | Funciones |
|---|---|---|---|
| KeepOrganizedAI (Chrome) [27] | v1.3.0, actualizada en mayo de 2026 | El usuario elige el proveedor: Gemini, Claude, OpenAI u Ollama | Organización con IA |
| AI Bookmark Manager & Organizer (Chrome) [27] | 7 usuarios y una reseña de 1/5 | OpenAI, Claude u Ollama; dice enviar solo títulos y URLs | Categorización |
| AIBookMarks (Chrome) [27] | Sin auditoría de seguridad según su ficha | **(?)** | Sugiere carpetas y permite revisar antes de aplicar |
| Stash Bookmarks (Firefox) [27] | Unos 4 usuarios | Dice procesar en local, pero pide permisos amplios (incluido el historial) | Categorías, duplicados y enlaces rotos |
| NeuroMark, MarkAI, Booky (Firefox) [27] | Decenas de usuarios; Booky v0.3.3 (mar-2026) | Varios proveedores; Booky sincroniza con Supabase | Clasificación al guardar, renombrado, enlaces rotos y copia de seguridad |
| Raindrop.io [27] | Gestor de marcadores en la nube, maduro | Nube | Enlaces rotos (Pro) y duplicados; **no se ha encontrado una función de organización con IA** |

**Lectura:** el espacio es **inmaduro**: proyectos con pocos usuarios, permisos excesivos y ninguna solución de **escritorio** que lea los marcadores de Safari, Chrome, Firefox, Brave o Arc en local, los deduplique, detecte enlaces rotos y proponga una estructura con IA local antes de reescribir nada. Es una oportunidad pequeña pero clara (el detalle técnico va en el doc 04).

**Qué copiar:** revisión previa antes de reorganizar (AIBookMarks), detección de duplicados y enlaces rotos (Stash, MarkAI, Raindrop.io), copia de seguridad antes de cambiar nada (MarkAI) y elección de proveedor con opción local vía Ollama (KeepOrganizedAI).
**Qué evitar:** pedir acceso al historial de navegación, limitarse a un solo navegador y enviar URLs a la nube sin consentimiento explícito.

### 3.16 Otras referencias relevantes

| Herramienta | Qué aporta al análisis |
|---|---|
| **Homebrew `brew vulns`** (Homebrew 7.0.0, 13-sep-2026 según la prensa (sec.)) [28] | Comprueba las **fórmulas instaladas** contra OSV.dev, de serie. Usa una base de avisos propia que tiene en cuenta los parches retroportados por Homebrew para **reducir falsos positivos**. Opciones `--severity`, `--deps` y `--fix-available`; JSON, SARIF y CycloneDX según el README del proyecto de origen. Los paquetes sin URL de origen compatible se **omiten**, y omitido no significa limpio. La cobertura de **casks no está confirmada (?)**: hay que verificarla en la documentación oficial de Homebrew. |
| **osv-scanner** (Google) [29] | Apache-2.0, Go. Analiza lockfiles de más de 11 ecosistemas, directorios y contenedores. **Modo offline** con base de datos local y un `fix` guiado experimental. Es una base natural para el módulo Seguridad de dependencias (detalle en el doc 03). |
| **Stats** [30] | MIT, unas 43k ★, macOS 12+. Monitor en la barra de menús (CPU, GPU, memoria, disco, red, batería y sensores). Marca el listón del Monitor del sistema y demuestra que este módulo, por sí solo, no diferencia. |
| **Mac Sai, MacOSCleaner y otros** [31] | Muchos intentos OSS (BSD-3, unas 822 ★; unas 75 ★ con licencia no OSI según AlternativeTo). Hay **mucha oferta inmadura y poca consolidada**. |
| **Herramientas de Apple** [32] | Ajustes → General → Almacenamiento. Apple recomienda vaciar Descargas, borrar copias de seguridad de iOS, desinstalar apps, comprimir, vaciar la Papelera y arrancar en modo seguro para limpiar cachés. **No menciona limpiadores de terceros.** |

---

## 4. Matriz de funcionalidades

Hay tres tablas, una por grupo de competidores. En las tablas B y C se omiten las filas que valen ❌ para todo el grupo.

### 4.A Generalistas (suites, desinstaladores y mantenimiento)

| Módulo · capacidad | Mole CLI | Mole for Mac | CleanMyMac | PureMac | Pearcleaner | AppCleaner | OnyX |
|---|---|---|---|---|---|---|---|
| **Panel/Escaneo rápido** · escaneo unificado de un clic | ⚠️ menú `mo` | ? | ✅ Smart Care | ✅ | ❌ | ❌ | ❌ |
| · Puntuación de salud | ✅ 0–100 | ? | ? | ? | ❌ | ❌ | ❌ |
| **Limpieza del sistema** · cachés, logs y temporales | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ✅ |
| · Restos de apps ya desinstaladas (huérfanos) | ✅ | ✅ | ✅ | ✅ | ✅ | ? | ? |
| **Desinstalador** · app + restos | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ |
| · Quitar arquitecturas sobrantes (slim/lipo) | ✅ | ? | ? | ❌ | ✅ | ❌ | ❌ |
| · Actualizar apps | ❌ (`mo update` solo actualiza Mole) | ✅ | ✅ | ⚠️ casks | ✅ | ❌ | ❌ |
| **Explorador de espacio** · visualización | ⚠️ TUI | ✅ treemap | ⚠️ Space Lens (Plus) | ✅ | ❌ | ❌ | ❌ |
| · Grandes y antiguos | ⚠️ top 30 | ✅ | ✅ | ✅ | ⚠️ búsqueda | ❌ | ❌ |
| · Duplicados | ❌ | ? | ✅ | ✅ SHA-256 | ❌ | ❌ | ❌ |
| · Snapshots, System Data y espacio purgable | ? | ⚠️ System Data | ⚠️ purgable | ✅ snapshots | ❌ | ❌ | ? |
| **Instaladores y descargas** · DMG/PKG/ZIP sobrantes | ✅ | ? | ? | ? | ❌ | ❌ | ❌ |
| **Mantenimiento y arranque** · tareas (DNS, QuickLook, índices) | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ |
| · Gestión de elementos de arranque | ❌ | ✅ | ✅ | ? | ✅ servicios | ❌ | ? |
| **Monitor del sistema** · CPU, memoria, disco, red y batería en vivo | ✅ | ✅ | ⚠️ | ✅ | ❌ | ❌ | ❌ |
| · Barra de menús / HUD | ❌ | ✅ | ? | ? | ❌ | ❌ | ❌ |
| **Zona Desarrollador** · artefactos de proyecto | ✅ `purge` | ⚠️ | ⚠️ CLI `purge` (beta) | ❌ | ❌ | ❌ | ❌ |
| · Xcode (DerivedData, simuladores) | ⚠️ | ⚠️ | ? | ⚠️ DerivedData | ? | ❌ | ❌ |
| · Cachés de gestores de paquetes | ✅ | ✅ | ⚠️ CLI `clean dev` | ✅ | ⚠️ entornos dev | ❌ | ❌ |
| · Docker | ? | ? | ? | ✅ | ❌ | ❌ | ❌ |
| · Herramientas y modelos de IA | ⚠️ protege modelos | ✅ | ⚠️ CLI `clean ai` | ❌ | ❌ | ❌ | ❌ |
| · Dependencias o toolchains globales sin uso | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Seguridad de dependencias** · vulnerabilidades en lo instalado | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| · Auditoría de seguridad o malware | ❌ | ⚠️ Privacy Check | ⚠️ antimalware limitado | ✅ configuración | ❌ | ❌ | ❌ |
| **Organizador con IA** | ❌ | ❌ | ❌ (Eney aparte) | ❌ | ❌ | ❌ | ❌ |
| **Historial y deshacer** · dry-run / revisión previa | ✅ | ✅ | ✅ | ⚠️ CLI + confirmación | ⚠️ | ⚠️ | ? |
| · Papelera por defecto en **todos** los flujos | ❌ | ❌ | ? | ❌ | ? | ? | ? |
| · Log o historial de operaciones | ✅ | ✅ | ? | ? | ? | ❌ | ? |
| · Deshacer de un clic | ❌ | ❌ | ? | ❌ | ❌ | ❌ | ❌ |
| **Ajustes/Reglas/Programación** · whitelist o exclusiones | ✅ fail-closed | ✅ | ⚠️ `ignore` en la CLI (GUI ?) | ✅ | ? | ❌ | ? |
| · Programación / automatización | ❌ | ? | ? | ✅ | ⚠️ Sentinel | ❌ | ? |
| · CLI / JSON / integraciones | ✅ | ⚠️ vía CLI | ⚠️ CLI (beta) | ✅ | ✅ CLI + Finder | ❌ | ❌ |
| **Transversal** · GUI nativa | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| · Open source (OSI) | ✅ GPL | ❌ | ❌ | ✅ MIT | ❌ Commons Clause | ❌ | ❌ |
| · Mantenimiento activo | ✅ | ✅ | ✅ | ✅ | ❌ en pausa | ✅ (v3.7) | ✅ |

### 4.B Especialistas: espacio, desarrollo y seguridad

| Módulo · capacidad | DaisyDisk | GrandPerspective | DevCleaner | npkill | kondo | `brew vulns` | osv-scanner |
|---|---|---|---|---|---|---|---|
| **Explorador** · visualización | ✅ | ✅ treemap | ❌ | ❌ | ❌ | ❌ | ❌ |
| · Grandes y antiguos | ⚠️ tamaño | ✅ filtros | ❌ | ❌ | ❌ | ❌ | ❌ |
| · Snapshots, System Data y espacio oculto | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| · Espacio real (clones APFS y hard links) | ✅ | ⚠️ hard links | ❌ | ❌ | ❌ | ❌ | ❌ |
| · Guardar o comparar escaneos | ❌ | ✅ guardar | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Zona Desarrollador** · artefactos de proyecto | ❌ | ❌ | ❌ | ⚠️ Node (+`-t`) | ✅ más de 20 ecosistemas | ❌ | ❌ |
| · Xcode | ❌ | ❌ | ✅ | ❌ | ⚠️ Swift | ❌ | ❌ |
| · Filtro por antigüedad o actividad | ❌ | ⚠️ por fecha | ⚠️ versiones | ⚠️ `last_mod` | ✅ `--older` | ❌ | ❌ |
| **Seguridad de dependencias** · paquetes instalados globalmente | ❌ | ❌ | ❌ | ❌ | ❌ | ⚠️ solo fórmulas brew | ⚠️ directorios |
| · Lockfiles de proyectos | ❌ | ❌ | ❌ | ❌ | ❌ | ⚠️ Brewfile | ✅ |
| · Base de datos offline | — | — | — | — | — | ? | ✅ |
| **Historial y deshacer** · dry-run / revisión | ⚠️ | ⚠️ | ⚠️ | ✅ | ❌ | — | — |
| · Papelera por defecto | ❌ permanente | ? | ? | ? | ❌ `rm -rf` | — | — |
| **Ajustes** · exclusiones | ? | ✅ filtros | ❌ | ✅ | ❌ | ⚠️ severidad | ✅ |
| · CLI / JSON | ❌ | ❌ | ❌ | ✅ | ✅ | ✅ JSON/SARIF | ✅ |
| **Transversal** · GUI nativa macOS | ✅ | ✅ | ✅ | ❌ TUI | ❌ (`kondo-ui` sin paquete Mac) | ❌ | ❌ |
| · Open source (OSI) | ❌ | ✅ GPL | ✅ GPL-3.0 | ✅ MIT | ✅ MIT | ✅ | ✅ Apache-2.0 |

### 4.C Organización: reglas, IA y marcadores

| Capacidad | Hazel 6 | Eney | Sparkle | Cosmos | AI File Sorter | Gemini Spark | LlamaFS | Extensiones de marcadores con IA |
|---|---|---|---|---|---|---|---|---|
| Organizar Escritorio y Descargas | ✅ reglas | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ |
| IA generativa | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| IA local / on-device | n/a | ⚠️ híbrida | ❌ nube | ✅ | ✅ (nube opcional) | ❌ nube | ⚠️ modo local | ⚠️ algunas con Ollama |
| Renombrado inteligente | ✅ reglas | ✅ capturas | ? | ✅ beta | ✅ | ? | ✅ | ⚠️ |
| Análisis de contenido | ? | ? | ❌ solo nombres | ✅ | ✅ opcional | ✅ | ✅ | ⚠️ |
| Marcadores del navegador | ❌ | ? | ❌ | ❌ | ❌ | ? | ❌ | ✅ (un navegador) |
| Revisión antes de aplicar | ? | ✅ | ? | ✅ | ✅ | ? | ⚠️ | ⚠️ |
| Deshacer | ? | ? | ? | ✅ | ⚠️ best-effort | ? | ? | ⚠️ |
| Reglas deterministas | ✅ | ⚠️ | ⚠️ | ✅ | ? | ⚠️ automatizaciones | ❌ | ❌ |
| Automático en segundo plano | ✅ | ⚠️ proactivo | ✅ | ? | ❌ | ✅ | ✅ daemon | ⚠️ |
| Integración con limpieza | ⚠️ App Sweep y Papelera | ⚠️ avisos (CleanMyMac sin confirmar) | ❌ | ❌ | ❌ | ❌ | ❌ | ⚠️ duplicados y enlaces rotos |
| Open source (OSI) | ❌ | ❌ | ❌ | ❌ | ✅ AGPL | ❌ | ? | ? |
| Coste | 42 $ en pago único | créditos **(?)** | suscripción **(?)** | 24,99 $ | gratis | 100 $/mes | gratis | gratis o BYO-key |

**Titular de la matriz:** las filas **"Dependencias o toolchains globales sin uso"**, **"Vulnerabilidades en lo instalado"** y **"Deshacer de un clic"** salen en ❌ (o ?) para todos los generalistas. La **IA local integrada con la limpieza** no la tiene nadie.

---

## 5. Huecos de mercado y oportunidades

| # | Hueco | Evidencia | Oportunidad para rotten-apple |
|---|---|---|---|
| H1 | **GUI open source de calidad, permisiva y mantenida** | La GUI de Mole es propietaria [3]. Pearcleaner está en pausa y no es OSI [9]. PureMac es joven [8][31]. Los demás proyectos OSS son pequeños [31]. | GUI nativa con licencia MIT, notarizada, sin telemetría, con CLI paritaria sobre el mismo núcleo en Rust y gobernanza comunitaria. |
| H2 | **Reversibilidad: historial y deshacer** | Mole, PureMac, DaisyDisk y kondo borran de forma permanente en flujos clave [1][3][8][11][16]. Nadie ofrece un deshacer de un clic. | Todo pasa por la Papelera o por una **cuarentena con historial**. Deshacer por operación. El borrado permanente queda como opt-in explícito y solo para lo regenerable. |
| H3 | **Zona Desarrollador unificada** | Herramientas separadas por ecosistema [14][15][16]; Mole `purge` y CleanMyMac CLI solo en terminal [1][7]. | Una sola vista con artefactos por proyecto (con heurísticas de proyecto activo y protección de git), cachés de gestores, Xcode y simuladores, Docker y toolchains/versiones (nvm, pyenv, rustup, JDK…) agrupados por proyecto y antigüedad. |
| H4 | **Dependencias globales sin uso** | Nadie lo cubre (matriz 4.A). | Inventario de lo instalado globalmente (fórmulas "hoja" de brew, `npm -g`, pipx, `cargo install`, gem, `go install`) con señales de uso. Hay que validar la fiabilidad de las señales: el doc 03 debe comprobar si atime u otras señales son fiables en APFS **(?)**. |
| H5 | **Seguridad de dependencias instaladas** | `brew vulns` cubre solo Homebrew [28]; osv-scanner va por proyecto y sin GUI [29]. | Vista multiecosistema con OSV offline, severidad comprensible, "omitido ≠ limpio", enlace al comando de actualización y exportación a SARIF o CycloneDX. **Complementar `brew vulns`, no competir con él.** |
| H6 | **IA con privacidad** | Eney es híbrido y con créditos [19]; Spark cuesta 100 $/mes y va en la nube [21]; Sparkle va en la nube [22]. Solo Cosmos y AI File Sorter son locales [23][24]. | IA **opcional y local-first** (modelo on-device; BYO-key opcional), siempre con plan revisable y deshacer. La IA **propone reglas** que después se ejecutan de forma determinista. Nunca borra. |
| H7 | **Marcadores** | Solo extensiones inmaduras de un único navegador y con permisos amplios [27]. | Lector local multinavegador: duplicados, enlaces rotos y propuesta de carpetas, con copia de seguridad previa y aplicación revisable. |
| H8 | **Transparencia** | CleanMyMac y OnyX son cerrados [5][13]; los criterios de detección de AppCleaner son opacos [10]. | **Catálogo de reglas declarativo y público**: cada elemento muestra la regla que lo detectó, su nivel de riesgo, su espacio real (sin contar dos veces clones ni hard links, como DaisyDisk [11]) y por qué es seguro borrarlo. |
| H9 | **Higiene de herramientas de IA** (categoría emergente) | Mole protege modelos y sesiones [2]; Mole for Mac tiene "AI Cleanup & Care" [3]; CleanMyMac CLI tiene `clean ai` [7]. | Gestionar modelos de Ollama, LM Studio y la caché de Hugging Face, y las cachés y sesiones de asistentes de código, con las sesiones protegidas por defecto. |
| H10 | **Integración con agentes** | Mole ofrece skills para agentes de IA [1]. | Exponer rotten-apple como servidor MCP o CLI JSON con **dry-run por defecto** y confirmación humana para aplicar. |
| H11 | **Comparación temporal** | GrandPerspective guarda escaneos [12], pero nadie muestra la evolución. | "¿Qué ha crecido desde la última vez?" y avisos discretos cuando una carpeta se dispara. |

---

## 6. Diferenciadores propuestos y propuesta de valor

### 6.1 Diferenciadores priorizados

| Prioridad | Diferenciador | Por qué | Contra quién diferencia |
|---|---|---|---|
| **P0** | **Reversible por defecto**: Papelera o cuarentena, historial de operaciones y deshacer de un clic | Es el principal miedo del usuario y el hueco H2. Es barato de comunicar ("puedes deshacerlo") | Mole, PureMac, DaisyDisk, kondo |
| **P0** | **Transparencia explicable**: regla de origen, nivel de riesgo (regenerable / costoso de regenerar / irreemplazable) y espacio real recuperable; catálogo de reglas abierto | Genera confianza en una categoría con mala fama (sección 8) | CleanMyMac, OnyX, AppCleaner |
| **P0** | **GUI nativa open source (MIT), notarizada y sin telemetría**, con CLI y JSON paritarios | Hueco H1; el usuario lo pide [4] | La GUI de Mole, Pearcleaner, CleanMyMac |
| **P0** | **Zona Desarrollador unificada y segura** (artefactos, cachés, Xcode, Docker, toolchains, modelos de IA) | Hueco H3; segmento en disputa [7] | Mole `purge`, CleanMyMac CLI, kondo, npkill, DevCleaner |
| **P1** | **Seguridad de dependencias instaladas, multiecosistema y offline** | Hueco H5. Diferenciador único entre los limpiadores | Nadie en la categoría; complementa `brew vulns` |
| **P1** | **Dependencias y toolchains globales sin uso** | Hueco H4. Hay que validar la viabilidad técnica en el doc 03 | Nadie |
| **P1** | **Honestidad en "optimizar"**: nada de "acelerar" ni "liberar RAM"; mantenimiento explicado y con efecto esperado | Riesgo reputacional (sección 8) | CleanMyMac, MacKeeper |
| **P1** | **Organizador con IA local-first y opcional**: plan revisable + deshacer + reglas generadas por IA | Hueco H6. La IA aporta valor sin comprometer la privacidad | Eney, Gemini Spark, Sparkle |
| **P2** | **Marcadores multinavegador** | Hueco H7; poca competencia | Extensiones de IA |
| **P2** | **MCP y launchers** (Raycast/Alfred) | Hueco H10; paridad con Mole | Mole |
| **P2** | **Evolución temporal del disco y avisos discretos** | Hueco H11; sin notificaciones molestas [4] | GrandPerspective, DaisyDisk |
| **P2** | **Higiene de herramientas de IA** | Hueco H9; categoría emergente | Mole for Mac, CleanMyMac CLI |

**Paridad mínima necesaria.** No diferencia, pero sin ella no se compite:
- Limpieza de cachés y logs.
- Desinstalador con restos y Sentinel.
- Explorador visual con grandes, antiguos y duplicados.
- Instaladores sobrantes.
- Monitor básico.
- Elementos de arranque.

### 6.2 Propuesta de valor por persona

**Usuario común.** *"Libera espacio y pon orden sin miedo: ves qué se va a borrar, por qué es seguro, y puedes deshacerlo."*

| Necesidad (*job*) | Módulos | Mensaje |
|---|---|---|
| "Mi Mac está lleno" | Panel/Escaneo rápido, Limpieza del sistema, Explorador de espacio, Instaladores y descargas | Un botón, resultados por categoría y espacio real |
| "No sé qué apps sobran" | Desinstalador de apps (arrastrar y soltar, Sentinel) | Elimina la app y sus restos y conserva tus datos personales |
| "El Escritorio y Descargas son un caos" | Organizador con IA (opcional, local) | Te propone un orden; tú lo apruebas; se puede deshacer |
| "¿Va bien mi Mac?" | Monitor del sistema, Mantenimiento y arranque | Información honesta, sin alarmas ni promesas de velocidad |
| "Me da miedo romper algo" | Historial y deshacer, Ajustes/Reglas | Todo es reversible; las exclusiones te protegen |

**Lo que no se promete:** acelerar el Mac, "liberar RAM" ni sustituir a un antivirus.

**Desarrollador.** *"Recupera decenas de GB de artefactos y cachés sin romper proyectos activos, y descubre qué tienes instalado, qué no usas y qué es vulnerable, desde la GUI o desde la terminal."*

| Necesidad (*job*) | Módulos | Mensaje |
|---|---|---|
| "`node_modules`/`target`/DerivedData se comen el disco" | Zona Desarrollador | Agrupado por proyecto, con los proyectos activos protegidos y git respetado |
| "Tengo cinco versiones de Node, tres de Python y runtimes de simulador" | Zona Desarrollador (toolchains) | Muestra qué usa cada proyecto y qué sobra |
| "¿Tengo algo vulnerable instalado?" | Seguridad de dependencias | Una vista multiecosistema con OSV offline; exportable a SARIF o CycloneDX |
| "Quiero automatizarlo" | Ajustes/Reglas/Programación, CLI/JSON, MCP | La misma lógica en GUI, CLI y agentes; dry-run por defecto |
| "Ollama y los asistentes de IA acumulan GB" | Zona Desarrollador (higiene IA) | Modelos y cachés visibles; sesiones protegidas |

---

## 7. Lecciones de Mole

### 7.1 Qué replicar (como comportamiento funcional, no como código)

| Mecanismo de Mole | Por qué replicarlo | Cómo encaja en rotten-apple |
|---|---|---|
| `--dry-run` en todos los comandos destructivos [1] | Vista previa sin riesgo | La revisión previa es obligatoria en la GUI y `--dry-run` existe en la CLI |
| Whitelist **fail-closed** [1] | Ante la duda, no borrar | Si las exclusiones no pueden leerse, se aborta |
| Log de operaciones + `history` [1] | Auditoría | Es la base del módulo Historial y deshacer |
| Sin sudo salvo necesidad; validación de rutas; protección de ubicaciones del sistema; saltarse lo no verificable [1] | Mínimo privilegio | Helper privilegiado solo para acciones concretas y explicadas |
| `purge`: agrupar por proyecto, desmarcar los proyectos activos en 7 días, proteger repos git anidados, ficheros trackeados y keypairs [1] | Evita romper trabajo en curso | Heurísticas configurables en la Zona Desarrollador |
| `uninstall`: conservar datos personales; listar referencias en configuraciones de shell/MCP **sin editarlas** [1] | Respeta la configuración del usuario | Informe de referencias rotas tras desinstalar |
| Saltarse las cachés de apps abiertas; proteger modelos y sesiones de IA [2] | Evita corrupción y pérdidas | Comprobación de procesos activos; reglas de "protegido por defecto" |
| Tres niveles de riesgo; *ownership matching*; reverificación en el momento de ejecutar [4] | Precisión y defensa frente a condiciones de carrera | Nivel de riesgo visible; verificación justo antes de mover cada elemento |
| `slim` verificando firma y Gatekeeper [1] | No romper apps | Si se implementa, que sea avanzado, reversible y con verificación |
| JSON, Raycast/Alfred y skills para agentes [1] | Ecosistema | CLI JSON + MCP |
| Cadencia de actualización del conocimiento (varias versiones por semana) [2] | Las rutas de las apps cambian constantemente | **Separar el catálogo de reglas del binario** para actualizarlo sin publicar versión (detalle en el doc 05) |

### 7.2 Qué mejorar

| Limitación de Mole | Evidencia | Mejora propuesta |
|---|---|---|
| `clean`, `purge` e `installer` **borran de forma permanente** | README [1] | Papelera o cuarentena por defecto con deshacer; el borrado permanente es opt-in |
| `clean` **vacía ~/.Trash** | README [1] | Nunca vaciar la Papelera de forma implícita: es una acción separada que pide confirmación y muestra lo que contiene |
| Mole for Mac: caché permanente por defecto | mole.fit [3] | Reversible por defecto también en la GUI |
| Solo terminal; la GUI es propietaria | [1][3] | GUI open source de primera clase |
| Modo oscuro forzado en la app (reconocido por el autor) | [4] | Respetar la apariencia del sistema, "reducir movimiento", VoiceOver y Dynamic Type |
| `optimize` modificó una preferencia de Spotlight (corregido en V1.67.0) | [2] | Cada tarea de mantenimiento declara qué cambia y cómo revertirlo; nunca toca preferencias del usuario sin permiso |
| Sin seguridad de dependencias ni IA de organización | [1] | Módulos Seguridad de dependencias y Organizador con IA |
| Proyecto de un único mantenedor (crítica de un competidor) | [7] | Gobernanza abierta y catálogo de reglas comunitario con revisión |
| GPL-3.0 | [1] | MIT, que facilita integraciones y redistribución (manteniendo el clean-room) |

---

## 8. Riesgos de producto y posicionamiento

### 8.1 Riesgos

| Riesgo | Evidencia | Mitigación y posicionamiento |
|---|---|---|
| **Mala reputación de los "cleaners"** | MacKeeper: demanda colectiva (2014) por señalar problemas inexistentes para vender la versión completa; acuerdo de 2 M$ de ZeoBIT sin admitir culpa, con unos 513.000 clientes afectados [35]. Malwarebytes cataloga "Advanced Mac Cleaner" como PUP: usa falsos positivos deliberados para vender [36]. | No usar nunca alarmas, contadores de "problemas" ni miedo. Sin upsell en el núcleo. Lenguaje neutro ("X GB de cachés regenerables"). |
| **Escepticismo de la comunidad Apple** | En Apple Support Communities, **usuarios voluntarios (no Apple)** sostienen que los limpiadores no son necesarios y pueden ser invasivos [34]. La guía oficial de Apple sobre almacenamiento **no menciona** herramientas de terceros [32]. | Posicionarse como **complemento transparente** de Ajustes → Almacenamiento: mostrar lo que macOS ya ofrece y enlazarlo cuando sea suficiente. |
| **Postura de Apple y restricciones de plataforma** | La guía 2.4.5(i) de la App Store exige sandbox [33], incompatible con una limpieza profunda. La 2.3.1(a) castiga el marketing engañoso. OnyX necesita una versión por macOS [13]; DevCleaner depende de estructuras sin documentar [14]. | Distribución directa notarizada (más Homebrew cask). Permisos (Acceso total al disco, helper) mínimos y explicados. Catálogo de reglas validado por versión de macOS, con **bloqueo de acciones no validadas** en sistemas nuevos. |
| **Falsas promesas de "acelerar"** | Macworld: las tareas de rendimiento de CleanMyMac equivalen a Ajustes del Sistema y no se mide ninguna mejora [6]. | No prometer velocidad. Explicar el efecto real de cada tarea. El monitor informa, no "repara". |
| **Pérdida de datos** (el riesgo más grave para un proyecto OSS) | Borrado permanente en la competencia [1][8][11][16]; condiciones de carrera reconocidas por PureMac [8]. | Reversible por defecto, niveles de riesgo, reverificación, tests por regla y modo conservador por defecto. |
| **Confianza en los permisos** | Pearcleaner necesita Acceso total al disco y un helper privilegiado, y sufre webs falsas [9]. | Código abierto, builds reproducibles, firma y notarización, un único canal oficial y un helper con superficie mínima. |
| **IA: privacidad, errores y coste** | Sparkle envía nombres de fichero a la nube [22]; Spark cuesta 100 $/mes [21]. | Opt-in, local-first, plan revisable, deshacer y prohibición de borrar con IA. Nada sale del Mac sin consentimiento explícito (el detalle va en el doc 04). |
| **Alarmismo por vulnerabilidades** | `brew vulns` tuvo que crear su propia base de avisos para evitar falsos positivos por parches retroportados [28]. | Severidad y contexto ("¿se usa?", "¿hay arreglo?"), "omitido ≠ limpio" y ningún mensaje alarmista. |
| **Competencia que se mueve rápido** | Mole (unas 73,5k ★, versiones casi diarias) [1][2]; CleanMyMac CLI (julio de 2026) [7]; Eney (octubre de 2026) [19]; `brew vulns` de serie [28]; Apple Intelligence podría absorber la organización de ficheros **(?)**. | Diferenciarse por reversibilidad, transparencia, GUI OSS y seguridad multiecosistema, no por amplitud bruta. |
| **Plataforma** | macOS 27 solo en Apple silicon (sec.) [37]; Homebrew baja Intel a Tier 3 [28]. | Apple silicon como objetivo principal. Decidir de forma explícita si se da soporte a macOS 26 o 15 (pregunta abierta). |
| **Licencias** | Mole, DevCleaner y GrandPerspective son GPL; AI File Sorter es AGPL [1][12][14][24]. | Clean-room estricto. Incluso con proyectos MIT (kondo, npkill, PureMac), mejor implementación propia o atribución explícita. |
| **Sostenibilidad** | Pearcleaner se paró [9]; a Mole se le critica ser de un único mantenedor [7]. | Gobernanza, contribución abierta al catálogo de reglas y un modelo de financiación claro (pregunta abierta). |

### 8.2 Guía de posicionamiento

| Decir | No decir |
|---|---|
| "Recupera X GB de cachés regenerables. Puedes deshacerlo." | "¡Tu Mac tiene 2.347 problemas!" |
| "Esto es lo que se borrará y por qué es seguro." | "Limpieza profunda en un clic" sin detalle |
| "Mantenimiento: reconstruye el índice de Spotlight (útil si la búsqueda falla)." | "Acelera tu Mac un 300 %" o "libera RAM" |
| "Código abierto, sin telemetría, notarizado por Apple." | Sugerir que Apple respalda la app (guía 5.2.4) [33] |
| "La IA es opcional y funciona en tu Mac." | "IA mágica que organiza todo sola" |

---

## 9. Supuestos y preguntas abiertas (para los docs 02–05 y para el usuario)

**Supuestos:**
- Apple silicon como objetivo y distribución directa (no Mac App Store).
- Núcleo en Rust compartido entre GUI y CLI.
- IA opcional y local por defecto.

**Preguntas abiertas:**
1. **Versión mínima de macOS:** ¿solo 27, o también 26 y 15? Afecta a las APIs de IA on-device y al público alcanzable.
2. **Modelo de sostenibilidad:** ¿donaciones o patrocinios, app gratuita con extras de pago, o 100 % gratis? Afecta a la confianza (sección 8).
3. **¿Borrado permanente alguna vez por defecto?** La propuesta es que nunca: solo como opt-in por categoría.
4. **¿Antimalware dentro del alcance?** La recomendación es que no: ofrecer como mucho una auditoría de configuración, como PureMac.
5. **Pendiente de verificar:**
   - Si `brew vulns` cubre casks.
   - Los precios actuales de Sparkle y Eney.
   - La licencia de LlamaFS.
   - Si atime es una señal de uso fiable en APFS (doc 03).

---

## 10. Fuentes

Consultadas el 2026-11-01. (sec.) indica fuente secundaria.

1. Mole, README: https://github.com/tw93/Mole
2. Mole, releases (V1.66.0, V1.67.0): https://github.com/tw93/Mole/releases
3. Mole for Mac: https://mole.fit/
4. Autor de Mole, "After Releasing the Mac Version of Mole, Users Taught Me How to Build Products" (reproducción): https://youmind.com/landing/x-viral-articles/mole-mac-product-lessons
5. CleanMyMac, página oficial: https://macpaw.com/cleanmymac
6. Macworld, reseña de CleanMyMac (actualizada el 10-oct-2025): https://www.macworld.com/article/352922/cleanmymac-x-review-macos.html
7. MacPaw, "Mole CLI vs CleanMyMac CLI" (blog del competidor): https://cleanmymac.com/blog/mole-review
8. PureMac: https://github.com/momenbasel/PureMac
9. Pearcleaner: https://github.com/alienator88/Pearcleaner
10. AppCleaner: https://freemacsoft.net/appcleaner/
11. DaisyDisk: https://daisydiskapp.com/
12. GrandPerspective: https://grandperspectiv.sourceforge.net/
13. OnyX: https://www.titanium-software.fr/en/onyx.html
14. DevCleaner for Xcode: https://github.com/vashpan/xcode-dev-cleaner
15. npkill: https://github.com/voidcosmos/npkill
16. kondo: https://github.com/tbillington/kondo
17. Hazel: https://www.noodlesoft.com/
18. Hazel 6 (sec.): FAQ https://www.noodlesoft.com/?p=1852 · TidBITS https://tidbits.com/watchlist/hazel-6-1/ · Monodeck https://monodeck.jp/hazel-20th-anniversary-sale/
19. MacPaw, anuncio de Eney: https://macpaw.com/news/eney-announcement
20. 9to5Mac, Eney en beta (2025): https://9to5mac.com/2025/05/22/macpaw-eney-ai-companion-now-in-beta-for-setapp-users/ · Setapp: https://setapp.com/news/what-is-eney
21. iDrop News, Gemini Spark en Mac: https://www.idropnews.com/?p=265891
22. Every, "Introducing Sparkle": https://every.to/p/introducing-sparkle · FutureTools (sec.): https://www.futuretools.io/tools/sparkle · Web del producto (para verificar el precio): https://makeitsparkle.co/
23. Cosmos – AI Files Assistant (Mac App Store): https://apps.apple.com/app/id6759027625
24. AI File Sorter: https://github.com/hyperfield/ai-file-sorter
25. LlamaFS: https://github.com/iyaja/llama-fs · SourcePulse (sec.): https://www.sourcepulse.org/projects/1254971
26. Local-File-Organizer (sec.): https://gittrend.io/repo/QiuYannnn/Local-File-Organizer
27. Marcadores: KeepOrganizedAI https://chromewebstore.google.com/detail/ejcmcpeohpjbipdfhpmfohpdefgbdfpl · AI Bookmark Manager & Organizer https://extensionauditor.com/scan/ai-bookmark-manager-organizer-kepohfgafghldgaebbambekjiabanfac · Stash https://addons.mozilla.org/addon/stash-bookmarks/ · NeuroMark https://addons.mozilla.org/firefox/addon/neuromark/ · MarkAI https://addons.mozilla.org/en-US/firefox/addon/markai/ · Booky https://addons.mozilla.org/addon/booky/ · Raindrop.io https://help.raindrop.io/broken-links
28. Homebrew 7.0.0 y `brew vulns`: Help Net Security https://www.helpnetsecurity.com/2026/09/15/homebrew-7-0-0/ · brew-vulns https://github.com/andrew/brew-vulns · Blog de A. Nesbitt https://nesbitt.io/2026/01/08/brew-vulns-cve-scanning-for-homebrew · PR #23080 https://github.com/Homebrew/brew/pull/23080
29. osv-scanner: https://github.com/google/osv-scanner
30. Stats: https://github.com/exelban/stats
31. Otros limpiadores OSS (sec.): Mac Sai https://awesome.ecosyste.ms/projects/github.com%2Filiyami%2FMacSai · MacOSCleaner https://alternativeto.net/software/macoscleaner/about/ · Reseña de PureMac https://rotek.fr/?p=43300 · OpenAlternative https://openalternative.co/alternatives/cleanmymac
32. Apple Support, "Free up storage space on your Mac": https://support.apple.com/en-us/102624
33. Apple, App Review Guidelines: https://developer.apple.com/app-store/review/guidelines/
34. Apple Support Communities (usuarios voluntarios): https://discussions.apple.com/thread/253183522 · https://discussions.apple.com/thread/250572318
35. Acuerdo colectivo de MacKeeper: AppleInsider https://appleinsider.com/articles/15/08/10/mackeeper-to-pay-out-2m-in-proposed-class-action-settlement · PCWorld https://www.pcworld.com/article/422975/mackeeper-customers-can-file-a-claim-to-get-their-money-back.html
36. Malwarebytes, PUP.Advanced Mac Cleaner: https://www.malwarebytes.com/blog/detections/pup-advanced-mac-cleaner
37. macOS 27 solo en Apple silicon (sec.): MacTrast https://www.mactrast.com/2026/06/psa-remember-macos-27-will-be-apple-silicon-only/amp/ · Gigazine https://www.gigazine.net/gsc_news/en/20260609-macos-27-intel-mac
38. DevCleaner, versión 2.8.0 (sec.): https://softantenna.com/softwares/7632-dev-cleaner-for-xcode
