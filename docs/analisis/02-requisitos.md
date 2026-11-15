# 02 — Visión, personas, casos de uso y requisitos

> **Nota de consolidación (2026-11-01):** las decisiones, el MVP único y las erratas entre documentos están en [`docs/README.md`](../README.md), que prevalece sobre este documento. La revisión crítica está en [06](06-revision-advisor.md). Cambios posteriores del autor:
> - El producto se llama **Rotten Apple**.
> - La limpieza de privacidad (Q-05) y adelgazar apps (Q-06) pasan a **v1**.
> - El CLI tendrá **todos los comandos de Mole** ([07](07-comandos-cli.md)).

| Campo | Valor |
|---|---|
| Producto | rotten-apple (nombre de trabajo) |
| Documento | 02 · Visión, personas, casos de uso, requisitos funcionales del núcleo y no funcionales |
| Versión / estado | 0.1 · borrador para revisión |
| Fecha | 2026-11-01 |
| Plataforma | macOS (versión actual: macOS 27) |
| Relacionados | 03 Zona Desarrollador y vulnerabilidades (RF-DEV, RF-VUL) · 04 Organizador con IA (RF-IA) · 05 Arquitectura (ADR, RNF-ARQ) |

## 0. Convenciones

**Identificadores.** CU-xx casos de uso · RF-PAN panel · RF-LIM limpieza · RF-DES desinstalador · RF-ESP explorador de espacio · RF-INS instaladores · RF-MAN mantenimiento/arranque · RF-MON monitor · RF-SEG seguridad operativa · RF-HIS historial · RF-AJU ajustes/reglas/programación · RF-CLI línea de comandos · RF-UI interfaz gráfica · RNF-xx no funcionales · CAT-xx categorías del catálogo limpiable (§5). RF-DEV, RF-VUL, RF-IA y ADR/RNF-ARQ se definen en los documentos 03, 04 y 05; aquí solo se referencian (§8).

**Prioridad (MoSCoW)** respecto al producto completo: **Must** = sin esto el producto no cumple su promesa; **Should** = importante, se entrega si no compromete un Must; **Could** = deseable; **Won't** = descartado explícitamente por ahora.

**Fase**: **MVP** = primera versión pública (beta) utilizable de punta a punta · **v1** = 1.0 estable · **v2** = posterior. Regla: ningún requisito MVP depende de uno v1/v2.

**Niveles de riesgo** (usados en todo el producto y en la UI con icono + texto, nunca solo color):

| Nivel | Significado | Preselección |
|---|---|---|
| **Seguro** | Se regenera solo o es copia redundante. Peor caso: una app tarda más la próxima vez o pierde estado efímero (posición de ventanas). | Puede preseleccionarse |
| **Precaución** | Normalmente prescindible, pero puede contener estado útil, implicar una re-descarga grande, afectar al funcionamiento de una app o requerir admin. | Nunca |
| **Revisar** | Puede contener datos personales o irrecuperables (copias del iPhone, adjuntos, ficheros grandes del usuario). Se revisa elemento a elemento. | Nunca |
| **Protegido** | No es seleccionable: la ruta está en la lista dura de rutas protegidas (RF-SEG-04). | — |

**Marcas.** "⚠ Verificar" señala un comportamiento de macOS no confirmado para macOS 27; todos se consolidan con su método de verificación en §11.4. Las unidades de tamaño son decimales (1 GB = 10⁹ bytes), igual que Finder. `~` es la carpeta personal del usuario actual.

**Clean-room.** La herramienta de referencia (Mole, GPL-3.0) se usa solo a partir de la descripción pública de sus funcionalidades. No se ha leído ni se portará su código; este repositorio es MIT.

---

## 1. Visión

### 1.1 Declaración

> La forma **más segura y comprensible** de recuperar espacio y mantener sano un Mac: para quien no sabe qué es `~/Library` y para quien tiene 40 proyectos con `node_modules`, tres toolchains y no quiere hacer `rm -rf` a ciegas.

### 1.2 Principios de producto

| ID | Principio | Consecuencia verificable |
|---|---|---|
| P-1 | **Nada se borra sin que lo veas.** | Toda acción destructiva pasa por una previsualización (RF-SEG-01). |
| P-2 | **Todo se puede deshacer**, salvo lo que el usuario pide explícitamente que sea permanente. | Papelera por defecto + historial con deshacer (RF-SEG-02, RF-HIS). |
| P-3 | **Tamaños honestos.** | Se informa del espacio asignado real y se distingue "movido a la Papelera" de "liberado" (RF-SEG-12). |
| P-4 | **Privado y local por defecto.** | Sin telemetría ni conexiones de red con la configuración por defecto (RNF-17). |
| P-5 | **Dos audiencias, un núcleo.** | Modo Simple y modo Experto sobre el mismo motor, reglas y salvaguardas (§3). |
| P-6 | **Sin placebo.** | No hay "liberar RAM" ni "optimizaciones" sin un síntoma que las justifique (RF-MAN-06). |
| P-7 | **La IA propone, el usuario decide.** | La IA no tiene capacidad de borrar; sus propuestas son planes que pasan por RF-SEG (§8). |

### 1.3 Objetivos

| ID | Objetivo |
|---|---|
| O-1 | Que un usuario no técnico libere espacio de forma segura en su primera sesión, sin ayuda. |
| O-2 | Que un desarrollador recupere el espacio de artefactos y cachés de toolchains sin miedo a perder trabajo (detalle en doc 03). |
| O-3 | Cero pérdidas de datos del usuario atribuibles al producto. |
| O-4 | Explicar a dónde va el espacio ("Datos del sistema", copias, instantáneas) en lenguaje claro. |
| O-5 | Ofrecer CLI scriptable con las mismas garantías que la GUI para desarrolladores y personal de IT. |

### 1.4 Métricas de éxito

Al no haber telemetría por defecto (P-4), las métricas se miden con pruebas de usabilidad, beta con informes voluntarios (opt-in) y tests automatizados.

| ID | Métrica | Objetivo | Cómo se mide |
|---|---|---|---|
| M-1 | Tiempo desde primer arranque hasta primera limpieza completada (con FDA concedido) | Mediana ≤ 3 min | Test moderado con 8–10 usuarios P1 |
| M-2 | Acciones destructivas ejecutadas sin previsualización | 0 | Test e2e + auditoría del journal (cada operación referencia un plan confirmado) |
| M-3 | Incidentes de pérdida de documentos del usuario en beta | 0 | Informes de beta, issues |
| M-4 | Elementos enviados a la Papelera que se restauran a su ruta original con "Deshacer" | ≥ 99 % (mientras la Papelera no se haya vaciado) | Test automatizado con árboles de prueba |
| M-5 | Error del "espacio liberado" anunciado frente al medido | ≤ máx(5 %, 200 MB) | Test: delta de capacidad disponible antes/después |
| M-6 | Éxito de tareas sin ayuda (CU-02, CU-03, CU-05) | ≥ 90 % | Test de usabilidad |
| M-7 | Usabilidad percibida | SUS ≥ 75 | Cuestionario SUS tras test |
| M-8 | Escaneo rápido en Mac de referencia | ≤ 10 s p95 (ver RNF-01) | Benchmark en CI y manual |

### 1.5 Qué mejora respecto a la referencia

| Aspecto | Referencia (CLI/TUI) | rotten-apple |
|---|---|---|
| Interfaz | Terminal | App nativa con GUI + CLI con las mismas garantías |
| Borrado en limpieza | Borrado permanente; vacía `~/.Trash` | **Papelera por defecto**; la Papelera nunca se vacía como parte de una limpieza ni automáticamente (salvo opt-in explícito, RF-AJU-09) |
| Deshacer | No | Historial con deshacer por operación y por elemento |
| Tamaños | Sin detalle sobre clones/purgable | Tamaño asignado, hardlinks deduplicados, purgable aparte, "movido" vs "liberado" |
| Audiencias | Usuario técnico | Modo Simple (sin jerga) y modo Experto |
| Desarrolladores | Purga de artefactos | Purga + toolchains + **vulnerabilidades conocidas** en dependencias (doc 03) |
| Organización | — | Organizador con IA opcional (doc 04) |
| Idiomas | Inglés | Español e inglés desde el MVP |
| Licencia | GPL-3.0 | MIT |

### 1.6 Alcance

Módulos: Panel/Escaneo rápido · Limpieza del sistema · Desinstalador de apps · Explorador de espacio (grandes/antiguos/duplicados) · Instaladores y descargas · Mantenimiento y arranque · Monitor del sistema · Zona Desarrollador (doc 03) · Seguridad de dependencias (doc 03) · Organizador con IA (doc 04) · Historial y deshacer · Ajustes/Reglas/Programación. Interfaces: GUI nativa y CLI.

### 1.7 Fuera de alcance

| Fuera de alcance | Motivo |
|---|---|
| Antivirus / detección o eliminación de malware | Otro dominio; las vulnerabilidades de dependencias (doc 03) no son antimalware. |
| "Liberar RAM", `purge`, "acelerar el Mac" | Placebo o contraproducente en macOS (P-6). |
| Editar configuraciones de otras apps o del shell (`~/.zshrc`, PATH) | Riesgo alto; como mucho se **listan** referencias (RF-DES-10). |
| Tocar el volumen del sistema (`/System`, SSV) o pedir desactivar SIP | Imposible o inseguro por diseño de macOS. |
| Eliminar apps de Apple de `/System/Applications` | Están en el volumen sellado. |
| Gestionar contenido de iCloud Drive / File Provider (Dropbox, OneDrive…) | Borrar ahí borra en todos los dispositivos; solo se muestra en el Explorador. |
| Copias de seguridad / sincronización | Existe Time Machine. |
| Borrado seguro (sobrescritura) | No tiene sentido en SSD con APFS/FileVault. |
| Limpieza de privacidad (historial, cookies, contraseñas del navegador) | Fuera del MVP/v1; pregunta abierta Q-05. |
| Eliminar idiomas (`.lproj`) o código Intel de apps ("adelgazar") | Rompe la firma de código y Gatekeeper; ganancia pequeña (CAT-13, RF-DES-13). |
| Gestión de flotas / MDM | Solo se facilita CLI + JSON + configuración exportable (persona P3). |
| Windows / Linux | El núcleo podría ser portable, pero no es objetivo. |

---

## 2. Personas

### 2.1 Personas principales

| | **P1 · Lucía — usuaria común** | **P2 · Diego — desarrollador** |
|---|---|---|
| Perfil | 52 años, administrativa y fotógrafa aficionada. MacBook Air 256 GB. | 34 años, full-stack. MacBook Pro 1 TB. Node, Rust, Python, Docker, Xcode, Homebrew. |
| Nivel técnico | Bajo. Usa Finder y Ajustes; nunca ha abierto Terminal. | Alto. Terminal a diario, scripts propios, Git. |
| Objetivos | Quitar el aviso "Tu disco está casi lleno" sin pagar más iCloud ni cambiar de Mac. Que el Mac "vaya bien". | Recuperar decenas de GB de `node_modules`, `target/`, DerivedData, simuladores e imágenes Docker. Saber qué dependencias tienen vulnerabilidades conocidas. |
| Frustraciones | "Datos del sistema" ocupa 60 GB y no sabe qué es. Apps "limpiadoras" con sustos, anuncios y suscripciones. Ajustes › Almacenamiento no le dice qué borrar. | Scripts `rm -rf` ad hoc; herramientas que borran cachés que luego tarda una hora en reconstruir; no saber qué proyectos siguen vivos. |
| Qué le da miedo | Borrar fotos, documentos, los chats o la copia de su iPhone. Que la app sea malware. | Borrar un proyecto activo, un `.env`, claves SSH, cambios sin commitear o romper una toolchain. |
| Qué necesita | Un botón, lenguaje claro, ver antes qué se borra y saber que puede deshacerlo. | Rutas visibles, dry-run, CLI con `--json`, reglas y exclusiones, protección de repos Git. |
| Modo por defecto | Simple | Experto/Desarrollador |

### 2.2 Persona secundaria

| | **P3 · Marta — power user / IT de una pyme** |
|---|---|
| Perfil | Mantiene 15 Macs de una oficina y los de su familia. |
| Nivel técnico | Alto en administración; no necesariamente programadora. |
| Objetivos | Aplicar las mismas exclusiones y reglas en todos los equipos; ejecutar limpiezas desde scripts; informes en JSON; nada de telemetría. |
| Frustraciones | Herramientas que piden `sudo` para todo; configuraciones no exportables; licencias por equipo. |
| Qué le da miedo | Dejar inutilizable el Mac de un compañero o borrar datos de trabajo de otro usuario. |
| Qué necesita | CLI estable con códigos de salida, configuración TOML importable/exportable, historial exportable, funcionamiento sin admin. |

---

## 3. Modos de uso

Un único interruptor en Ajustes (y una pregunta en el onboarding) cambia **presentación y valores por defecto**, nunca las salvaguardas de RF-SEG.

| Aspecto | Modo Simple (por defecto) | Modo Desarrollador / Experto |
|---|---|---|
| Lenguaje | Sin jerga: "Archivos temporales de apps", "Copias del iPhone" | Nombre llano + ruta: "Cachés de usuario (`~/Library/Caches`)" |
| Rutas y detalles técnicos | Ocultos tras "Ver detalles" | Visibles; copiar ruta; columnas de tamaño lógico/asignado |
| Preselección | Solo categorías **Seguro** | Solo categorías **Seguro** (idéntico: nadie preselecciona Precaución/Revisar) |
| Categorías Precaución / Revisar | Agrupadas en "Más opciones" plegado | Visibles en la lista principal, sin marcar |
| Zona Desarrollador y Seguridad de dependencias | Ocultas; si se detectan toolchains, una sugerencia ofrece activarlas | Visibles en la barra lateral |
| Borrado permanente directo (sin pasar por Papelera) | No disponible; solo "Vaciar Papelera"/"Liberar ahora" (CU-08) | Disponible como acción separada, con advertencia (RF-SEG-03) |
| Mantenimiento | Tareas sin admin con explicación del síntoma que resuelven | Todas, incluidas las que requieren admin (v1) |
| Monitor | Resumen de salud con explicación | Métricas detalladas y procesos |
| Cifras | "Puedes liberar 8,2 GB" | Desglose: asignado, lógico, purgable, nº de elementos |
| Acciones de admin | Ocultas hasta que hacen falta; se explica por qué | Visibles con icono de candado |

**Invariantes en ambos modos:** previsualización, Papelera por defecto, rutas protegidas, exclusiones fail-closed, verificación de identidad, omisión de ficheros en uso, no preselección de documentos, no materialización de iCloud y journal (RF-SEG-01…19).

**Sugerencia de modo:** en el onboarding se propone el modo Experto si existen rutas como `~/.cargo`, `~/.rustup`, `~/.npm`, `~/.nvm`, `~/Library/Developer/Xcode` o un ejecutable `brew`. Solo se comprueba su existencia, nunca dentro de carpetas protegidas por TCC.

---

## 4. Recorridos de usuario y casos de uso

### 4.1 Recorridos clave

| ID | Persona | Recorrido | CU implicados |
|---|---|---|---|
| J-1 | P1 | Ve el aviso de disco lleno → descarga la app → concede permiso → escanea → limpia lo seguro → ve "8 GB en la Papelera" → pulsa "Liberar ahora" → una semana después revisa las copias de su iPhone. | CU-01, CU-02, CU-08, CU-04 |
| J-2 | P1 | Quiere quitar una app de prueba que ya no usa y sus restos sin perder nada más. | CU-03, CU-05 |
| J-3 | P2 | Instala con Homebrew, activa modo Experto, explora qué ocupa el disco, purga artefactos de proyectos antiguos (doc 03), excluye su carpeta de trabajo en curso y programa limpieza semanal. | CU-04, CU-07, CU-06, CU-12 |
| J-4 | P2 | Algo deja de funcionar tras una limpieza → abre Historial → deshace la operación. | CU-05 |
| J-5 | P3 | Exporta exclusiones y reglas a TOML, las importa en otros Macs y lanza `rotten clean --json` desde un script. | CU-07, CU-12 |

### 4.2 Casos de uso

#### CU-01 · Primer arranque y concesión de permisos

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP · **Requisitos:** RF-UI-06, RF-AJU-02, RF-AJU-08, RF-AJU-10, RF-PAN-05

**Precondiciones:** app notarizada y descargada; primer lanzamiento.

**Flujo principal:**
1. El usuario abre la app (Gatekeeper muestra el aviso estándar de app descargada de Internet).
2. Bienvenida (una pantalla): qué hace la app y su promesa: "Nada se borra sin que lo veas. Todo va a la Papelera y se puede deshacer".
3. Pregunta de modo: "¿Usas este Mac para programar?" → Simple / Desarrollador (con sugerencia automática, §3).
4. Pantalla de permisos: explica por qué hace falta **Acceso total al disco** (FDA) y qué categorías quedan bloqueadas sin él (columna "Permisos" de §5). macOS **no permite pedir FDA por programa**: el botón "Abrir Ajustes" abre Ajustes del Sistema › Privacidad y seguridad › Acceso total al disco mediante la URL `x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles` (⚠ Verificar en macOS 27). Una guía ilustrada muestra cómo activar el interruptor o añadir la app con "+".
5. Mientras la pantalla está visible, la app comprueba cada 2 s (máximo 5 min) si ya tiene FDA con una lectura no destructiva de una ruta protegida solo por FDA que **no** provoca diálogos de consentimiento (p. ej. listar `~/Library/Mail` o `~/Library/Safari`).
6. macOS ofrece "Salir y volver a abrir". La app guarda el estado del onboarding y, al relanzarse, muestra "Permiso concedido" y continúa.
7. Pregunta única de red: "¿Comprobar actualizaciones automáticamente?" (Sí preseleccionado, Q-07; es la única conexión posible con la configuración por defecto, RNF-17).
8. Botón "Escanear ahora" → CU-02.

**Alternativos y errores:**
- A1 · "Ahora no": la app funciona en modo limitado. Las categorías que requieren FDA aparecen como "Bloqueada · requiere permiso", **sin estimación de tamaño inventada**. Banner descartable en el Panel.
- A2 · La app se ejecuta desde la imagen DMG o desde Descargas (App Translocation, ruta bajo `/private/var/folders/…/AppTranslocation/`): se pide moverla a `/Applications` antes de conceder FDA, porque el permiso queda ligado a la ruta/firma de la app (⚠ Verificar comportamiento exacto).
- A3 · La app no aparece en la lista de FDA: la guía explica cómo añadirla con "+" desde `/Applications`.
- A4 · Mac gestionado (MDM) que bloquea FDA: tras 5 min sin éxito se muestra ayuda ("Si tu Mac lo gestiona tu organización…") y se continúa en modo limitado.
- A5 · Escritorio/Documentos/Descargas sin FDA: macOS muestra un diálogo de consentimiento por carpeta. La app **no accede** a esas carpetas durante el onboarding ni en el escaneo rápido; solo cuando el usuario abre un módulo que las necesita (Instaladores, Explorador) y después de explicarlo.
- A6 · Permiso revocado más tarde: se re-verifica en cada arranque y antes de cada escaneo; las categorías vuelven a "Bloqueada" sin errores en cascada.
- A7 · Notificaciones: **no** se piden en el onboarding; se piden al activar la programación (CU-06).

**Postcondición:** modo guardado; estado de permisos visible en Ajustes › Permisos; ningún fichero del usuario modificado.

#### CU-02 · Escaneo rápido y limpieza

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP · **Requisitos:** RF-PAN-01…05, RF-LIM-01…08, RF-SEG-01…12, RF-HIS-01

**Precondiciones:** CU-01 completado (con o sin FDA).

**Flujo principal:**
1. Panel → "Escanear" (⌘R).
2. El núcleo recorre solo las raíces del catálogo (§5) aplicables según modo y permisos. Los resultados aparecen por categoría en streaming con su tamaño asignado.
3. Resumen: "Puedes liberar ~8,2 GB de forma segura" (solo lo preseleccionado, categorías Seguro) y "4,1 GB más para revisar" (Precaución/Revisar, sin marcar).
4. Opcional: "Ver detalles" → categoría → app → elemento, con explicación y riesgo; el usuario marca o desmarca.
5. "Limpiar" → **pantalla de revisión** (RF-SEG-01): nº de elementos, tamaño, destino "Papelera", elementos que se omitirán y por qué (apps abiertas), opción "Cerrar estas apps y continuar".
6. El usuario confirma. Para cada elemento, el núcleo revalida su identidad (RF-SEG-07), comprueba que no está en uso (RF-SEG-08), lo mueve a la Papelera y lo registra en el journal (RF-HIS-01).
7. Informe: "Movido a la Papelera: 8,0 GB (1 234 elementos) · Omitidos: 37 (en uso) · [Deshacer]". Se indica que **el espacio se libera al vaciar la Papelera**, con botón "Liberar ahora" (CU-08, solo los elementos de esta operación).

**Alternativos y errores:**
- A1 · Cancelar el escaneo: se detiene en ≤ 1 s y no hay cambios.
- A2 · Un elemento cambió entre la revisión y la ejecución: se omite con motivo "cambió desde la revisión".
- A3 · App abierta: sus cachés se omiten, o el usuario elige "Cerrar y continuar" (se pide salida normal; nunca se fuerza sin confirmación aparte).
- A4 · Error de permisos en un elemento: se omite, se continúa y se informa.
- A5 · Nada que limpiar: estado vacío positivo ("Tu Mac está limpio").
- E1 · La app se cierra a mitad de operación: al relanzar, el journal permite reconciliar (cada elemento está en su sitio o en la Papelera y registrado) y se muestra "Operación interrumpida · revisar".

**Postcondición:** elementos en la Papelera; operación restaurable en Historial; Panel actualizado.

#### CU-03 · Desinstalar una app con sus restos

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP (sistema/admin en v1) · **Requisitos:** RF-DES-01…09, RF-SEG

**Precondiciones:** la app a desinstalar está en `/Applications` o `~/Applications`.

**Flujo principal:**
1. Desinstalador → lista de apps (nombre, icono, versión, tamaño total, último uso si se conoce) o arrastrar la app a la ventana.
2. El usuario elige una app. Si está abierta, se ofrece cerrarla.
3. Se descubren los ficheros asociados por bundle id, nombre y Team ID (rutas de CAT-14) y se agrupan:
   - **App** (`.app`) — marcada.
   - **Cachés, logs, estado guardado, almacenamiento web** — marcados (Seguro).
   - **Agentes de inicio de la app** (`~/Library/LaunchAgents/<id>*.plist`) — marcados (si se dejan, apuntarían a un binario inexistente).
   - **Ajustes y datos de la app** (`Preferences`, `Application Support`, `Containers`, `Group Containers`) — **sin marcar**, con el texto "Consérvalos si piensas reinstalarla".
4. Revisión → confirmación → todo a la Papelera → journal → resumen con "Deshacer".

**Alternativos y errores:**
- A1 · App de Apple en `/System/Applications` o app protegida: no aparece en la lista.
- A2 · App propiedad de root (instalada con `.pkg`) que el usuario no puede mover: MVP → "Mostrar en Finder" para que el usuario la arrastre a la Papelera (Finder pedirá contraseña); v1 → helper privilegiado (CU-13).
- A3 · `Group Container` compartido con otra app instalada del mismo Team ID (p. ej. suites de oficina): se marca "Compartido con X" y no se puede seleccionar.
- A4 · La app incluye extensiones del sistema, extensiones de red, kexts o un desinstalador propio: aviso y recomendación de usar el desinstalador del fabricante; no se tocan extensiones del sistema.
- A5 · App instalada con Homebrew Cask: en modo Experto se sugiere `brew uninstall --cask <nombre>` para no dejar metadatos de Homebrew desfasados (detalle en doc 03).
- A6 · Mover el bundle requiere el permiso "Gestión de apps" de macOS: se explica y se enlaza al panel (⚠ Verificar si aplica a mover a la Papelera).

**Postcondición:** app y restos seleccionados en la Papelera; operación restaurable.

#### CU-04 · Encontrar qué ocupa espacio

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP (duplicados/antiguos en v1) · **Requisitos:** RF-ESP-01…08

**Precondiciones:** ninguna (sin FDA, algunas carpetas aparecerán como "sin acceso").

**Flujo principal:**
1. Explorador → elegir volumen o carpeta (por defecto, la carpeta personal).
2. Si el destino incluye Escritorio/Documentos/Descargas y no hay FDA, se avisa antes de que macOS muestre sus diálogos.
3. Escaneo con resultados en streaming: lista jerárquica ordenada por tamaño asignado (y mapa visual en v1); panel "Más grandes" con los 50 mayores.
4. El usuario navega, abre Quick Look o "Mostrar en Finder".
5. Opcional: selecciona elementos (**nada viene marcado**) → "Mover a la Papelera" → revisión (RF-SEG-01) → journal.

**Alternativos y errores:**
- A1 · Ficheros de iCloud no descargados: aparecen como "En iCloud · 0 B en este Mac" y nunca se descargan (RF-SEG-10).
- A2 · Subcarpetas sin permiso: "Sin acceso (requiere permiso)", sin tamaño inventado.
- A3 · Volumen externo expulsado durante el escaneo: resultados parciales marcados como incompletos.
- A4 · El usuario intenta seleccionar algo protegido: la casilla está deshabilitada con explicación.
- A5 · Paquetes (`.app`, `.photoslibrary`): se muestran como una unidad; en Simple no se puede entrar en ellos.

**Postcondición:** el usuario sabe qué ocupa el espacio; si movió algo, la operación es restaurable.

#### CU-05 · Deshacer una limpieza

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP · **Requisitos:** RF-HIS-01…05, RF-UI-10

**Precondiciones:** existe una operación en Historial cuyos elementos siguen en la Papelera.

**Flujo principal:**
1. Inmediatamente tras la operación: aviso "Deshacer" (⌘Z) durante 30 s. Después: Historial → operación.
2. Historial muestra fecha, módulo, origen (manual/programada/CLI), espacio y estado: Restaurable / Parcialmente restaurable / Irreversible.
3. El usuario elige "Restaurar todo" o elementos concretos → previsualización de la restauración (qué vuelve a dónde, conflictos).
4. Confirmación → cada elemento vuelve de la Papelera a su ruta original → nueva entrada de journal "Restauración".

**Alternativos y errores:**
- A1 · Elemento ya no está en la Papelera (vaciada o movido por el usuario): se marca "Irreversible" con motivo; el resto se restaura.
- A2 · La ruta original ya existe (la app regeneró la caché): se ofrece "Conservar ambos" (sufijo " (restaurado)"), "Sustituir" (lo actual va a la Papelera) u "Omitir". Por defecto: Omitir para cachés y Conservar ambos para el resto.
- A3 · La carpeta padre ya no existe: se recrea con el propietario y permisos registrados.
- A4 · Volumen de origen no montado: se pospone esos elementos con aviso.
- A5 · Operación de borrado permanente: aparece como "Irreversible" desde el principio; no hay botón de deshacer.

**Postcondición:** elementos de vuelta en su ruta; la operación original queda marcada como restaurada (total o parcialmente).

#### CU-06 · Programar limpieza automática

**Actor:** P1, P2, P3 · **Prioridad:** Should · **Fase:** v1 · **Requisitos:** RF-AJU-03, RF-SEG-14, RNF-08

**Precondiciones:** al menos una limpieza manual completada (para que el usuario haya visto qué incluye cada categoría).

**Flujo principal:**
1. Ajustes › Programación → frecuencia (semanal/mensual, día y hora).
2. Elegir categorías: **solo son elegibles las de nivel Seguro**, cada una marcada explícitamente por el usuario.
3. Condiciones (por defecto): solo conectado a corriente, no en Modo de bajo consumo, Mac inactivo.
4. Se piden permisos de notificaciones.
5. Se registra un agente de launchd mediante la API de ítems de inicio del sistema (detalle en doc 05). macOS muestra su aviso "Ítem en segundo plano añadido".
6. En cada ejecución: escaneo → plan auto-aprobado solo para las reglas preaprobadas → Papelera → journal (origen "programada") → notificación con resumen y "Deshacer".

**Alternativos y errores:**
- A1 · El usuario desactiva el ítem en Ajustes del Sistema › General › Ítems de inicio: la app detecta el estado y muestra "Programación desactivada en Ajustes del Sistema".
- A2 · Freno de anomalías: si el plan supera 2× la mediana de las 5 ejecuciones anteriores o 20 GB, no se ejecuta; se notifica "Revisa esta limpieza".
- A3 · Sin FDA en el contexto del agente: se limpian solo las categorías accesibles y se informa (⚠ Verificar atribución TCC del agente).
- A4 · Mac apagado o en reposo a la hora programada: se ejecuta en la siguiente oportunidad que cumpla las condiciones.
- A5 · Fichero de exclusiones ilegible: la ejecución aborta (fail-closed) y se notifica.

**Postcondición:** limpieza periódica activa, visible en Ajustes y en Historial.

#### CU-07 · Excluir rutas

**Actor:** P2, P3 (también P1) · **Prioridad:** Must · **Fase:** MVP · **Requisitos:** RF-AJU-01, RF-SEG-05

**Flujo principal:**
1. Desde cualquier resultado: menú contextual "No tocar nunca" (ruta, carpeta, app por bundle id o categoría). O bien Ajustes › Exclusiones › "+" (selector de carpeta o patrón glob).
2. La exclusión se aplica de inmediato: los elementos desaparecen de los resultados actuales.
3. Se persiste en la configuración y afecta a GUI, CLI y programación.

**Alternativos y errores:**
- A1 · Patrón inválido: error en línea, no se guarda.
- A2 · Exclusión de algo ya protegido: se informa de que ya está protegido.
- A3 · Fichero de exclusiones corrupto o ilegible: **todas las acciones destructivas se bloquean** hasta corregirlo (fail-closed), con opción de abrirlo o restaurar la última versión válida.
- A4 · Importar exclusiones desde TOML (P3): se muestra un diff antes de aplicar.

**Postcondición:** la ruta o patrón nunca vuelve a aparecer como seleccionable.

#### CU-08 · Vaciar Papelera / "Liberar ahora" (borrado permanente explícito)

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP · **Requisitos:** RF-SEG-03, RF-HIS-05

**Flujo principal:**
1. Desde el informe de una limpieza ("Liberar ahora") o desde Panel › Papelera.
2. Se muestra el contenido por volumen, distinguiendo "enviado por rotten-apple (operación X)" de "otros elementos".
3. El usuario elige: solo los elementos de esta operación / todos los de rotten-apple / toda la Papelera.
4. Confirmación específica: "Se eliminarán definitivamente N elementos (X GB). **No se puede deshacer.**" El botón destructivo no es el predeterminado (Intro no lo activa).
5. Borrado → journal (operación irreversible) → espacio liberado medido.

**Alternativos y errores:**
- A1 · Leer `~/.Trash` requiere FDA (⚠ Verificar): sin FDA, solo se ofrece vaciar los elementos que la app conoce por su journal, o abrir la Papelera en Finder.
- A2 · Elementos de otros volúmenes (`/Volumes/<X>/.Trashes/<uid>`): se listan por volumen.
- A3 · Elemento bloqueado o en uso: se omite y se informa.

**Postcondición:** espacio liberado; las operaciones afectadas pasan a "Irreversible" en Historial.

#### CU-09 · Limpiar instaladores descargados

**Actor:** P1, P2 · **Prioridad:** Must · **Fase:** MVP (fuentes adicionales en v1) · **Requisitos:** RF-INS-01…08

**Flujo principal:**
1. Instaladores → la app explica que va a revisar Descargas y Escritorio (sin FDA, macOS pedirá permiso por carpeta).
2. Lista de `.dmg`, `.pkg`, `.mpkg`, `.iso`, `.xip` (y `.zip` con app dentro en v1): nombre, tamaño, fecha, origen (URL de descarga si macOS la registró) y "App ya instalada" cuando se puede deducir.
3. **Nada viene marcado.** Acceso rápido: "Seleccionar los de apps ya instaladas".
4. Revisión → Papelera → journal.

**Alternativos y errores:**
- A1 · Imagen montada actualmente: se omite ("en uso").
- A2 · Fichero en iCloud sin descargar: "En iCloud"; no cuenta como espacio local y se advierte de que borrarlo lo borra en todos los dispositivos.
- A3 · Instalador de macOS (`/Applications/Install macOS *.app`): nivel Precaución, con explicación.

**Postcondición:** instaladores seleccionados en la Papelera.

#### CU-10 · Revisar elementos de arranque y en segundo plano

**Actor:** P1, P2 · **Prioridad:** Should · **Fase:** v1 (listado de solo lectura en MVP) · **Requisitos:** RF-MAN-01…03

**Flujo principal:**
1. Mantenimiento › Arranque → lista de LaunchAgents/LaunchDaemons y, en lo posible, ítems de inicio: etiqueta, app propietaria, firmante (Team ID), estado (cargado o no), "huérfano" si el ejecutable no existe.
2. El usuario elige "Desactivar" en un agente de usuario → explicación del efecto.
3. Se descarga el agente (`launchctl bootout gui/<uid>/<label>`) y su plist va a la Papelera → journal.

**Alternativos y errores:**
- A1 · LaunchDaemon o agente en `/Library`: requiere admin (v1, helper; CU-13).
- A2 · Ítem gestionado por la API moderna de ítems de inicio de otra app: no se puede modificar desde fuera; se enlaza a Ajustes del Sistema › General › Ítems de inicio y extensiones (⚠ Verificar nombre y URL del panel en macOS 27).
- A3 · Deshacer: el plist vuelve a su sitio y se recarga (`launchctl bootstrap gui/<uid> <plist>`).

**Postcondición:** agente desactivado de forma reversible.

#### CU-11 · Restos de apps ya desinstaladas

**Actor:** P1, P2 · **Prioridad:** Should · **Fase:** v1 · **Requisitos:** RF-LIM-13

**Flujo principal:** Limpieza › "Restos de apps desinstaladas" → lista de carpetas y ficheros cuyo bundle id no corresponde a ninguna app instalada, con nivel de confianza → nada marcado → revisión → Papelera.

**Alternativos:** falso positivo (app en un disco externo, en `~/Applications`, en Setapp o que se reinstalará) → "No tocar nunca" (CU-07). Datos en `Application Support`/`Containers` → nivel Revisar.

#### CU-12 · Uso desde la línea de comandos

**Actor:** P2, P3 · **Prioridad:** Must · **Fase:** MVP (subconjunto) · **Requisitos:** RF-CLI-01…11

**Flujo principal (nombre de comando provisional `rotten`, Q-03):**
1. `rotten scan --json` → resultados por categoría con esquema versionado.
2. `rotten clean --dry-run` → plan exacto que se ejecutaría.
3. `rotten clean --category user-caches` → muestra el plan y pide confirmación interactiva; con `--yes` no pregunta.
4. `rotten history` / `rotten undo <op-id>`.

**Alternativos y errores:**
- A1 · Sin TTY y sin `--yes`: aborta con código de salida de "abortado por seguridad".
- A2 · Ejecutado como root: rechaza salvo `--allow-root` explícito (con root, `$HOME` y el ámbito cambian).
- A3 · El terminal no tiene FDA: `rotten doctor` lo detecta y explica cómo concederlo (el CLI hereda los permisos TCC del terminal; ⚠ Verificar).
- A4 · Borrado permanente: requiere `--permanent` y confirmación interactiva, o `--permanent --yes`.

**Postcondición:** la operación aparece en el Historial de la GUI y es deshacible igual que las de la GUI.

#### CU-13 · Operación que requiere privilegios de administrador

**Actor:** P2, P3 · **Prioridad:** Should · **Fase:** v1 · **Requisitos:** RF-SEG-13, RF-LIM-09, RF-LIM-10; diseño en doc 05

**Flujo principal:**
1. El usuario selecciona algo que requiere admin (cachés de `/Library/Caches`, instantáneas locales de Time Machine, LaunchDaemon). Se marca con candado.
2. La primera vez se explica qué es el ayudante privilegiado y qué puede (y no puede) hacer; macOS pide aprobarlo en Ajustes del Sistema y autenticación de administrador.
3. El ayudante ejecuta solo operaciones tipadas de una lista cerrada, aplicando las mismas comprobaciones de RF-SEG → journal.

**Alternativos y errores:** el usuario rechaza la autorización → esos elementos se omiten; versión del ayudante desfasada → se reinstala tras confirmación; en MVP no existe ayudante y estos elementos aparecen como "Requiere admin · disponible en próximas versiones" o con alternativa manual (Finder, comando sugerido).

**Postcondición:** operación privilegiada registrada en el journal, igual que las de usuario.

---

## 5. Catálogo de categorías limpiables (usuario común)

Las categorías de desarrollo (Xcode, simuladores, Homebrew, npm, cargo, Docker…) están en el doc 03 y usan el mismo pipeline y niveles de riesgo.

**Leyenda de permisos:** **FDA** = requiere Acceso total al disco · **Admin** = requiere privilegios de administrador (ayudante, v1) · **TCC carpeta** = sin FDA, macOS pide consentimiento por carpeta · — = ninguno.

| ID | Categoría (nombre en modo Simple) | Rutas reales | Riesgo | Permisos | Preselección | Fase | Notas y exclusiones |
|---|---|---|---|---|---|---|---|
| CAT-01 | Archivos temporales de apps | `~/Library/Caches/*` | Seguro | — (algunas subcarpetas de Apple: FDA) | Sí | MVP | Se excluyen apps en ejecución y cachés con estado (lista versionada a validar ⚠): `com.apple.bird`, `CloudKit`, `com.apple.akd`, `com.apple.nsurlsessiond` (descargas en curso), `FamilyCircle`, `com.apple.HomeKit`. `Homebrew` → doc 03. |
| CAT-02 | Temporales de apps aisladas | `~/Library/Containers/*/Data/Library/Caches/*`, `~/Library/Group Containers/*/Library/Caches/*` | Seguro | FDA (desde macOS 14 el acceso a datos de otras apps está protegido ⚠) | Sí | MVP | Excluir contenedores de apps abiertas. Solo la subcarpeta `Caches`, nunca el resto del contenedor. |
| CAT-03 | Registros e informes de fallos | `~/Library/Logs/*` (incluye `DiagnosticReports`) | Seguro | — | Sí | MVP | Antigüedad mínima por defecto: 7 días (evita ficheros que se están escribiendo). |
| CAT-04 | Registros del sistema | `/Library/Logs/*`, `/Library/Logs/DiagnosticReports/*`, `/private/var/log/*.gz` (solo rotados) | Precaución | Admin | No | v1 | Nunca `/private/var/db/diagnostics` ni `/private/var/db/uuidtext` (log unificado, Protegido). |
| CAT-05 | Temporales de la sesión | `$(getconf DARWIN_USER_TEMP_DIR)` → `/private/var/folders/<xx>/<id>/T/` (en el equipo de análisis: `/var/folders/1_/mwtq97pn29302rn4hq38dzc00000gn/T/`) | Seguro (solo elementos ≥ 3 días y no en uso) | — | Sí | MVP | Contiene sockets y ficheros vivos; nunca se borra la carpeta raíz. macOS ya purga parte al arrancar: ganancia moderada. |
| CAT-06 | Cachés de sistema del usuario | `$(getconf DARWIN_USER_CACHE_DIR)` → `/private/var/folders/<xx>/<id>/C/` | Precaución | — | No | v1 | Contiene cachés de Metal, Quick Look, Launch Services… Solo subcarpetas en lista permitida; miniaturas Quick Look vía RF-MAN-05. |
| CAT-07 | Cachés del sistema | `/Library/Caches/*` | Precaución | Admin | No | v1 | Excluir `com.apple.*` salvo lista permitida ⚠. |
| CAT-08 | Cachés de navegadores | Safari: `~/Library/Caches/com.apple.Safari`, `~/Library/Containers/com.apple.Safari/Data/Library/Caches`. Chrome: `~/Library/Caches/Google/Chrome/*`, `~/Library/Application Support/Google/Chrome/<perfil>/{Code Cache,GPUCache}`. Firefox: `~/Library/Caches/Firefox/Profiles/*/cache2`. Edge: `~/Library/Caches/Microsoft Edge`. Brave: `~/Library/Caches/BraveSoftware`. Arc: `~/Library/Caches/Arc` ⚠ | Seguro (con el navegador cerrado) | Safari: FDA | Sí | MVP (Safari, Chrome, Firefox) · v1 (resto) | **Nunca**: `Cookies`, `History`, `Login Data`, `Bookmarks`, `Local Storage`, `IndexedDB`, `Sessions`. `Service Worker/CacheStorage` = Precaución (datos sin conexión de webs). |
| CAT-09 | Cachés de apps de mensajería y Electron | `~/Library/Application Support/<App>/{Cache,Code Cache,GPUCache}` (Slack, Discord, VS Code…), `~/Library/Containers/com.microsoft.teams2/Data/Library/Caches` ⚠ | Seguro (con la app cerrada) | Contenedores: FDA | Sí | v1 | Lista de apps y subcarpetas versionada; nunca el resto de `Application Support/<App>`. Descargas sin conexión (p. ej. música) = Revisar. |
| CAT-10 | Estado guardado de ventanas | `~/Library/Saved Application State/*.savedState` | Seguro | — | Sí | MVP | Solo apps cerradas. Efecto: las apps no reabren sus ventanas anteriores. |
| CAT-11 | Adjuntos de Mail abiertos | `~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads/*` | Precaución | FDA | No | v1 | Son copias, pero si el usuario editó y guardó un adjunto abierto desde Mail, el cambio vive aquí. |
| CAT-12 | Adjuntos guardados en Mail | `~/Library/Mail/V*/<cuenta>/**/Attachments/` (el segmento `V*` cambia con la versión de macOS ⚠) | Revisar | FDA | No | v2 | En IMAP se vuelven a descargar; en POP o "En mi Mac" se pierden. |
| CAT-13 | Idiomas no usados | `/Applications/*.app/Contents/Resources/*.lproj` | No soportado | — | — | Won't | Modificar el bundle rompe el sello de la firma de código; Gatekeeper puede bloquear la app y fallan las actualizaciones. Apps de Apple: volumen sellado. |
| CAT-14 | Restos de apps (desinstalador y huérfanos) | `~/Library/Application Support/<id·nombre>`, `~/Library/Preferences/<id>.plist`, `~/Library/Preferences/ByHost/<id>.*.plist`, `~/Library/Containers/<id>`, `~/Library/Group Containers/<TeamID>.<id>`, `~/Library/Caches/<id>`, `~/Library/HTTPStorages/<id>`, `~/Library/WebKit/<id>`, `~/Library/Saved Application State/<id>.savedState`, `~/Library/Logs/<nombre>`, `~/Library/Cookies/<id>.binarycookies`, `~/Library/LaunchAgents/<id>*.plist`, `/Library/LaunchAgents/<id>*.plist`, `/Library/LaunchDaemons/<id>*.plist`, `/Library/PrivilegedHelperTools/<id>*`, `/Library/Application Support/<nombre>`, recibos `/private/var/db/receipts/<pkgid>.{bom,plist}` | Seguro (cachés, logs, estado, HTTPStorages, WebKit) · Precaución (Preferences, LaunchAgents) · Revisar (Application Support, Containers, Group Containers) | Containers, Cookies: FDA · `/Library/*`: Admin | Desinstalador: según CU-03 · Huérfanos: No | MVP (usuario) · v1 (sistema y huérfanos) | Coincidencia por bundle id, nombre y Team ID; un `Group Container` compartido con otra app instalada no es seleccionable. |
| CAT-15 | Copias de seguridad del iPhone/iPad | `~/Library/Application Support/MobileSync/Backup/<UDID>/` | Revisar | FDA | No | v1 | Mostrar dispositivo, fecha y si está cifrada (desde `Info.plist`/`Manifest.plist` de cada copia ⚠). Puede ser la única copia del dispositivo: aviso reforzado en la más reciente. |
| CAT-16 | Actualizaciones e instaladores descargados | `/Library/Updates/*` (en macOS recientes suele contener solo metadatos: en el equipo de análisis, `index.plist` y `ProductMetadata.plist`), `/Applications/Install macOS *.app`, `~/Library/iTunes/iPhone Software Updates/*.ipsw` ⚠ | Seguro (`.ipsw`) · Precaución (instalador de macOS, `/Library/Updates`) | `/Library/Updates`: Admin (¿SIP? ⚠) | No | v1 | Las actualizaciones preparadas del sistema en `/System/Library/AssetsV2/…` son Protegido. |
| CAT-17 | Instantáneas locales de Time Machine | Listado: `tmutil listlocalsnapshots /` (funciona sin admin, comprobado), `diskutil apfs listSnapshots /` | Precaución | Borrado: Admin (`tmutil deletelocalsnapshots <fecha>`, `tmutil thinlocalsnapshots / <bytes> <urgencia>`) ⚠ | No | v1 | macOS las elimina solo cuando falta espacio y cuentan como purgable. Solo `com.apple.TimeMachine.*`; nunca instantáneas de actualización del sistema ni de terceros. |
| CAT-18 | Papelera(s) | `~/.Trash`, `/Volumes/<X>/.Trashes/<uid>/` | Revisar (borrado permanente) | `~/.Trash`: FDA ⚠ | Nunca | MVP | Solo mediante CU-08. Nunca en limpieza ni en programación. |
| CAT-19 | Instaladores en Descargas y Escritorio | `~/Downloads/*.{dmg,pkg,mpkg,iso,xip}`, `~/Desktop/…` | Revisar | TCC carpeta (o FDA) | No | MVP | Módulo Instaladores (RF-INS), nunca en Limpieza. |

**Excluido de la limpieza (solo visible en el Explorador, nunca seleccionable por categorías):** `~/Library/Messages` (adjuntos de Mensajes), bibliotecas `*.photoslibrary`, `~/Music/Music`, `~/Movies`, `~/Documents`, `~/Desktop`, `~/Downloads` (salvo RF-INS y doc 04 con selección explícita), `~/Library/Mobile Documents` (iCloud Drive), `~/Library/CloudStorage` (File Provider), `/private/var/vm` (swap y `sleepimage`), `/private/var/db/*`, `~/Library/Keychains`, `~/.ssh`, `~/.gnupg`. Lista dura completa en RF-SEG-04.

**Cobertura según permisos.** Sin FDA, el escaneo rápido MVP cubre CAT-01 (parcial), CAT-03, CAT-05, CAT-08 (Chrome, Firefox) y CAT-10. Con FDA añade CAT-02, CAT-08 (Safari), CAT-11, CAT-15 y la lectura completa de CAT-18.

**Cómo verificar la columna "Permisos":** desde un Terminal **sin** FDA y fuera de cualquier sandbox, `ls <ruta>`; "Operation not permitted" indica protección TCC. Las pruebas de este análisis se hicieron dentro de un sandbox de herramientas y no sirven como evidencia de TCC.

---

## 6. Requisitos funcionales por módulo (núcleo)

Formato: ID · descripción · prioridad · fase · criterio de aceptación verificable.

### 6.1 Panel y escaneo rápido (RF-PAN)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-PAN-01 | Mostrar estado del volumen de arranque: capacidad, usado, libre, purgable (aparte) y disponible. | Must | MVP | "Disponible" coincide con Finder y "libre" con `diskutil info /` (± 1 %); el purgable aparece separado y etiquetado. |
| RF-PAN-02 | Botón único "Escanear" que recorre solo las raíces del catálogo aplicables a modo y permisos. | Must | MVP | Con el corpus de referencia (RNF-01) termina en ≤ 10 s p95; no accede a Escritorio, Documentos ni Descargas (sin diálogos TCC; traza de accesos vacía para esas rutas). |
| RF-PAN-03 | Resultados por categoría: nombre, explicación llana, riesgo, nº de elementos y tamaño asignado, en streaming. | Must | MVP | Las categorías MVP muestran los 5 campos; el primer resultado aparece en ≤ 1 s. |
| RF-PAN-04 | Resumen con dos cifras: "seguro (preseleccionado)" y "para revisar". | Must | MVP | La cifra "seguro" es la suma del tamaño asignado de lo preseleccionado, deduplicada por inode (test con fixture). |
| RF-PAN-05 | Categorías bloqueadas por permisos como "Bloqueada · requiere permiso", con acceso a CU-01. | Must | MVP | Sin FDA, no muestran tamaño ni error; tras conceder FDA y re-escanear, aparecen con su tamaño. |
| RF-PAN-06 | Cancelar el escaneo en cualquier momento sin efectos. | Must | MVP | Cancelación efectiva en ≤ 1 s; árbol fixture idéntico (hash) antes y después. |
| RF-PAN-07 | Acceso a la Papelera con su tamaño por volumen y lo enviado por la app. | Should | MVP | Con FDA, el tamaño coincide con `du -sk` del contenido (± 1 %); sin FDA, se muestra solo lo registrado en el journal y se indica. |
| RF-PAN-08 | Recomendaciones contextuales ("3 copias de iPhone de 2023, 45 GB · revisar"). | Should | v1 | ≥ 5 reglas de recomendación con test; ninguna preselecciona nada. |
| RF-PAN-09 | Última limpieza y espacio liberado acumulado. | Should | v1 | Los valores coinciden con la suma calculada desde el journal. |

### 6.2 Limpieza del sistema (RF-LIM)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-LIM-01 | Catálogo (§5) como reglas declarativas versionadas: raíz, patrón, antigüedad mínima, riesgo, permisos, condición "app cerrada", explicación es/en. Embebido en la app firmada. | Must | MVP | Cada CAT MVP tiene fichero de regla y test con árbol fixture; añadir una categoría no exige cambiar el motor. |
| RF-LIM-02 | Selección granular: categoría → app/subgrupo → elemento. | Must | MVP | Desmarcar un subgrupo excluye todos sus elementos del plan (test). |
| RF-LIM-03 | Preselección solo de categorías Seguro en ambos modos. | Must | MVP | Test sobre todo el catálogo: ningún elemento Precaución/Revisar aparece marcado tras escanear. |
| RF-LIM-04 | Explicación por categoría: qué es, qué pasa si se borra, si se regenera y por qué tiene ese riesgo. | Must | MVP | 100 % de categorías con texto es/en revisado. |
| RF-LIM-05 | Antigüedad mínima por categoría (temporales 3 días, logs 7 días), editable en Experto. | Should | MVP | Elementos más recientes que el umbral no aparecen (test con mtime simulado). |
| RF-LIM-06 | Omitir elementos de apps en ejecución u ofrecer cerrarlas (salida normal). | Must | MVP | Con la app fixture abierta, sus cachés aparecen como "omitidas · app abierta". |
| RF-LIM-07 | Ejecutar solo mediante el pipeline seguro: plan → revisión → verificación → Papelera → journal → informe. | Must | MVP | Test e2e; ver RF-SEG-01, 02 y 07. |
| RF-LIM-08 | Informe final: movido a Papelera, liberado (medido), omitidos con motivo, errores. | Must | MVP | Movidos + omitidos + errores = elementos del plan. |
| RF-LIM-09 | Cachés y logs del sistema (CAT-04, CAT-07) vía ayudante privilegiado. | Should | v1 | Sin ayudante no se ofrecen; con él, cada elemento pasa las mismas comprobaciones RF-SEG (test). |
| RF-LIM-10 | Instantáneas locales de Time Machine: listar con fecha, explicar, eliminar las seleccionadas. | Should | v1 | Solo se ofrecen `com.apple.TimeMachine.*`; tras borrar, `tmutil listlocalsnapshots /` ya no las lista. |
| RF-LIM-11 | Copias de iPhone/iPad (CAT-15): dispositivo, fecha, tamaño, cifrada; mover a Papelera las seleccionadas. | Should | v1 | Nunca preseleccionadas; la más reciente de cada dispositivo muestra aviso adicional. |
| RF-LIM-12 | Cachés de navegadores (CAT-08) solo con el navegador cerrado. | Must | MVP | Con un perfil fixture de Chrome y Firefox, tras limpiar, `Cookies`, `History`, `Login Data`, `Bookmarks`, `Local Storage` quedan intactos (hash). |
| RF-LIM-13 | Restos de apps desinstaladas (CU-11) con nivel de confianza. | Should | v1 | Set de 20 apps desinstaladas: ≥ 90 % de restos detectados y 0 falsos positivos de apps instaladas (incluidas `~/Applications` y volúmenes externos montados). |
| RF-LIM-14 | Cachés de apps Electron y mensajería (CAT-09) según lista versionada. | Should | v1 | Solo se tocan las subcarpetas listadas por app (test con fixture). |
| RF-LIM-15 | Adjuntos de Mail abiertos (CAT-11) e instaladores de macOS/firmware iOS (CAT-16). | Could | v1 | Nivel según §5; nunca preseleccionados. |
| RF-LIM-16 | Reglas de limpieza definidas por el usuario. | Could | v2 | Ver RF-AJU-05. |
| RF-LIM-17 | Actualización remota del catálogo de reglas sin actualizar la app. | Won't | — | Las reglas destructivas solo llegan con actualizaciones firmadas de la app (RF-SEG-17). |

### 6.3 Desinstalador de apps (RF-DES)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-DES-01 | Listar apps de `/Applications` (con subcarpetas) y `~/Applications`: nombre, icono, versión, tamaño total (bundle + restos), origen (App Store, Developer ID, Homebrew Cask). | Must | MVP | Coincide con las apps del fixture; las de `/System/Applications` no aparecen. |
| RF-DES-02 | Último uso de cada app si macOS lo registra (`kMDItemLastUsedDate`); "desconocido" si falta. | Should | v1 | Nunca se muestra una fecha inventada; se puede ordenar por último uso. |
| RF-DES-03 | Descubrir ficheros asociados por bundle id, nombre y Team ID en las rutas de CAT-14. | Must | MVP | Set de 20 apps populares inventariadas a mano: recall ≥ 95 %; precisión 100 % en lo preseleccionado. |
| RF-DES-04 | Preseleccionar solo app, cachés, logs, estado guardado, almacenamiento web y agentes de usuario de la app; ajustes y datos, sin marcar. | Must | MVP | Test: `Application Support`, `Containers`, `Group Containers` y `Preferences` nunca marcados por defecto. |
| RF-DES-05 | Comprobar si la app está abierta y ofrecer cerrarla. | Must | MVP | No se mueve ningún fichero de una app abierta (test). |
| RF-DES-06 | Mover app y restos a la Papelera con journal y deshacer. | Must | MVP | Tras restaurar, la app vuelve a su ruta y abre correctamente (test con app firmada de prueba). |
| RF-DES-07 | Agentes de la app: `launchctl bootout` y plist a la Papelera. MVP: `~/Library/LaunchAgents`. v1: `/Library/LaunchAgents`, `/Library/LaunchDaemons`, `/Library/PrivilegedHelperTools` vía ayudante. | Must | MVP · v1 | Tras desinstalar, `launchctl print gui/<uid>/<label>` no encuentra el servicio. |
| RF-DES-08 | Detectar `Group Containers` compartidos con otra app instalada del mismo Team ID y bloquear su selección. | Must | MVP | Test con dos apps fixture del mismo Team ID. |
| RF-DES-09 | Apps propiedad de root sin ayudante: ofrecer "Mostrar en Finder" en lugar de escalar privilegios. | Must | MVP | Ninguna operación privilegiada en MVP. |
| RF-DES-10 | Listar (sin editar) referencias a la app en configuraciones (`~/.zshrc`, `/etc/paths.d/*`…). | Could | v1 | Solo lectura: ficheros sin cambios (hash). |
| RF-DES-11 | Detectar extensiones del sistema, de red, kexts y desinstaladores del fabricante; advertir y no tocar extensiones. | Should | v1 | Una app con `Contents/Library/SystemExtensions` muestra el aviso. |
| RF-DES-12 | "Restablecer app": eliminar ajustes y datos manteniendo la app, con selección explícita. | Should | v1 | Tras restablecer, la app arranca como recién instalada; deshacer recupera sus datos. |
| RF-DES-13 | Adelgazar apps universales (eliminar código de otra arquitectura). | Won't | — | Modifica bundles firmados, requiere "Gestión de apps"; ganancia pequeña. |
| RF-DES-14 | Arrastrar una app a la ventana para desinstalarla. | Should | v1 | Soltar una `.app` fixture sobre la ventana abre su vista de desinstalación con los asociados descubiertos; soltar algo que no es una app muestra un aviso y no hace nada. |
| RF-DES-15 | Recibos de paquetes (`pkgutil --pkgs`, `pkgutil --files <id>`): proponer ficheros fuera del bundle; `pkgutil --forget` con admin. | Could | v2 | Ficheros en rutas compartidas (`/usr/local/lib`…) se marcan Revisar. |

### 6.4 Explorador de espacio (RF-ESP)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-ESP-01 | Escanear un volumen o carpeta elegida y mostrar una jerarquía navegable ordenada por tamaño asignado. | Must | MVP | Primer resultado en ≤ 1 s; escaneo completo según RNF-02. |
| RF-ESP-02 | Contabilidad honesta: tamaño asignado, hardlinks contados una vez, sin seguir symlinks, sin cruzar volúmenes salvo elección, ficheros dataless como "en la nube". | Must | MVP | En un fixture con hardlinks, symlink circular y fichero sparse, el total coincide con `du -skx` (± 1 %). |
| RF-ESP-03 | Panel "Más grandes": top 50 ficheros (N configurable). | Must | MVP | Coincide con la lista de referencia calculada sobre el fixture. |
| RF-ESP-04 | Paquetes (`.app`, `.photoslibrary`, `.fcpbundle`…) como unidad; entrar en ellos solo en Experto. | Must | MVP | Con un `.app` y un `.photoslibrary` fixture, en Simple aparecen como un único elemento sin hijos; en Experto se pueden expandir. |
| RF-ESP-05 | Acciones: Quick Look, Mostrar en Finder, Copiar ruta, Mover a la Papelera (con RF-SEG), No tocar nunca. Nada preseleccionado. | Must | MVP | Ninguna acción destructiva sin revisión. |
| RF-ESP-06 | Mapa visual (treemap o sunburst) con vista de lista equivalente accesible. | Should | v1 | VoiceOver recorre la vista lista con la misma información. |
| RF-ESP-07 | Ficheros antiguos (no abiertos en N meses) usando el metadato de último uso o, si falta, la fecha de modificación, indicando la fuente. | Should | v1 | La UI muestra la fuente de la fecha; atime no es fuente única (⚠ APFS actualiza atime de forma diferida). |
| RF-ESP-08 | Duplicados: agrupar por tamaño → hash parcial → hash completo; excluir dataless, interior de paquetes y ficheros < 1 MB por defecto. | Should | v1 | 0 falsos positivos en fixture con ficheros del mismo tamaño y distinto contenido; ninguno preseleccionado; aviso de que los clones APFS no liberan espacio al borrarse. |
| RF-ESP-09 | Explicar "Datos del sistema": desglose de lo medible (instantáneas, cachés, copias iOS, swap en `/private/var/vm`, imágenes de VM/Docker) y lo no medible. | Should | v1 | Suma de componentes ≤ total; el resto se muestra como "No medible por la app". |
| RF-ESP-10 | Filtros por tipo (vídeo, imagen, comprimido, imagen de disco), tamaño mínimo y antigüedad. | Should | v1 | Cada filtro aplicado al fixture devuelve exactamente el subconjunto esperado (test por tipo, tamaño y antigüedad). |
| RF-ESP-11 | Volúmenes externos y de red, solo con selección explícita y aviso de lentitud en red. | Should | v1 | Nunca se escanean por defecto. |
| RF-ESP-12 | Re-escaneo incremental (FSEvents + caché de resultados). | Could | v2 | Re-escaneo ≤ 20 % del tiempo inicial si cambia < 1 % de entradas. |
| RF-ESP-13 | Imágenes similares (no idénticas). | Won't | — | Fuera de alcance; candidata a doc 04. |

### 6.5 Instaladores y descargas (RF-INS)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-INS-01 | Detectar `.dmg`, `.pkg`, `.mpkg`, `.iso` y `.xip` en `~/Downloads` y `~/Desktop` (extensión + cabecera). | Must | MVP | Fixture de 10 instaladores y 10 señuelos: 100 % de aciertos. |
| RF-INS-02 | Ningún instalador preseleccionado; acceso rápido "Seleccionar los de apps ya instaladas". | Must | MVP | Test. |
| RF-INS-03 | Omitir imágenes montadas y ficheros dataless (mostrarlos como "En iCloud" con advertencia). | Must | MVP | Con un DMG montado aparece "en uso"; un fichero desalojado sigue dataless tras el escaneo (`ls -lO`). |
| RF-INS-04 | Nunca montar imágenes, expandir paquetes ni ejecutar nada del contenido analizado. | Must | MVP | `hdiutil info` no muestra nuevas imágenes tras el escaneo. |
| RF-INS-05 | Mostrar origen (`kMDItemWhereFroms`, atributo `com.apple.quarantine`) y fecha de descarga. | Should | v1 | Para un fichero fixture con `com.apple.quarantine` y `kMDItemWhereFroms`, se muestran la URL y la fecha; sin metadatos, se muestra "desconocido". |
| RF-INS-06 | "App ya instalada" cuando se pueda deducir sin montar (nombre, metadatos). | Should | v1 | Sin falsos "ya instalada" en el fixture. |
| RF-INS-07 | Fuentes adicionales: `/Applications/Install macOS *.app`, adjuntos de Mail, descargas de apps de mensajería (rutas por app ⚠), carpeta Descargas de iCloud Drive (solo ficheros locales). Caché de Homebrew → doc 03. | Should | v1 | Con fixtures en cada fuente, todos los instaladores se listan con su fuente; ninguno preseleccionado; los ficheros de iCloud no descargados siguen `dataless`. |
| RF-INS-08 | `.zip` que contienen `.app` o `.pkg`, leyendo solo el índice del archivo. | Could | v1 | No se extrae nada a disco. |

### 6.6 Mantenimiento y arranque (RF-MAN)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-MAN-01 | Inventario de solo lectura de `~/Library/LaunchAgents`, `/Library/LaunchAgents` y `/Library/LaunchDaemons`: etiqueta, programa, app propietaria, firmante (Team ID), cargado o no, huérfano (ejecutable inexistente). | Must | MVP | Coincide con los plists del fixture; "cargado" coincide con `launchctl print`. |
| RF-MAN-02 | Ítems de inicio y en segundo plano de otras apps: mostrar lo enumerable con API pública e indicar explícitamente lo que no; enlazar a Ajustes del Sistema › General › Ítems de inicio y extensiones (⚠ nombre/URL en macOS 27). | Must | MVP | Si no se puede enumerar, la UI lo dice; el enlace abre el panel correcto. |
| RF-MAN-03 | Desactivar agentes de usuario de forma reversible (bootout + plist a Papelera) y reactivarlos al deshacer (`launchctl bootstrap gui/<uid> <plist>`). | Should | v1 | Tras deshacer, `launchctl print gui/<uid>/<label>` vuelve a mostrar el servicio. |
| RF-MAN-04 | Desactivar LaunchDaemons y agentes globales vía ayudante. | Should | v1 | Mismo criterio que RF-MAN-03 en el dominio `system`. |
| RF-MAN-05 | Tareas de mantenimiento a petición, cada una con "cuándo sirve": refrescar miniaturas Quick Look (`qlmanage -r cache`, sin admin); vaciar caché DNS (`dscacheutil -flushcache` + `killall -HUP mDNSResponder`, admin); reconstruir índice de Spotlight (`mdutil -E /`, admin, aviso de horas de reindexado); reconstruir Launch Services (`lsregister -kill -r -domain local -domain system -domain user`, ⚠ flags en macOS 27); verificar volumen (`diskutil verifyVolume /`). | Should | v1 | Cada tarea registra comando, salida y código en el journal; ninguna se ejecuta sin acción del usuario. |
| RF-MAN-06 | Sin placebo: no "liberar RAM", no mantenimiento programado con admin, no tareas "por si acaso". | Must | MVP | Revisión de producto; la programación solo admite categorías Seguro (RF-AJU-03). |
| RF-MAN-07 | Listar extensiones del sistema y kexts cargadas (`systemextensionsctl list`, `kmutil showloaded`), solo lectura. | Could | v2 | El listado coincide con la salida de `systemextensionsctl list` y `kmutil showloaded`; no se ofrece ninguna acción de modificación. |
| RF-MAN-08 | Diagnóstico de ítems en segundo plano con `sfltool dumpbtm` (admin), solo lectura. | Could | v2 | Marcado como diagnóstico avanzado. |

### 6.7 Monitor del sistema (RF-MON)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-MON-01 | CPU (total y por tipo de núcleo), presión de memoria y swap, disco (libre, E/S), batería (carga, ciclos, condición). | Must | v1 | Coincide con Monitor de Actividad, `pmset -g batt` y `system_profiler SPPowerDataType` (± 5 %). |
| RF-MON-02 | Red: rendimiento de subida y bajada. | Should | v1 | ± 10 % frente a Monitor de Actividad. |
| RF-MON-03 | Procesos top por CPU, memoria y energía con app propietaria; "Salir" y "Forzar salida" (con confirmación) solo de procesos del usuario. | Should | v1 | Nunca se ofrecen acciones sobre procesos de root o del sistema. |
| RF-MON-04 | Estado de salud explicado por factores (espacio libre < 10 %, presión de memoria alta sostenida, batería "Servicio recomendado"), sin puntuación opaca. | Should | v1 | Cada estado enumera los factores que lo causan. |
| RF-MON-05 | No muestrear con la ventana oculta (salvo el extra de barra de menús, si está activado). | Must | v1 | CPU de la app oculta ≈ 0 % (RNF-07). |
| RF-MON-06 | Extra de barra de menús opcional con métricas mínimas. | Could | v1 | ≤ 1 % CPU medio con muestreo cada 2 s. |
| RF-MON-07 | Avisos opcionales (espacio libre < X GB). | Could | v1 | Con umbral por encima del espacio libre real se emite una notificación; no se repite más de una vez cada 24 h. |
| RF-MON-08 | Temperaturas y ventiladores. | Could | v2 | ⚠ Sin API pública estable en Apple Silicon; solo con fuente pública. |
| RF-MON-09 | Procesos zombi y apps que no responden (informativo). | Could | v2 | Un proceso zombi creado en test aparece listado; no se ofrece ninguna acción sobre él. |

### 6.8 Historial y deshacer (RF-HIS)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-HIS-01 | Journal append-only por operación: id, fecha, módulo, origen (GUI/CLI/programada/IA), plan confirmado y, por elemento, ruta original, ruta en la Papelera, tamaño asignado, dev/inode, mtime, propietario y permisos, resultado. | Must | MVP | Cada elemento movido tiene su entrada; validación de esquema en tests. |
| RF-HIS-02 | Escritura anticipada (write-ahead): intención antes de mover, resultado después; reconciliación al arrancar tras un cierre abrupto. | Must | MVP | Test de inyección de fallos (RNF-09). |
| RF-HIS-03 | Vista Historial con estado por operación: Restaurable / Parcial / Irreversible / Restaurada. | Must | MVP | Para operaciones fixture en los cuatro estados, cada una muestra el estado correcto; vaciar la Papelera cambia el estado sin reiniciar la app. |
| RF-HIS-04 | Deshacer por operación o por elemento, con previsualización y gestión de conflictos (CU-05). | Must | MVP | M-4 (≥ 99 %). |
| RF-HIS-05 | Detectar elementos ya no restaurables y marcarlos, sin fallar en silencio. | Must | MVP | Tras vaciar la Papelera, la operación pasa a Irreversible. |
| RF-HIS-06 | Historial y deshacer desde CLI (`rotten history`, `rotten undo <op-id>`). | Must | MVP | Una operación hecha en la GUI se deshace desde la CLI y viceversa. |
| RF-HIS-07 | Exportar historial en JSON y CSV. | Should | v1 | El JSON exportado valida contra su esquema y el CSV abre en Numbers con una fila por elemento; los totales coinciden con la vista Historial. |
| RF-HIS-08 | Retención configurable (por defecto 180 días) y tope de tamaño del journal. | Should | v1 | Las entradas más antiguas que la retención se purgan al arrancar, salvo las de operaciones aún restaurables; el journal no supera el tope configurado. |

### 6.9 Ajustes, reglas y programación (RF-AJU)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-AJU-01 | Exclusiones por ruta, patrón glob, bundle id o categoría, desde resultados o Ajustes; aplican a GUI, CLI y programación. | Must | MVP | Un elemento excluido no aparece en ningún plan de los tres orígenes (test). |
| RF-AJU-02 | Interruptor modo Simple / Experto. | Must | MVP | Cambia presentación y preselección visual, no las salvaguardas (test: mismos rechazos RF-SEG en ambos modos). |
| RF-AJU-03 | Programación de limpieza (CU-06): solo categorías Seguro elegidas una a una; condiciones (corriente, no bajo consumo, inactividad); freno de anomalías. | Should | v1 | Con batería y ajustes por defecto no se ejecuta; plan > 2× mediana o > 20 GB → no se ejecuta y se notifica. |
| RF-AJU-04 | Disparador por umbral: limpiar cuando el espacio libre < X GB. | Could | v2 | Con el umbral por encima del espacio libre del fixture, se dispara una ejecución con las mismas restricciones que RF-AJU-03; no se dispara más de una vez cada 24 h. |
| RF-AJU-05 | Reglas personalizadas (ruta + patrón + antigüedad → Papelera), sujetas a rutas protegidas; nunca borrado permanente. | Could | v2 | Una regla sobre una ruta protegida se rechaza al guardarla. |
| RF-AJU-06 | Importar/exportar configuración (exclusiones, reglas, programación) en TOML, con diff antes de aplicar. | Should | v1 | Ida y vuelta sin pérdidas. |
| RF-AJU-07 | Idioma: seguir el sistema, con opción manual es/en. | Must | MVP | Con el sistema en inglés y la app forzada a español, el 100 % de las cadenas visibles están en español (y viceversa). |
| RF-AJU-08 | Pantalla de permisos: FDA, notificaciones, ayudante, "Gestión de apps" si aplica; estado real y botón al panel correspondiente. | Must | MVP | El estado se actualiza al conceder o revocar sin reiniciar la app (salvo cuando macOS lo exige). |
| RF-AJU-09 | Opción opt-in (desactivada por defecto) para eliminar definitivamente, tras N días, los elementos que la app envió a la Papelera. | Could | v1 | Activarla exige la confirmación de RF-SEG-03; Historial muestra "se eliminará el <fecha>". |
| RF-AJU-10 | Comprobación de actualizaciones (pregunta en el onboarding) e informes de fallos opt-in. | Should | MVP | Sin consentimiento, cero tráfico de red (RNF-17). |
| RF-AJU-11 | Restablecer ajustes por defecto. | Should | v1 | Tras restablecer, la configuración coincide con la de una instalación limpia; el journal y el historial se conservan. |

### 6.10 Interfaz de línea de comandos (RF-CLI)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-CLI-01 | Binario CLI (nombre provisional `rotten`) sobre el mismo núcleo, reglas, exclusiones y journal que la GUI. | Must | MVP | Una operación CLI aparece en el Historial de la GUI y es deshacible. |
| RF-CLI-02 | Subcomandos MVP: `scan`, `clean`, `history`, `undo`, `doctor`, `config`. v1: `uninstall`, `analyze`, `installers`, `startup`, `status`, `trash`. | Must | MVP · v1 | `rotten --help` lista los subcomandos de la fase. |
| RF-CLI-03 | `--dry-run` en toda acción destructiva; sin `--yes`, muestra el plan y pide confirmación; sin TTY y sin `--yes`, aborta. | Must | MVP | Test de los tres casos. |
| RF-CLI-04 | `--json` con esquema versionado (`schema_version`) y claves estables en inglés, independiente del idioma. | Must | MVP | Validación contra JSON Schema en CI. |
| RF-CLI-05 | Códigos de salida: 0 ok · 1 error · 2 uso incorrecto · 3 falta permiso · 4 completado con omisiones · 5 abortado por seguridad. | Must | MVP | Tests por código. |
| RF-CLI-06 | Rechazar la ejecución como root salvo `--allow-root`; nunca pedir `sudo` para operaciones de usuario. | Must | MVP | `sudo rotten clean` sin la opción → código 5. |
| RF-CLI-07 | `doctor`: permisos (FDA del terminal), versión, rutas de configuración y journal. | Must | MVP | Sin FDA en el terminal, `rotten doctor` lo indica y devuelve código 3; con FDA, informa "concedido" y las rutas reales de configuración y journal. |
| RF-CLI-08 | Borrado permanente solo con `--permanent` y confirmación interactiva (o `--permanent --yes`). | Must | MVP | Sin `--permanent`, ningún elemento se borra fuera de la Papelera (test). |
| RF-CLI-09 | Salida para humanos localizada es/en. | Must | MVP | `LANG=es_ES.UTF-8 rotten scan` imprime en español y `LANG=en_US.UTF-8` en inglés; la salida `--json` es idéntica en ambos. |
| RF-CLI-10 | Autocompletado (zsh, bash, fish) y página `man`. | Should | v1 | Tras instalar los scripts, el autocompletado sugiere subcomandos y opciones en zsh, bash y fish; `man rotten` abre la página. |
| RF-CLI-11 | Instalación del CLI desde la app y vía Homebrew (detalle en doc 05). | Should | v1 | Tras instalar desde la app o con Homebrew, `rotten --version` coincide con la versión de la app; desinstalar elimina el enlace. |

### 6.11 Interfaz gráfica (RF-UI)

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-UI-01 | App nativa de macOS: menús estándar, atajos, ventana redimensionable, modo claro/oscuro. Framework → doc 05. | Must | MVP | Checklist de las Human Interface Guidelines superado. |
| RF-UI-02 | Barra lateral por módulos; Zona Desarrollador y Seguridad de dependencias ocultas en Simple; Organizador IA oculto hasta activarlo. | Must | MVP | En Simple, la barra lateral no muestra Zona Desarrollador ni Seguridad de dependencias; al cambiar a Experto aparecen sin reiniciar; el Organizador IA solo aparece tras activarlo. |
| RF-UI-03 | Pantalla de revisión común a toda acción destructiva: lista, tamaño, riesgo, destino (Papelera / permanente), omitidos. | Must | MVP | Ver RF-SEG-01. |
| RF-UI-04 | Progreso con resultados en streaming y cancelación; el hilo de UI nunca se bloquea > 100 ms. | Must | MVP | Instrumentación (RNF-05). |
| RF-UI-05 | Lenguaje sin jerga en Simple; rutas y términos técnicos en Experto; glosario en ayudas contextuales. | Must | MVP | Revisión de textos; M-6. |
| RF-UI-06 | Onboarding de ≤ 4 pantallas: promesa de seguridad, modo, permisos, primer escaneo. | Must | MVP | M-1. |
| RF-UI-07 | Quick Look y "Mostrar en Finder" en cualquier elemento listado. | Must | MVP | En cada vista con elementos (Limpieza, Desinstalador, Explorador, Instaladores, Historial), Espacio abre Quick Look y "Mostrar en Finder" selecciona el elemento en Finder. |
| RF-UI-08 | Estados vacíos, de error y "bloqueado por permiso" con acción sugerida. | Must | MVP | Ningún error muestra solo un código. |
| RF-UI-09 | Riesgo indicado con icono + texto, nunca solo color. | Must | MVP | RNF-22. |
| RF-UI-10 | Aviso "Deshacer" (⌘Z) durante 30 s tras cada operación. | Must | MVP | ⌘Z dentro de los 30 s posteriores a una operación lanza CU-05; pasado ese tiempo el aviso desaparece y ⌘Z no hace nada destructivo. |
| RF-UI-11 | Notificaciones del sistema para ejecuciones programadas y avisos. | Should | v1 | Una ejecución programada de prueba genera una notificación con resumen y botón "Deshacer" que abre CU-05. |
| RF-UI-12 | Ayuda integrada "¿Qué es esto?" en cada categoría. | Should | v1 | El 100 % de las categorías del catálogo tienen ayuda "¿Qué es esto?" en es/en. |

---

## 7. Seguridad operativa (RF-SEG) — requisitos duros

Todos son **Must**. Se aplican a todos los módulos (incluidos los de los docs 03 y 04), a ambos modos, a la GUI, a la CLI y a toda automatización. Ninguno se puede desactivar desde Ajustes.

| ID | Descripción | Prio. | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-SEG-01 | **Previsualización obligatoria.** Ninguna acción destructiva (mover a Papelera, borrar, desactivar agente, vaciar Papelera) se ejecuta sin un plan explícito mostrado y confirmado (GUI: pantalla de revisión; CLI: plan + confirmación o `--yes`). El núcleo solo expone la ejecución de planes confirmados. | Must | MVP | La API pública del núcleo no ofrece borrar sin plan confirmado (revisión de API + test); cada entrada del journal referencia un plan con marca de confirmación; M-2 = 0. |
| RF-SEG-02 | **Papelera por defecto.** Todo elemento se mueve a la Papelera de su volumen con la API del sistema equivalente a Finder. Si el volumen no tiene Papelera (p. ej. recursos SMB), la operación se trata como borrado permanente y exige RF-SEG-03. | Must | MVP | Tras limpiar, cada elemento está en `~/.Trash` o `/Volumes/<X>/.Trashes/<uid>`; en un volumen sin Papelera aparece el diálogo de borrado permanente. |
| RF-SEG-03 | **Borrado permanente solo explícito.** Acción separada, nunca por defecto (ni en Experto, ni en programaciones, ni en IA, ni en CLI sin `--permanent`). La confirmación indica nº de elementos, tamaño y "no se puede deshacer"; el botón destructivo no es el predeterminado. Incluye vaciar la Papelera. La Papelera nunca se vacía como efecto secundario de una limpieza. | Must | MVP | Intro en el diálogo no borra; las ejecuciones programadas y de IA no pueden producir planes con destino permanente (validación del plan + test). |
| RF-SEG-04 | **Rutas protegidas** (lista dura, no editable por usuario ni reglas): `/System`, `/usr` (salvo `/usr/local`), `/bin`, `/sbin`, `/private/var/db`, `/private/var/vm`, `/Library/Apple`, `~/Library/Keychains`, `~/Library/Mobile Documents`, `~/Library/CloudStorage`, `~/Library/Messages`, `~/Library/Mail` (salvo CAT-11/12 explícitos), `~/Library/Safari` (salvo cachés), `~/Library/Application Support/AddressBook`, `~/Library/Calendars`, `~/Library/Application Support/com.apple.TCC`, `*.photoslibrary`, `~/Music/Music`, `~/.ssh`, `~/.gnupg`, datos y journal de la propia app, la carpeta personal y la raíz de cada volumen en sí. Documentos, Escritorio, Descargas, Imágenes, Películas y Música solo son accesibles por selección explícita (Explorador, Instaladores, doc 04), nunca por reglas de categoría. | Must | MVP | Test de propiedades: ningún plan de ningún módulo contiene una ruta protegida ni un ancestro de ella; en la UI la casilla aparece deshabilitada con explicación. |
| RF-SEG-05 | **Fail-closed.** (a) Solo se actúa dentro de raíces permitidas declaradas por reglas del catálogo o por selección explícita del usuario; (b) las exclusiones del usuario prevalecen sobre cualquier regla; (c) si una ruta no se puede canonicalizar o clasificar, o la configuración/exclusiones no se pueden leer o validar, no se ejecuta ninguna acción destructiva. | Must | MVP | Con el fichero de exclusiones corrupto, `rotten clean` aborta con código 5 y la GUI deshabilita "Limpiar" con explicación. |
| RF-SEG-06 | **Resolución de rutas segura.** Canonicalizar; nunca seguir symlinks (se mueve el enlace, no su destino); omitir si un componente padre es un symlink que sale de la raíz permitida; no cruzar puntos de montaje; comparar con exclusiones respetando la sensibilidad a mayúsculas y la normalización Unicode del volumen. | Must | MVP | Fuzzing y tests de propiedades de canonicalización; fixture con un symlink a `~/Documents` dentro de una caché: `~/Documents` intacto. |
| RF-SEG-07 | **Identidad del fichero (anti-TOCTOU).** En la previsualización se registran dev, inode, tipo, tamaño y mtime; justo antes de actuar se revalidan sobre el elemento abierto relativo a su carpeta padre (operaciones relativas a descriptor). Si no coinciden, se omite con motivo "cambió desde la revisión". En carpetas se revalida la carpeta y se omite si contiene entradas modificadas tras la previsualización (en cachés, se omiten solo esas entradas). | Must | MVP | Sustituir un elemento por otro del mismo nombre entre revisión y ejecución → omitido; sustituir una carpeta por un symlink → omitido. |
| RF-SEG-08 | **Ficheros en uso y apps abiertas.** No tocar elementos de apps en ejecución ni ficheros abiertos por procesos visibles para el usuario; en áreas del sistema, "no determinable" cuenta como "en uso"; omitir imágenes montadas; ofrecer cerrar la app con salida normal. | Must | MVP | Un proceso de prueba que mantiene abierto un fichero de caché provoca su omisión con motivo "en uso". |
| RF-SEG-09 | **Nunca preseleccionar documentos del usuario.** Ningún módulo (Limpieza, Explorador, Duplicados, Instaladores, Desinstalador, Dev, IA) marca por defecto contenido de Documentos, Escritorio, Descargas, Imágenes, Películas, Música, iCloud Drive o File Provider, ni ficheros de tipo documento, imagen, vídeo o audio fuera de raíces de caché. | Must | MVP | Test de propiedades con un home fixture: preseleccionados ∩ documentos = ∅ en todos los módulos. |
| RF-SEG-10 | **No provocar descargas de iCloud/File Provider.** Detectar ficheros dataless (`SF_DATALESS` en `st_flags`) y no abrirlos, leerlos ni hashearlos; fijar la política de proceso que impide materializarlos (`setiopolicy_np` con `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES`, ⚠ verificar); no recorrer `~/Library/Mobile Documents` ni `~/Library/CloudStorage` en limpieza. | Must | MVP | Un fichero desalojado con `brctl evict <fichero>` sigue `dataless` en `ls -lO` tras escaneo rápido, Explorador, Duplicados e Instaladores. |
| RF-SEG-11 | **Log de operaciones.** Journal (RF-HIS-01/02) + log técnico local rotado; sin contenido de ficheros; rutas solo en local. | Must | MVP | Toda operación destructiva tiene entrada; un fichero con un marcador conocido no aparece en ningún log. |
| RF-SEG-12 | **Tamaños honestos.** (a) Tamaño asignado (bloques), no lógico; (b) hardlinks contados una vez; (c) aviso de que los clones APFS pueden reducir el espacio realmente liberado (no detectables de forma fiable por fichero ⚠); (d) purgable siempre aparte; (e) distinguir "movido a la Papelera" (aún no libera espacio) de "liberado" (delta medido de capacidad disponible). | Must | MVP | Tras vaciar, el "liberado" coincide con el delta medido ± máx(5 %, 200 MB) (M-5); con un fichero clonado (`cp -c`) se muestra el aviso. |
| RF-SEG-13 | **Mínimo privilegio.** MVP: ninguna operación de admin. Desde v1: solo mediante un ayudante con interfaz tipada de lista cerrada (p. ej. "eliminar instantánea X", "mover a Papelera ruta dentro de raíz de sistema permitida") que aplica él mismo RF-SEG-04…08 y verifica la firma del cliente. Nunca "ejecutar comando como root" genérico. | Must | MVP (restricción) · v1 (ayudante) | MVP: sin llamadas de elevación (revisión de código en CI). v1: el ayudante rechaza rutas fuera de raíces permitidas y clientes no firmados (test). |
| RF-SEG-14 | **Automatizaciones con las mismas salvaguardas.** Programación, CLI no interactiva, IA y reglas de usuario: solo Papelera; programación solo con categorías Seguro preaprobadas; freno de anomalías (> 2× mediana de las últimas 5 ejecuciones o > 20 GB → pausa y aviso). | Must | MVP (CLI) · v1 (programación) | Tests de RF-CLI-03 y RF-AJU-03. |
| RF-SEG-15 | **Errores parciales.** Continuar con el resto, informar cada fallo con motivo, nunca reintentar automáticamente con más privilegios. | Must | MVP | Un elemento sin permiso se omite, el resto se procesa y la CLI devuelve código 4. |
| RF-SEG-16 | **Dry-run idéntico.** La simulación usa el mismo código que la ejecución y produce el mismo plan (salvo cambios en disco entre ambas). | Must | MVP | Sobre un fixture estático, plan de `--dry-run` = plan ejecutado. |
| RF-SEG-17 | **Integridad de reglas.** El catálogo va embebido en la app firmada; no se cargan reglas destructivas remotas; las reglas de usuario se validan contra RF-SEG-04. | Must | MVP | La app no abre conexiones para obtener reglas; una regla de usuario sobre una ruta protegida se rechaza. |
| RF-SEG-18 | **Restauración segura.** Deshacer nunca sobrescribe un fichero existente sin confirmación (CU-05 A2) y restaura propietario y permisos registrados. | Must | MVP | Test de conflicto y de permisos. |
| RF-SEG-19 | **Solo el usuario actual.** No se leen ni modifican carpetas personales de otros usuarios ni sus carpetas en `/private/var/folders`. | Must | MVP | Test en VM con un segundo usuario. |

---

## 8. Encaje de los módulos de otros documentos

Los módulos Zona Desarrollador, Seguridad de dependencias y Organizador con IA se especifican en los docs 03 y 04; la arquitectura, en el doc 05. Este documento solo fija **cómo encajan en el producto** y el **contrato** que deben cumplir con el núcleo.

| Módulo | Doc | Visible en | Encaje en el producto | Contrato con el núcleo |
|---|---|---|---|---|
| Zona Desarrollador (RF-DEV) | 03 | Experto (en Simple, sugerencia si se detectan toolchains) | Artefactos de proyecto, cachés de toolchains, simuladores, Docker. Reutiliza el escáner del Explorador y la pantalla de revisión. | Sus elementos son planes del pipeline RF-SEG con niveles Seguro/Precaución/Revisar. Sus protecciones propias (repos Git, ficheros trackeados o sin commitear, `.env`, claves) **se suman** a RF-SEG-04; nunca lo relajan. Subcomandos bajo el mismo binario CLI. |
| Seguridad de dependencias (RF-VUL) | 03 | Experto | Inventario de librerías instaladas y vulnerabilidades conocidas. | Solo lectura por defecto. Toda acción derivada (p. ej. eliminar un paquete sin uso) es un plan RF-SEG. Descargar bases de vulnerabilidades es tráfico de red: requiere activar el módulo y figura en Ajustes › Privacidad (RNF-17, RNF-19). |
| Organizador con IA (RF-IA) | 04 | Oculto hasta activarlo (opt-in) | Organizar Escritorio, Descargas, carpetas y marcadores del navegador. | La IA solo **propone** planes (mover, renombrar, agrupar); nunca borrado permanente (RF-SEG-03). Propuestas revisables y deshacibles (journal con origen "IA"). Modificar datos de otras apps (p. ej. marcadores) exige una copia previa restaurable. Qué datos salen del equipo lo define el doc 04 dentro de RNF-17/19. |
| Arquitectura (ADR, RNF-ARQ) | 05 | — | Framework de GUI, comunicación núcleo ↔ GUI ↔ CLI, ayudante privilegiado, agente de programación, formato del journal, distribución y actualizaciones. | Debe satisfacer los RNF de este documento; lo marcado "→ doc 05" se decide allí. |

---

## 9. Requisitos no funcionales (RNF)

**Mac de referencia** para cifras de rendimiento: MacBook Air M1, 8 GB, SSD interno, macOS 15, con un corpus sintético reproducible (script en el repositorio) de 500 000 entradas en las raíces del catálogo y 2 M entradas en el volumen. "Caliente" = caché del sistema de ficheros poblada; "frío" = tras reiniciar.

| ID | Área | Requisito | Prio. | Fase | Criterio / cifra |
|---|---|---|---|---|---|
| RNF-01 | Rendimiento | Escaneo rápido de las raíces del catálogo. | Must | MVP | ≤ 10 s p95 en caliente; ≤ 20 s en frío. |
| RNF-02 | Rendimiento | Explorador sobre volumen completo. | Must | MVP | 2 M entradas en ≤ 45 s (caliente); primer resultado ≤ 1 s; ≥ 50 000 entradas/s sostenidas en SSD interno. |
| RNF-03 | Rendimiento | Memoria. | Must | MVP | Escaneo rápido ≤ 150 MB de pico; Explorador ≤ 300 MB con 2 M entradas (≈ 150 B/entrada); `rotten scan` ≤ 100 MB; GUI en reposo ≤ 200 MB (depende del framework, doc 05). |
| RNF-04 | Rendimiento | Arranque de la app. | Should | MVP | Interactiva en ≤ 1,5 s (caliente) y ≤ 3 s (frío). |
| RNF-05 | Rendimiento | Respuesta de la UI. | Must | MVP | Hilo principal sin bloqueos > 100 ms; progreso ≥ 4 actualizaciones/s; cancelación ≤ 1 s. |
| RNF-06 | Rendimiento | Ejecución de planes. | Should | MVP | 10 000 elementos a la Papelera, con verificación y journal, en ≤ 30 s. |
| RNF-07 | Energía | Sin consumo en reposo. | Must | MVP | 0 % CPU y sin temporizadores con la app oculta (extra de barra de menús ≤ 1 %); sin procesos residentes: el agente de programación lo lanza launchd a su hora y termina al acabar. |
| RNF-08 | Energía | Trabajo en segundo plano amable. | Must | v1 | QoS utility/background y E/S de baja prioridad; programación por defecto solo con corriente y sin Modo de bajo consumo; pausa con estado térmico "serio" o superior. |
| RNF-09 | Fiabilidad | Consistencia ante fallos. | Must | MVP | 1 000 ejecuciones con `kill -9` en puntos aleatorios: 100 % de elementos en su ruta original o en la Papelera y registrados. |
| RNF-10 | Fiabilidad | Estabilidad. | Should | v1 | < 0,5 % de sesiones con cierre inesperado en beta (informes opt-in y manuales). |
| RNF-11 | Fiabilidad | Degradación elegante. | Must | MVP | Errores de permisos, volúmenes que desaparecen y ficheros que cambian nunca cierran la app; se clasifican y se muestran. |
| RNF-12 | Fiabilidad | Idempotencia. | Must | MVP | Repetir un deshacer o reanudar una ejecución interrumpida no duplica ni pierde elementos. |
| RNF-13 | Seguridad | Mínimo privilegio. | Must | MVP | Ver RF-SEG-13; ayudante solo en v1, instalado con el mecanismo moderno del sistema y con verificación del firmante del cliente (doc 05). |
| RNF-14 | Seguridad | Código y cadena de suministro. | Must | MVP | `unsafe` solo en módulos FFI aislados y documentados; `cargo audit` y `cargo deny` (vulnerabilidades y licencias) bloqueantes en CI; dependencias fijadas; SBOM por release. |
| RNF-15 | Seguridad | Firma y notarización. | Must | MVP | Developer ID, notarizado, hardened runtime; actualizaciones firmadas verificadas antes de instalar. |
| RNF-16 | Seguridad | No ejecutar contenido analizado. | Must | MVP | Nunca se montan imágenes, se expanden paquetes ni se ejecutan scripts de proyectos. |
| RNF-17 | Privacidad | Sin telemetría; red cero por defecto. | Must | MVP | Con ajustes por defecto, cero conexiones salvo las aceptadas en el onboarding (comprobación de actualizaciones); verificable con `nettop -p <pid>` o un cortafuegos de salida durante 24 h. |
| RNF-18 | Privacidad | Datos locales. | Must | MVP | Configuración y journal en `~/Library/Application Support/<bundle-id>/`; logs en `~/Library/Logs/<app>/`; sin contenido de ficheros; el diagnóstico exportable se puede revisar antes de compartirlo. |
| RNF-19 | Privacidad | Transparencia de red. | Must | v1 | Ajustes › Privacidad lista todos los destinos posibles (actualizaciones, bases de vulnerabilidades, proveedores de IA) con su estado; cada uno es opt-in. |
| RNF-20 | Accesibilidad | VoiceOver. | Must | MVP | 100 % de controles con etiqueta y rol; tablas navegables; progreso anunciado; el mapa visual tiene vista de lista equivalente; Accessibility Inspector sin errores. |
| RNF-21 | Accesibilidad | Teclado. | Must | MVP | Todo accesible sin ratón; orden de foco lógico; atajos ⌘R escanear, ⌘Z deshacer, ⌘⌫ mover a la Papelera, Espacio Quick Look, ⌘, Ajustes. |
| RNF-22 | Accesibilidad | Contraste y preferencias. | Must | MVP | WCAG 2.2 AA (4,5:1 texto, 3:1 componentes); riesgo con icono + texto; respeta "Aumentar contraste", "Reducir movimiento" y "Reducir transparencia". |
| RNF-23 | Accesibilidad | Tamaño de texto. | Should | MVP | Utilizable con texto al 200 % sin truncar cifras, riesgo ni botones de acción. |
| RNF-24 | i18n | Español e inglés desde el MVP. | Must | MVP | 100 % de cadenas externalizadas, sin concatenación y con plurales por idioma; pseudo-localización en CI; números, fechas y tamaños según locale; unidades decimales como Finder. |
| RNF-25 | i18n | Contenido de reglas y CLI. | Must | MVP | Explicaciones del catálogo en es/en; JSON de la CLI independiente del idioma. |
| RNF-26 | Usabilidad | Lenguaje claro en Simple. | Must | MVP | Sin rutas ni jerga visibles por defecto; M-6 ≥ 90 % y SUS ≥ 75. |
| RNF-27 | Usabilidad | Vocabulario de riesgo consistente. | Must | MVP | Seguro / Precaución / Revisar idénticos en GUI, CLI y documentación. |
| RNF-28 | Usabilidad | Errores accionables. | Must | MVP | Todo mensaje de error incluye causa y acción sugerida. |
| RNF-29 | Mantenibilidad | Núcleo desacoplado. | Must | MVP | Núcleo Rust como biblioteca independiente de la UI; GUI y CLI son clientes finos (doc 05). |
| RNF-30 | Mantenibilidad | Catálogo declarativo. | Must | MVP | Reglas y textos como datos con tests por regla. |
| RNF-31 | Mantenibilidad | Pruebas. | Must | MVP | ≥ 80 % de líneas en módulos de seguridad (canonicalización, plan, ejecución, journal, deshacer) con tests de propiedades y fuzzing; ≥ 60 % global. |
| RNF-32 | Mantenibilidad | CI multiplataforma. | Must | MVP | CI en el macOS mínimo y en el actual; arm64 (y x86_64 si se confirma RNF-42). |
| RNF-33 | Mantenibilidad | Logs estructurados. | Should | MVP | Niveles configurables; sin datos personales salvo rutas locales. |
| RNF-34 | Legal | Licencias y clean-room. | Must | MVP | MIT; dependencias con licencias permisivas compatibles (sin GPL), verificado con `cargo deny`; sin código de la referencia. |
| RNF-35 | Distribución | Canales. | Must | MVP | Fuera de la Mac App Store (su sandbox es incompatible con limpiar datos de otras apps ⚠, decisión en doc 05): DMG notarizado en GitHub Releases + Homebrew Cask. |
| RNF-36 | Distribución | Actualizaciones. | Should | v1 | Automáticas, firmadas, con comprobación opt-in y vuelta a la versión anterior; configuración y journal migran sin pérdida. |
| RNF-37 | Distribución | Tamaño. | Should | MVP | Descarga ≤ 50 MB. |
| RNF-38 | Compatibilidad | macOS mínimo. | Must | MVP | **macOS 15 Sequoia** (N-2 respecto a macOS 27, alineado con la ventana de parches de seguridad de Apple); probado en 15, 26 y 27. |
| RNF-39 | Compatibilidad | Apple Silicon. | Must | MVP | Binario arm64 nativo; todos los benchmarks RNF-01…07 se miden en arm64. |
| RNF-40 | Compatibilidad | Sistemas de ficheros. | Must | MVP | APFS (sensible e insensible a mayúsculas), HFS+ y exFAT externos; volúmenes de red solo con selección explícita (sin Papelera → RF-SEG-02). |
| RNF-41 | Compatibilidad | Coexistencia. | Must | MVP | Funciona con Time Machine, iCloud, File Provider, FileVault y Macs gestionados por MDM; los bloqueos de permisos se explican, no fallan. |
| RNF-42 | Compatibilidad | Intel (x86_64). | Should | v1 | Binario universal mientras las versiones soportadas (15, 26) corran en Intel; macOS 27 ya no admite Intel (⚠ verificar). CI con tests en x86_64. Revisar en 2027. |

---

## 10. Priorización del MVP

**Criterio:** entregar de punta a punta la promesa central (limpiar con seguridad, entender el espacio, deshacer) sin privilegios de administrador, para validar la confianza antes de ampliar superficie.

| # | Entra en el MVP | Por qué |
|---|---|---|
| 1 | Onboarding y permisos (CU-01; RF-UI-06, RF-AJU-08) | Sin FDA el valor es parcial y es el primer punto de abandono. |
| 2 | RF-SEG completo (sin ayudante) + Historial y deshacer (RF-HIS-01…06) | Es el diferenciador principal y no se puede añadir después sin rehacer el motor. |
| 3 | Panel, escaneo rápido y limpieza de CAT-01, 02, 03, 05, 08 (Safari/Chrome/Firefox) y 10 | Mayor ganancia sin admin y con riesgo Seguro. |
| 4 | Desinstalador a nivel usuario (RF-DES-01, 03–09) | Muy demandado; reutiliza el pipeline. |
| 5 | Explorador básico: jerarquía + "Más grandes" (RF-ESP-01…05) | Responde "¿qué ocupa espacio?" con cifras honestas. |
| 6 | Instaladores en Descargas y Escritorio (RF-INS-01…04) | Ganancia grande y barata. |
| 7 | Exclusiones y modos Simple/Experto (RF-AJU-01, 02) | Control y confianza para ambas personas. |
| 8 | Arranque en solo lectura (RF-MAN-01, 02) | Visibilidad sin riesgo. |
| 9 | CLI mínima (`scan`, `clean`, `history`, `undo`, `doctor`, `config`; `--dry-run`, `--json`) | Personas P2/P3 y base de los tests e2e. |
| 10 | Español/inglés y accesibilidad base (RNF-20…24) | Más caro de añadir después. |

**Recomendación al doc 03:** incluir en el MVP un corte mínimo de la Zona Desarrollador (artefactos `node_modules`, `target/`, DerivedData, con protección de repos Git), porque P2 es persona principal. La decisión y el detalle corresponden al doc 03.

**Queda fuera del MVP:** ayudante privilegiado y categorías de admin (v1; amplía la superficie de ataque); Monitor (v1; no es el problema principal y Monitor de Actividad ya existe); programación (v1; requiere confianza previa y agente de launchd); duplicados, antiguos y mapa visual (v1); copias de iOS e instantáneas de Time Machine (v1; riesgo alto o admin); vulnerabilidades e IA (docs 03 y 04).

**Criterio de salida del MVP:** beta cerrada de ≥ 20 usuarios (10 P1, 10 P2) durante 4 semanas con M-1…M-5 cumplidas y cero incidentes de pérdida de datos.

---

## 11. Glosario, supuestos y preguntas abiertas

### 11.1 Glosario

| Término | Definición |
|---|---|
| Acceso total al disco (FDA) | Permiso de privacidad de macOS que permite leer datos protegidos (Mail, Safari, contenedores de otras apps…). Solo lo concede el usuario en Ajustes del Sistema. |
| TCC | Transparency, Consent and Control: subsistema de permisos de privacidad de macOS. |
| SIP | System Integrity Protection: impide modificar rutas del sistema incluso como root. |
| Volumen sellado (SSV) | Volumen del sistema de solo lectura y firmado criptográficamente. |
| APFS | Sistema de ficheros de Apple; admite clones, instantáneas y espacio purgable. |
| Clon APFS | Copia que comparte bloques con el original; borrarla puede no liberar espacio. |
| Espacio purgable | Espacio que macOS puede liberar por sí solo cuando lo necesita (instantáneas, ficheros optimizados de iCloud, ciertas cachés). |
| Instantánea local | Punto de restauración APFS que crea Time Machine en el disco interno. |
| Fichero dataless ("sin descargar") | Fichero de iCloud/File Provider cuyo contenido no está en el Mac; leerlo provoca su descarga. |
| File Provider | Mecanismo de macOS para servicios de nube (iCloud Drive, Dropbox, OneDrive…) bajo `~/Library/CloudStorage`. |
| LaunchAgent / LaunchDaemon | Servicios gestionados por launchd: por usuario (agent) o de sistema (daemon). |
| Ítems de inicio / en segundo plano | Apps y servicios que arrancan al iniciar sesión, gestionados en Ajustes del Sistema › General. |
| Ayudante privilegiado | Componente separado que ejecuta, con privilegios, un conjunto cerrado de operaciones tipadas (v1). |
| Gestión de apps | Permiso de macOS que controla qué apps pueden modificar otras apps. |
| App Translocation | Ejecución desde una ruta aleatoria de solo lectura de una app no movida a `/Applications`. |
| Bundle id / Team ID | Identificador de una app / identificador del desarrollador en su firma. |
| Notarización / Gatekeeper | Revisión automática de Apple / mecanismo que impide ejecutar software no firmado o alterado. |
| Plan | Lista exacta de elementos y acciones generada por el núcleo; lo único que se puede ejecutar. |
| Previsualización / dry-run | Mostrar el plan sin ejecutarlo. |
| Journal | Registro append-only de operaciones que permite auditar y deshacer. |
| TOCTOU | Time-of-check to time-of-use: que el fichero cambie entre la revisión y la acción. |
| Inode | Identificador de un fichero dentro de un volumen. |
| Exclusión ("whitelist") | Ruta, patrón o app que el usuario marca como "no tocar nunca". |
| Raíz permitida | Carpeta dentro de la cual una regla puede proponer acciones. |
| Fail-closed | Ante la duda o el error, no actuar. |
| Tamaño asignado / lógico | Bloques realmente ocupados en disco / tamaño declarado del fichero. |
| Huérfano | Resto de una app ya desinstalada, o agente cuyo ejecutable ya no existe. |
| Modo Simple / Experto | Presentaciones del producto para P1 y P2 (§3). |

### 11.2 Supuestos

| ID | Supuesto |
|---|---|
| A-01 | Distribución fuera de la Mac App Store (decisión final en doc 05). |
| A-02 | Núcleo en Rust; GUI nativa con framework a decidir en doc 05. |
| A-03 | Un usuario por sesión; sin cuentas ni servidor propio. |
| A-04 | Quien quiera el valor completo concederá FDA; el modo limitado es útil pero parcial. |
| A-05 | macOS 27 solo funciona en Apple Silicon (⚠ verificar). |
| A-06 | Producto gratuito y de código abierto (MIT); modelo de negocio sin definir (Q-08). |
| A-07 | La referencia se usa solo como lista pública de funcionalidades (clean-room). |
| A-08 | El Mac de referencia (MacBook Air M1, 8 GB) representa el extremo bajo del hardware soportado relevante. |

### 11.3 Preguntas abiertas para el usuario

| ID | Pregunta | Propuesta actual |
|---|---|---|
| Q-01 | ¿macOS mínimo 15 o 14 (más alcance)? | 15 |
| Q-02 | ¿Soporte Intel en v1? | Sí, como binario universal, mientras las versiones soportadas corran en Intel |
| Q-03 | Nombre del comando CLI | `rotten` (provisional) |
| Q-04 | Mover a la Papelera no libera espacio hasta vaciarla. ¿Basta con "Liberar ahora" tras cada limpieza más la opción opt-in RF-AJU-09, o se quiere en Experto "eliminar definitivamente ahora" por operación para categorías Seguro? | "Liberar ahora" + RF-AJU-09 opt-in |
| Q-05 | ¿Limpieza de privacidad (cookies, historial)? | Fuera hasta v2 |
| Q-06 | ¿Se confirma Won't para eliminar idiomas y adelgazar apps universales? | Sí |
| Q-07 | ¿Comprobación de actualizaciones activada por defecto (con pregunta en el onboarding)? | Pregunta en el onboarding, preseleccionada en "Sí" |
| Q-08 | Modelo de negocio (gratis, donaciones, versión pro) y si habrá versión reducida en Mac App Store | Gratis y abierto; sin versión App Store |
| Q-09 | ¿Corte mínimo de Zona Desarrollador en el MVP? | Sí (coordinar con doc 03) |
| Q-10 | Retención del journal y si debe cifrarse | 180 días, sin cifrar (datos locales de rutas) |
| Q-11 | ¿Monitor en el MVP? | No, v1 |
| Q-12 | ¿Soporte de configuración gestionada por MDM para P3? | v2 |

### 11.4 Puntos de macOS a verificar

| # | Punto | Afecta a | Cómo verificar |
|---|---|---|---|
| V-01 | URL del panel de FDA en macOS 27 | CU-01 | `open "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"` en macOS 15, 26 y 27. |
| V-02 | Detección de FDA sin provocar diálogos | CU-01 | Desde la app firmada, con y sin FDA, listar `~/Library/Mail` y `~/Library/Safari`; confirmar EPERM sin diálogo. |
| V-03 | Atribución TCC de la CLI y del agente de programación | CU-06, CU-12 | CLI desde Terminal con y sin FDA del Terminal; agente lanzado por launchd intentando listar `~/Library/Containers/<otra app>`. |
| V-04 | App Translocation y persistencia de FDA | CU-01 | Ejecutar desde el DMG; comprobar la ruta y si el permiso persiste tras mover a `/Applications`. |
| V-05 | Protección de datos de otras apps (macOS 14+) en `~/Library/Containers` | CAT-02 | Sin FDA, listar el contenedor de otra app desde la app firmada; observar diálogo o EPERM. |
| V-06 | ¿Leer `~/.Trash` y restaurar desde ella requiere FDA? | CU-05, CU-08 | `ls ~/.Trash` desde Terminal sin FDA; restaurar un elemento enviado con la API de Papelera desde la app sin FDA. |
| V-07 | "Recuperar" de Finder para elementos enviados con la API | CU-05 | Enviar con la API y probar "Recuperar" en Finder (el deshacer no depende de ello). |
| V-08 | Permiso "Gestión de apps" al mover apps a la Papelera | CU-03 | Mover una app de otro desarrollador desde la app firmada sin ese permiso. |
| V-09 | `tmutil deletelocalsnapshots` / `thinlocalsnapshots` requieren root | CAT-17 | Ejecutar con y sin `sudo` en macOS 15/26/27. |
| V-10 | Política de no materialización de dataless | RF-SEG-10 | `brctl evict <f>`; escanear con la política activa; `ls -lO <f>` debe seguir mostrando `dataless`. |
| V-11 | Detección de clones APFS por fichero | RF-SEG-12 | `cp -c a b` e investigar atributos de `getattrlist(2)` como `ATTR_CMNEXT_PRIVATESIZE` / `ATTR_CMNEXT_CLONE_REFCNT`; si funcionan, el aviso (c) pasa a cifra exacta. |
| V-12 | Enumeración de ítems de inicio de otras apps con API pública | RF-MAN-02 | Probar APIs públicas; comparar con `sudo sfltool dumpbtm`. |
| V-13 | Nombre y URL del panel Ítems de inicio | RF-MAN-02, CU-10 | `open "x-apple.systempreferences:com.apple.LoginItems-Settings.extension"`. |
| V-14 | Flags de `lsregister` | RF-MAN-05 | `/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -h`. |
| V-15 | Segmento `V*` de Mail y ruta `Mail Downloads` | CAT-11, CAT-12 | Con FDA: `ls ~/Library/Mail` y `ls ~/Library/Containers/com.apple.mail/Data/Library/`. |
| V-16 | Ruta de los `.ipsw` descargados por Finder | CAT-16 | Descargar un firmware con Finder y localizarlo con `mdfind -name .ipsw`. |
| V-17 | macOS 27 sin soporte para Intel | RNF-42, A-05 | Página oficial de compatibilidad de macOS 27. |
| V-18 | Cachés con estado en `~/Library/Caches` | CAT-01 | En VM, borrar cada candidata y observar efectos (sesiones, descargas, sincronización). |
| V-19 | `/Library/Updates` protegido por SIP | CAT-16 | `ls -lO /Library/Updates` (flag `restricted`). |
| V-20 | Incompatibilidad del sandbox de la Mac App Store | RNF-35 | Prototipo con sandbox + FDA intentando leer `~/Library/Caches/<otra app>`; decidir en doc 05. |
