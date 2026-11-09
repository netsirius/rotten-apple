# 03 · Zona desarrollador y seguridad de dependencias

> **Nota de consolidación (2026-11-01):** las decisiones, el MVP único y las erratas entre documentos están en [`docs/README.md`](../README.md), que prevalece sobre este documento. En particular, los requisitos marcados «MVP» aquí que no aparezcan en la §4 del README se leen como v1 (D-02, D-03). La revisión crítica está en [06](06-revision-advisor.md). Por petición del autor, la higiene de secretos (§6) se convierte en el módulo **Secretos** de v1: repos, historial de git y árbol de directorios. Su detalle está en [07 §3.1](07-comandos-cli.md).

| Campo | Valor |
|---|---|
| Producto | rotten-apple — limpiador y mantenimiento de macOS (núcleo Rust, licencia MIT) |
| Fecha | 2026-11-01 |
| Plataforma objetivo | macOS 27 (Apple silicon prioritario; Intel donde cambie una ruta, p. ej. `/usr/local` frente a `/opt/homebrew`) |
| Alcance | Zona desarrollador (desorden, librerías y toolchains sin uso) y seguridad de dependencias (vulnerabilidades conocidas en librerías instaladas), más una higiene de seguridad opcional |
| Prefijos | `RF-DEV-xx` (zona desarrollador) · `RF-VUL-xx` (vulnerabilidades e higiene) |
| Relación con otros docs | 02 Requisitos núcleo (`RF-SEG-*`: previsualización, Papelera, deshacer, exclusiones, tamaños honestos; niveles **Seguro / Precaución / Revisar / Protegido**; `RNF-16` no ejecutar contenido analizado; `RNF-17/19` red cero por defecto y transparencia de red) · 04 IA (`RF-IA`, explicación de vulnerabilidades) · 05 Arquitectura (crates `ra-dev` y `ra-vuln`) |
| Regla clean-room | Mole (tw93/mole) es GPL-3.0. De Mole solo se cita el comportamiento descrito en el brief del equipo; **no se ha leído ni se portará su código**. |

**Marcas usadas en este documento**

- **[Verificado aquí]**: comprobado el 2026-11-01 con inspección de solo lectura en un Mac con macOS 27.0.1 (build 26A434), Homebrew 7.0.11, Xcode instalado y git 2.47.1.
- **[VERIFICAR]**: dato incierto o dependiente de versión. Cuando se conoce, se indica el método. Si no, la verificación por defecto es: ejecutar la ayuda del CLI (`<herramienta> --help`), la herramienta de resolución de rutas que ofrezca (`fnm env`, `pipx environment`, `go env`, `pnpm store path`) o consultar la documentación oficial o el LICENSE del proyecto.
- Los niveles de riesgo son los del doc 02. Solo **Seguro** puede preseleccionarse.

---

## 0. Resumen ejecutivo

1. **"Sin uso" se decide con evidencia, no con atime.** Se comprobó en APFS que leer un fichero solo actualiza atime si el atime guardado es *estrictamente anterior* al mtime. Por tanto atime responde "¿se ha leído desde la última escritura?" y no "¿cuándo se usó por última vez?". Las señales válidas son: mtime, historial git (commits y reflog), la marca `CACHEDIR.TAG`, los registros de último uso de cada herramienta (`~/.cargo/.global-cache`, `xcrun simctl runtime delete --notUsedSinceDays`, `INSTALL_RECEIPT.json` de Homebrew) y las referencias cruzadas entre toolchains y ficheros de versión (`.nvmrc`, `rust-toolchain.toml`, …).
2. **Se prefiere el comando nativo antes que borrar.** `brew autoremove`, `brew cleanup`, `pnpm store prune`, `go clean -modcache`, `xcrun simctl delete unavailable`, `docker builder prune`, `hf cache rm`, `ollama rm`… La herramienta conoce sus invariantes: ficheros de solo lectura (Go), hardlinks (pnpm), blobs compartidos (Ollama) o la VM de Docker. Siempre se ejecuta primero en modo simulación (dry-run) y se muestra el comando exacto. Solo si no hay comando nativo se usa el pipeline de borrado `RF-SEG` (Papelera).
3. **Los proyectos se descubren en raíces de desarrollo sin permisos TCC** (`~/Developer`, `~/workspace`, `~/Projects`, `~/src`…). Además se *proponen* raíces deducidas de los proyectos recientes de los IDE. Documentos, Escritorio, Descargas e iCloud solo se escanean si el usuario lo activa. Las protecciones git (ficheros versionados, repos anidados, worktrees, cambios sin commitear, commits sin push, stashes) y la detección de secretos o keystores **se suman** a `RF-SEG`; nunca las relajan.
4. **Vulnerabilidades: OSV.dev como fuente principal, con la menor huella de datos posible.** En modo online solo se envía `(ecosistema, nombre, versión)` deduplicado, nunca rutas ni nombres de proyecto, y se excluyen los paquetes privados. En modo offline se descargan los volcados por ecosistema (medidos: npm 220 MB, PyPI 38 MB, crates.io 3,8 MB… comprimidos) y se actualizan de forma incremental con `modified_id.csv`. Para priorizar se usan CISA KEV (1,7 MB) y EPSS (2,8 MB/día).
5. **Homebrew ya resuelve su parte:** Homebrew 7.0 (septiembre de 2026) incluye `brew vulns` (OSV + parches backport + SBOM por keg) y publica una base de avisos propia en formato OSV, con licencia CC0 (`formulae.brew.sh/api/advisories.json`, 15.979 registros sobre 590 fórmulas). Se recomienda **delegar en `brew vulns --json`** y no reimplementar su emparejador, que en esta máquina ocupa unas 4.300 líneas de Ruby.
6. **Apps y extensiones, con alcance honesto.** Es viable detectar la versión de Electron embebida en las apps y consultarla en OSV (`npm/electron`), las extensiones de VS Code maliciosas conocidas (ecosistema OSV `VSCode`, solo registros `MAL-*`) y el estado de parches de macOS (feed SOFA). En cambio, emparejar apps genéricas con CVE vía CPE **no es fiable** y se pospone. Las extensiones de navegador no se han investigado en este análisis y quedan pendientes para v2.
7. **Reutilizar en Rust:** `cargo-lock`, `rustsec`, `cvss`, `cyclonedx-bom`, `packageurl`, `semver`, `nodejs-semver`, `pep440_rs`, `auditable-info`, `gix`. **Implementar en casa:** structs del esquema OSV, comparadores de versión que falten (RubyGems, Maven, Go pseudo-versiones, Homebrew `_N`) y el escritor SPDX. **No empaquetar** osv-scanner (Go) en el MVP.
8. **Higiene de seguridad (opcional, solo local):** en v1, claves SSH sin passphrase, permisos laxos, ficheros de credenciales en texto plano en rutas conocidas y `.env` versionados. En v2 y con opt-in, secretos sueltos en Descargas/Escritorio y en el historial de shell. Los secretos nunca salen del equipo y nunca se "validan" contra APIs.

---

## 1. Definiciones operativas

### 1.1 "Desorden de desarrollador"

Espacio ocupado por **datos derivados o instalados** que el desarrollador no gestiona activamente y que **(a) se pueden regenerar** o **(b) ya no se usan**. Hay cuatro familias:

| Familia | Ejemplos | Característica |
|---|---|---|
| Artefactos por proyecto | `node_modules/`, `target/`, `.build/`, `build/`, `.venv/`, `Pods/`, `.terraform/`, `__pycache__/` | Viven dentro del árbol del proyecto, casi siempre ignorados por git |
| Cachés globales de gestores de paquetes | `~/.npm/_cacache`, `~/Library/pnpm/store`, `~/.cargo/registry`, `~/Library/Caches/pip`, `~/.cache/uv`, `~/.gradle/caches` | Compartidos entre proyectos; se vuelven a descargar si hace falta |
| Toolchains, runtimes y SDK | Versiones de Node/Python/Ruby/JDK, toolchains de rustup, runtimes de simulador, system images de Android, distribuciones del wrapper de Gradle | Grandes; "sin uso" = no referenciados por ningún proyecto ni por el valor global por defecto |
| Datos de herramientas | Imágenes y build cache de Docker, modelos de IA (Hugging Face, Ollama, LM Studio), cachés de IDE, `workspaceStorage` de VS Code, DerivedData | Mezclan caché con estado; requieren reglas específicas |

**No es desorden** (nunca se propone en lote): código fuente y cualquier fichero versionado; estado (`*.tfstate`, volúmenes de Docker, datos de usuario de AVD o simuladores); Archives de Xcode (contienen los dSYM necesarios para simbolicar crashes); claves y keystores de firma; configuración y credenciales (`~/.m2/settings.xml`, `~/.npmrc`, `~/Library/org.swift.swiftpm/security`).

### 1.2 "Librería o toolchain sin uso"

Se usa una **escala de evidencia**. La UI siempre muestra *por qué* algo se considera sin uso.

| Nivel | Definición | Ejemplos | Tratamiento |
|---|---|---|---|
| **E1 · Huérfano demostrable** | Nada lo referencia, y la herramienta propietaria lo puede confirmar | Dependencia de Homebrew sin dependientes (`brew autoremove -n`); simulador "unavailable"; venv cuyo intérprete base (`home` en `pyvenv.cfg`) ya no existe; entrada de `workspaceStorage` cuyo workspace ya no existe; system image de Android que no usa ningún AVD; output base de Bazel cuyo workspace desapareció | Puede ser **Seguro** si además se regenera |
| **E2 · No referenciado por proyectos conocidos** | La versión no aparece en ningún fichero de versión de los proyectos descubiertos (§2.15) ni es el valor global por defecto | Toolchain `1.93.0` de rustup sin `rust-toolchain.toml` que la fije; Node 18 sin `.nvmrc` que lo pida; `gradle-8.2-bin` sin wrapper que lo use | **Precaución**: puede haber proyectos fuera de las raíces escaneadas |
| **E3 · Sin actividad reciente** | Señales fiables (§1.4) por encima de un umbral | Artefactos de un proyecto sin commits ni ediciones en 120 días; runtime de simulador sin uso en 90 días | Seguro / Precaución según la regenerabilidad |
| **E4 · Sospecha** | Parece no usarse, pero no se puede demostrar | Fórmula de Homebrew instalada a petición; paquete global de npm; binario de `cargo install`; extensión de VS Code | **Solo se informa**; nunca se preselecciona |

**Qué no se puede saber de forma fiable:**

- Si un binario o CLI se ha ejecutado. macOS no registra las ejecuciones por fichero sin auditoría o EndpointSecurity, y atime no sirve (§1.4). El historial de shell es la única pista, y solo con opt-in (RF-DEV-28).
- Si una entrada de caché hará falta mañana.
- Si alguien editó `node_modules` a mano (sin `patch-package`).
- Si existen proyectos fuera de las raíces escaneadas que usan una toolchain.

### 1.3 "Vulnerabilidad en librería instalada"

Un par **(paquete, versión)** presente en el equipo que cae dentro de un rango *affected* de un aviso publicado para el **mismo ecosistema** (OSV, GHSA, RustSec, Homebrew), o un paquete marcado como **malicioso** (`MAL-*`). Se distinguen tres orígenes del inventario, porque su fiabilidad es distinta:

| Origen | Qué es | Ejemplo | Fiabilidad |
|---|---|---|---|
| **Declarado** | Resuelto en un lockfile | `Cargo.lock`, `package-lock.json`, `uv.lock` | Alta si el lockfile es exacto; baja con rangos (`requirements.txt` sin `==`) |
| **Instalado** | Presente en disco | `node_modules/*/package.json`, `site-packages/*.dist-info/METADATA`, kegs de Homebrew, `~/.cargo/.crates2.json` | Alta: es lo que realmente se ejecuta |
| **Embebido** | Metadatos dentro de binarios o apps | `cargo auditable`, buildinfo de Go, `Electron Framework.framework` | Media: solo para lo que deja metadatos |

**Qué se puede detectar:** coincidencias nombre + versión + ecosistema; paquetes maliciosos conocidos; fórmulas de Homebrew afectadas, incluidos los parches que Homebrew aplica (vía `brew vulns` o la base `BREW-*`); el estado de parches de macOS.

**Qué no:**

- Alcanzabilidad, es decir, si el código vulnerable se ejecuta. Solo Go la ofrece, con `govulncheck`, y requiere compilar.
- Vulnerabilidades sin aviso publicado.
- Librerías C embebidas dentro de apps o binarios sin metadatos.
- Casks de Homebrew.
- Apps genéricas sin un identificador fiable (§5.5).

### 1.4 Señales de uso y su fiabilidad

**Experimento atime [Verificado aquí]** (APFS, volumen de datos montado sin `noatime`; `mount` no muestra opciones de atime para `/System/Volumes/Data`):

| Prueba | Estado previo | Tras `cat` | Resultado |
|---|---|---|---|
| 1 | atime 2020-01-01 < mtime (hoy) | atime = ahora | Se actualiza |
| 1b | Segunda lectura inmediata | sin cambio | No se actualiza |
| 2 | atime 25-oct > mtime 20-oct (atime de hace 7 días) | sin cambio | No se actualiza aunque el atime sea antiguo |
| 3 | atime = mtime (20-oct) | sin cambio | No se actualiza con igualdad |
| 3b | atime 1 min > mtime | sin cambio | No se actualiza |

Conclusión: APFS se comporta como un *relatime* sin la regla de 24 horas. Un fichero leído a diario durante un año conserva el atime de la primera lectura posterior a su última modificación. **atime no se usa para decidir "sin uso"**. Como mucho sirve como señal débil de "nunca leído desde que se escribió". Esto también afecta a herramientas de terceros que calculan "último acceso" por atime (p. ej. `hf cache ls`), cuyas cifras deben presentarse con esa salvedad. [VERIFICAR en otras versiones de macOS repitiendo las pruebas con `touch -a -t`/`stat -f %Sa`.]

| Señal | Fiabilidad | Uso |
|---|---|---|
| **mtime** de ficheros o directorios | Alta para "última modificación" | Actividad del proyecto (ediciones) y de los artefactos (una compilación o instalación reciente toca `target/`, `node_modules/.package-lock.json`, etc.) |
| **Git**: fecha del último commit por rama; última entrada de `.git/logs/HEAD` (checkouts, commits, rebases, pulls locales); mtime de `.git/FETCH_HEAD` | Alta | Actividad del proyecto |
| **`CACHEDIR.TAG`** (firma `8a477f597d28d172789f06886806bc55`, [Cache Directory Tagging Spec](https://bford.info/cachedir/)) | Alta para "regenerable" | [Verificado aquí] presente en `target/` (Cargo), `.venv/` (uv), `.ruff_cache/`, `.mypy_cache/`, `.pytest_cache/` y `~/.cache/uv`. **No** está en `node_modules/` |
| **Registros de último uso nativos** | Alta | `~/.cargo/.global-cache` (registro de último uso de Cargo para su GC automático; [VERIFICAR esquema, es una base SQLite interna, abrir solo en lectura]); `xcrun simctl runtime delete --notUsedSinceDays N --dry-run` [Verificado aquí en la ayuda]; Homebrew `INSTALL_RECEIPT.json` (`time`, `installed_on_request`, `runtime_dependencies`) [Verificado aquí] |
| **Procesos en ejecución** (cwd o ficheros abiertos bajo la ruta) | Alta para "activo ahora" | `proc_listpidspath()`/`proc_pidinfo` (libproc), como en el doc 05; un servidor de desarrollo en marcha implica proyecto activo |
| **Spotlight `kMDItemLastUsedDate`** | Media, solo para apps y documentos abiertos con LaunchServices | Útil para IDE, Xcode y Android Studio; inútil para CLI |
| **Historial de shell** (`~/.zsh_history`, `~/.zsh_sessions/*.history`, bash, fish) | Media, y sensible | Solo con opt-in, local, para el nivel E4 → evidencia de uso (RF-DEV-28) |
| **atime** | Baja (ver experimento) | No se usa para decidir |

### 1.5 Niveles de riesgo aplicados a esta zona

Se usan los niveles del doc 02. Esta tabla fija cómo se asignan en la zona desarrollador:

| Nivel (doc 02) | Criterio en la zona desarrollador | Ejemplos |
|---|---|---|
| **Seguro** | Se regenera sin intervención o con una re-descarga *reproducible* (hay lockfile o la caché la gestiona la herramienta), y la actividad está por encima del umbral | DerivedData, `target/` con `CACHEDIR.TAG`, `node_modules` con lockfile de un proyecto inactivo, `__pycache__`, caché de npm/pnpm/uv, versiones antiguas de Homebrew (`brew cleanup`) |
| **Precaución** | Regenerable pero caro (re-descarga de GB, tiempo de compilación alto), o con estado útil, o con regeneración no garantizada | Runtimes de simulador, modelos de IA, imágenes de Docker, toolchains E2, `node_modules`/`.venv` **sin** lockfile, artefactos que dependen de un registro privado, dependencias huérfanas de Homebrew que el usuario podría usar directamente |
| **Revisar** | Puede contener trabajo o datos irrecuperables | Archives de Xcode, AVD y simuladores con datos, volúmenes y contenedores parados de Docker, modelos de Ollama creados localmente, directorios `dist/`/`build/` ambiguos, proyecto completo "abandonado" |
| **Protegido** (además de `RF-SEG-04`) | Nunca seleccionable desde esta zona | `*.tfstate*`, keystores (`*.jks`, `*.keystore`, `~/.android/debug.keystore`), `.git/`, `~/Library/org.swift.swiftpm/security`, credenciales (`~/.m2/settings.xml`, `~/.npmrc`, `token` de Hugging Face, `~/.ollama/id_ed25519`), la toolchain por defecto activa |

---

## 2. Inventario por ecosistema

### 2.0 Reglas transversales

1. **Comando nativo primero**, en dos pasos: (a) dry-run o listado y (b) ejecución tras confirmar (`RF-SEG-01`, `RF-SEG-16`). El plan muestra el comando literal y su salida. Si la herramienta no tiene dry-run, el plan se construye con su comando de listado (`pnpm store path` + tamaño, `ollama list`, `docker system df -v`).
2. **Sin comando nativo**, se borra con el pipeline `RF-SEG` (Papelera por defecto, `RF-SEG-02`). Si la herramienta crea ficheros de solo lectura (Go modcache, output base de Bazel), solo se usa el comando nativo. Así se evita un borrado parcial.
3. **No ejecutar contenido analizado (`RNF-16`)**. Se pueden invocar las herramientas globales (`brew`, `npm`, `cargo`, `xcrun`, `docker`), pero nunca scripts del proyecto: ni `./gradlew`, ni `npm run`, ni plugins de build, ni hooks. Para git, ver §3.5.
4. **Herramienta en uso.** No se limpia si el proceso propietario está trabajando, por ejemplo con `cargo` o `brew` activos, ficheros de bloqueo (`~/.cargo/.package-cache`, `target/debug/.cargo-lock`) o daemons de Gradle (`RF-SEG-08`).
5. **Tamaños honestos (`RF-SEG-12`).** Se cuentan bloques asignados, los hardlinks una sola vez y se usa `ATTR_CMNEXT_PRIVATESIZE` (doc 05) para estimar lo que realmente se libera. Es crítico en esta zona por tres motivos:
   - pnpm enlaza con hardlinks desde su store: borrar `node_modules` libera casi nada hasta `pnpm store prune`.
   - uv usa clones o hardlinks desde su caché [VERIFICAR el modo de enlace por defecto en macOS con `uv venv -v`].
   - `Docker.raw` es un fichero disperso: `ls` muestra el máximo, `du` el uso real.
6. **TCC.** Las rutas de datos de otras apps (`~/Library/Containers/<otra app>`, `~/Library/Group Containers/…`) pueden disparar el aviso de privacidad "acceder a datos de otras apps" [VERIFICAR en macOS 27 con `log stream --predicate 'subsystem == "com.apple.TCC"'`]. Por eso Docker y OrbStack se miden con su CLI y no con `stat`.
7. **Sin privilegios en el MVP (`RF-SEG-13`).** Los runtimes de simulador en `/Library/Developer/CoreSimulator` se gestionan con `simctl`, que gestiona sus propios permisos. Los JDK de `/Library/Java/JavaVirtualMachines` requieren admin y quedan para v1 (helper) o para instrucciones al usuario.
8. **Herramienta ausente o parada.** Si el CLI no está instalado o el daemon está parado (Docker), se informa ("Inicia Docker para analizar") y **no** se tocan sus ficheros directamente.

Columnas de las tablas: **Elemento** · **Ruta(s)** · **Detectar "sin uso"** · **Limpieza preferente** · **Nivel / regenera**.

### 2.1 Homebrew

Prefijo `brew --prefix`: `/opt/homebrew` (Apple silicon) o `/usr/local` (Intel). Cellar, Caskroom y caché en `brew --cache` (`~/Library/Caches/Homebrew`). [Verificado aquí: Homebrew 7.0.11; 136 fórmulas, 33 *leaves*, 5 casks.]

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Dependencias huérfanas | `Cellar/<f>/<v>` | `brew autoremove -n`; `INSTALL_RECEIPT.json` con `installed_on_request: false` y sin dependientes; `brew list --no-installed-on-request` | `brew autoremove` | **Precaución**: el usuario puede usar directamente una dependencia (`python@3.x`, `openssl`, `node`). Para protegerla se ofrece `brew tab --installed-on-request <f>` [Verificado aquí] |
| Versiones antiguas y descargas | Cellar (versiones no enlazadas), `~/Library/Caches/Homebrew` | `brew cleanup -n` (descargas de más de 120 días, ajustable con `HOMEBREW_CLEANUP_MAX_AGE_DAYS`) | `brew cleanup`; `--prune=all` para toda la caché; `-s` para *scrub* | **Seguro** |
| *Leaves* instaladas a petición | Cellar | `brew leaves --installed-on-request` + `time` del recibo; uso real desconocido (E4) | `brew uninstall <f>` (comprueba dependientes) | **Precaución**, solo informativo |
| Casks fantasma | `Caskroom/<c>/<v>` | La entrada existe pero la app ya no está en `/Applications` (el usuario la borró a mano) | `brew uninstall --cask <c>` (**nunca** `--zap` por defecto: borra datos de la app) | Precaución |
| Casks con `auto_updates` | Caskroom | La versión de Caskroom no coincide con la de la app (se actualiza sola) | Informar; `brew outdated --cask --greedy` | Informativo |
| Taps sin uso o no confiables | `$(brew --repository)/Library/Taps` | `brew tap` frente a `brew list --full-name` (ninguna fórmula instalada de ese tap); *Tap Trust* de Homebrew 7 ([docs](https://docs.brew.sh/Tap-Trust)) | `brew untap <tap>` | Seguro |
| Fórmulas obsoletas o deshabilitadas | Cellar | `brew info --json=v2 --installed` → `deprecated`, `disabled`, `deprecation_reason` [VERIFICAR nombres exactos de campo] | Informar; sugerir el sustituto | Señal de mantenimiento y seguridad |
| Desviación del Brewfile | `Brewfile` del usuario | `brew bundle cleanup` (lista lo que no está en el Brewfile) | `brew bundle cleanup --force` solo si el usuario mantiene un Brewfile | Precaución |
| Diagnóstico | — | `brew doctor` | Informar | — |

### 2.2 JavaScript / TypeScript

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Caché de npm | `~/.npm/_cacache` | Tamaño; E3 por mtime | `npm cache verify` (recolecta basura) o `npm cache clean --force` | Seguro |
| npx y logs de npm | `~/.npm/_npx`, `~/.npm/_logs` | mtime | Papelera | Seguro |
| Yarn clásico | `yarn cache dir` (habitualmente `~/Library/Caches/Yarn`) | Tamaño | `yarn cache clean` | Seguro |
| Yarn Berry (≥2) | `~/.yarn/berry/cache`; `.yarn/cache` por proyecto | Si `.yarn/cache` está **versionado** (*zero-installs*) es fuente, no caché | `yarn cache clean --mirror` | Seguro (global); Protegido si está versionado |
| Store de pnpm | `pnpm store path` (en macOS `~/Library/pnpm/store`) [Verificado aquí] | Paquetes no referenciados por ningún proyecto | `pnpm store prune` | Seguro |
| Bun | `~/.bun/install/cache` | Tamaño | `bun pm cache rm` | Seguro |
| Corepack | `~/.cache/node/corepack` [VERIFICAR con `corepack --help`] | Versiones de gestores no referenciadas en `packageManager` | `corepack cache clean` [VERIFICAR] | Seguro |
| `node_modules` por proyecto | dentro del proyecto | §3 (actividad del proyecto) | Papelera; regenerar con `npm ci` / `pnpm install --frozen-lockfile` / `yarn install --immutable` / `bun install --frozen-lockfile` | Seguro **con** lockfile; Precaución sin lockfile o con registro privado |
| Cachés de frameworks | `.next/cache`, `.nuxt`, `.svelte-kit`, `.turbo`, `.parcel-cache`, `.angular/cache`, `node_modules/.cache`, `.expo` | Proyecto inactivo | Papelera | Seguro |
| Globales | `npm prefix -g`/`lib/node_modules` (con nvm, **uno por versión de Node**: `~/.nvm/versions/node/<v>/lib/node_modules`); `~/Library/pnpm/global`; `~/.bun/install/global` | E4 | `npm uninstall -g <p>` | Precaución, informativo |
| **nvm** | `~/.nvm/versions/node/v*`; caché `~/.nvm/.cache` | Versión no referenciada (§2.15) y distinta de `~/.nvm/alias/default` | `nvm uninstall <v>`. nvm es una función de shell: invocarla con `bash -c '. "$NVM_DIR/nvm.sh" && nvm uninstall <v>'`, o mover el directorio a la Papelera (nvm no guarda más estado) | Precaución |
| **fnm** | `FNM_DIR` (lo muestra `fnm env`; en macOS `~/Library/Application Support/fnm` o `~/.local/share/fnm` según la versión) [VERIFICAR] | Igual que nvm; el valor por defecto lo da `fnm default` | `fnm uninstall <v>` | Precaución |
| **Volta** | `~/.volta/tools/image/{node,npm,yarn,pnpm}` | Versiones no fijadas en `package.json` → `volta` ni como valor por defecto (`volta list`) | Papelera (Volta no tiene desinstalación por versión) [VERIFICAR] | Precaución |

### 2.3 Gestores de versiones multi-lenguaje

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| **asdf** | `ASDF_DATA_DIR` (por defecto `~/.asdf/installs/<plugin>/<v>`) | No referenciada en ningún `.tool-versions` (ni en el global `~/.tool-versions`) | `asdf uninstall <plugin> <v>` | Precaución |
| **mise** | `~/.local/share/mise/installs/<tool>/<v>`; caché `~/Library/Caches/mise` [VERIFICAR] | mise registra los ficheros de configuración que ha visto (*tracked configs*) | `mise prune --dry-run` y después `mise prune` (borra versiones no referenciadas) [VERIFICAR flags]; `mise cache clear` | Precaución |

### 2.4 Python

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Caché de pip | `pip cache dir` (`~/Library/Caches/pip`) | Tamaño | `pip cache purge` (por intérprete) | Seguro |
| Caché de uv | `uv cache dir` (`~/.cache/uv`, con `CACHEDIR.TAG`) [Verificado aquí] | Tamaño; entradas no usadas | `uv cache prune` (no usadas) o `uv cache clean` | Seguro (el espacio liberado puede ser menor por clones o hardlinks) |
| Pythons gestionados por uv | `~/.local/share/uv/python` [Verificado aquí] | No referenciado en `.python-version` ni en `requires-python` | `uv python list --only-installed` → `uv python uninstall <v>` | Precaución |
| Herramientas de uv | `~/.local/share/uv/tools` | E4 | `uv tool list` → `uv tool uninstall <t>` | Precaución |
| **pyenv** | `$(pyenv root)/versions/*` | No referenciada y distinta de `pyenv global` | `pyenv uninstall -f <v>` | Precaución |
| venv de proyecto | `.venv/`, `venv/`, `env/` (con `pyvenv.cfg` dentro) | **Roto** si `home =` en `pyvenv.cfg` apunta a un intérprete inexistente (E1); proyecto inactivo (E3) | Papelera; regenerar con `uv sync` / `poetry install` / `pip install -r` | Seguro con lockfile (`uv.lock`, `poetry.lock`, `Pipfile.lock`, `requirements` con `==`); Precaución sin lockfile (puede haber paquetes instalados a mano) |
| virtualenvwrapper / pipenv | `~/.virtualenvs/*`; `~/.local/share/virtualenvs/<proy>-<hash>` (fichero `.project` con la ruta del proyecto) | Huérfano si la ruta del proyecto no existe (E1) | `rmvirtualenv`; `pipenv --rm` desde el proyecto, o Papelera si es huérfano | Precaución |
| Poetry | `~/Library/Caches/pypoetry/{cache,artifacts,virtualenvs}` | Venvs de proyectos inexistentes o inactivos | `poetry env list`/`poetry env remove`; `poetry cache clear --all <repo>` | Seguro (caché); Precaución (venvs) |
| **pipx** | `PIPX_HOME`: en macOS `~/Library/Application Support/pipx`, pero si existe `~/.local/pipx` se usa ese (heredado); lo resuelve `pipx environment` | E4 | `pipx list --json` → `pipx uninstall <p>` | Precaución |
| conda / mamba / micromamba | `~/miniconda3`, `~/miniforge3`, `~/anaconda3`, `~/micromamba`, `~/.conda` (`environments.txt`) [Verificado aquí: `~/miniconda3` y `~/.conda`] | Entornos de `environments.txt` cuya ruta no existe (E1); entornos sin uso (E3 por mtime de `conda-meta/history`) | `conda clean --all --dry-run` → `conda clean --all` (caché de paquetes y tarballs); `conda env remove -n <env>` | Seguro (caché `pkgs`); Precaución (entornos) |
| Cachés de proyecto | `__pycache__/`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`, `.tox/`, `.nox/`, `*.egg-info/` | Proyecto inactivo; `CACHEDIR.TAG` | Papelera | Seguro |
| `build/` y `dist/` de Python | dentro del proyecto | `dist/` puede contener artefactos de release | Papelera, sin preseleccionar | Revisar |

### 2.5 Rust

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Registro de crates | `~/.cargo/registry/{cache,src,index}` [Verificado aquí] | Cargo ≥ 1.88 hace GC automático, a diario: borra lo descargado sin acceso en 3 meses y lo extraído sin acceso en 1 mes; no actúa en modo `--offline`/`--frozen`; se configura con `cache.auto-clean-frequency`. El último uso está en `~/.cargo/.global-cache` | Confiar en el GC nativo. Limpieza manual: `registry/src` (se re-extrae de `cache`, sin red) y luego `registry/cache`. `cargo clean gc` [VERIFICAR si está estabilizado; en 2025 requería `-Zgc`] | Seguro |
| Checkouts git | `~/.cargo/git/{db,checkouts}` | Igual (GC de Cargo) | Igual | Seguro |
| `target/` por proyecto | dentro del proyecto (con `CACHEDIR.TAG`) [Verificado aquí] | §3; artefactos de toolchains ya desinstaladas | `cargo clean`; parcial con `cargo sweep --time <días>` o `cargo sweep --installed` (cargo-sweep, MIT) | Seguro (la recompilación puede ser larga) |
| Toolchains de rustup | `~/.rustup/toolchains/<canal>-<triple>` [Verificado aquí: `1.93.0`, `1.94.0`, `1.95.0`, `stable`, `nightly`] | Versión fijada que ningún `rust-toolchain(.toml)` ni *override* (`rustup override list`, `~/.rustup/settings.toml`) referencia, y que no es `rustup default` → E2. Es el caso típico de "toolchain huérfana". `rust-version` (MSRV) en `Cargo.toml` **no** obliga a tenerla instalada | `rustup toolchain uninstall <tc>` | Precaución |
| Descargas de rustup | `~/.rustup/downloads`, `~/.rustup/tmp` | Siempre | Papelera | Seguro |
| Binarios de `cargo install` | `~/.cargo/bin`, listado en `~/.cargo/.crates2.json` (crate, versión, origen) [Verificado aquí] | E4 | `cargo uninstall <crate>` | Precaución; alimenta el inventario de vulnerabilidades (§5) |
| sccache | `~/Library/Caches/Mozilla.sccache` [VERIFICAR con `sccache --show-stats`] | Tamaño | `sccache --stop-server` + Papelera | Seguro |

### 2.6 Go

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Caché de módulos | `go env GOMODCACHE` (por defecto `~/go/pkg/mod`) | Tamaño. Sus ficheros son de **solo lectura**: `rm -rf` falla a medias | `go clean -modcache` (único método admitido) | Seguro (re-descarga) |
| Toolchains descargadas | `GOMODCACHE/golang.org/toolchain@v0.0.1-go1.X.Y.darwin-arm64` | No referenciada por ninguna directiva `go`/`toolchain` de los `go.mod`/`go.work` descubiertos | Incluidas en `go clean -modcache` (no hay limpieza selectiva nativa) [VERIFICAR] | Precaución |
| Caché de compilación | `go env GOCACHE` (`~/Library/Caches/go-build`) | Go la recorta solo [VERIFICAR el umbral] | `go clean -cache` | Seguro |
| Cachés de test y fuzz | `GOCACHE` | — | `go clean -testcache`; `go clean -fuzzcache` (borra corpus generado: Precaución) | Seguro / Precaución |
| Binarios de `go install` | `go env GOBIN` o `~/go/bin` | E4 | Papelera | Precaución; el buildinfo (`go version -m <bin>`) alimenta §5 |

### 2.7 JVM (Gradle, Maven, JDK)

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Cachés de Gradle | `~/.gradle/caches/{modules-2,transforms-*,build-cache-1,jars-*,<versión>}` [Verificado aquí: `~/.gradle`] | Gradle limpia solo las entradas sin uso (por defecto unos 30 días para lo descargado y 7 para lo generado) y las cachés de versiones sin uso [VERIFICAR los valores vigentes y la configuración con el bloque `caches {}` en un *init script*] | Parar los daemons primero: detectarlos por proceso. **No** ejecutar `./gradlew --stop` (`RNF-16`); usar `gradle --stop` solo si hay un Gradle global. Después, Papelera de las cachés de versiones sin uso | Seguro |
| Distribuciones del wrapper | `~/.gradle/wrapper/dists/gradle-<v>-{bin,all}/<hash>` (100–200 MB cada una) | Versión no referenciada por ningún `gradle/wrapper/gradle-wrapper.properties` (`distributionUrl`) → E2 | Papelera | Precaución |
| JDK aprovisionados por Gradle | `~/.gradle/jdks` | No requeridos por ninguna `languageVersion` de toolchain | Papelera | Precaución |
| Daemons y logs | `~/.gradle/daemon/<v>`, `~/.gradle/native` | Versiones sin uso; logs antiguos | Papelera | Seguro |
| Repositorio local de Maven | `~/.m2/repository` | Sin GC nativo. Heurísticas: directorios `*-SNAPSHOT` antiguos; borrado completo si el usuario lo pide | Papelera (snapshots: Seguro; todo el repo: Precaución por la re-descarga). Por proyecto, `mvn dependency:purge-local-repository` (lo ejecuta el usuario) | Seguro / Precaución |
| `~/.m2/settings.xml`, `settings-security.xml` | — | — | **Nunca** | Protegido (credenciales) |
| Wrapper de Maven | `~/.m2/wrapper/dists` | Sin `.mvn/wrapper/maven-wrapper.properties` que lo referencie | Papelera | Precaución |
| JDK | `/usr/libexec/java_home -V` lista: `/Library/Java/JavaVirtualMachines` (admin), `~/Library/Java/JavaVirtualMachines` (descargas de IntelliJ), `~/.sdkman/candidates/java`, asdf/mise, Homebrew `openjdk@N` | No referenciado por `.java-version` (jenv), `.sdkmanrc`, `.tool-versions` ni toolchains de Gradle; nunca el JDK por defecto | `sdk uninstall java <v>`, `brew uninstall openjdk@N`, Papelera (ámbito usuario); admin → v1 | Precaución |
| Kotlin/Native | `~/.konan` (varios GB) | Sin proyectos KMP activos | Papelera | Precaución |

### 2.8 Ruby

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Gemas (por Ruby) | `gem env gemdir` | Versiones antiguas de una misma gema | `gem cleanup --dryrun` → `gem cleanup` | Seguro |
| rbenv / rvm / chruby | `~/.rbenv/versions`, `~/.rvm/rubies`, `~/.rubies` | No referenciada en `.ruby-version`, en la directiva `ruby` del `Gemfile` ni en `.tool-versions`; distinta del valor global | `rbenv uninstall <v>`, `rvm remove <v>` / `rvm cleanup all` | Precaución |
| Ruby del sistema | `/usr/bin/ruby` | — | **Nunca** | Protegido (`RF-SEG-04`) |
| Bundler por proyecto | `vendor/bundle` (comprobar si está versionado), `.bundle/` | Proyecto inactivo | `bundle clean --dry-run` / Papelera | Seguro si no está versionado |
| Cachés de Bundler y RubyGems | `~/.bundle/cache`, `~/.gem/specs` | Tamaño | Papelera | Seguro |

### 2.9 Apple: Xcode, CocoaPods, Carthage, SwiftPM

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| DerivedData | `~/Library/Developer/Xcode/DerivedData/<Proyecto>-<hash>` [Verificado aquí] | Huérfano si el workspace de su `info.plist` (`WorkspacePath` [VERIFICAR la clave]) no existe (E1); E3 por mtime | Papelera, nunca durante una compilación (procesos Xcode o `xcodebuild`) | Seguro (incluye `SourcePackages`: re-descarga de SPM) |
| Archives | `~/Library/Developer/Xcode/Archives/<fecha>/*.xcarchive` | Antigüedad | Solo revisión manual; sugerir antes exportar los dSYM | **Revisar** (dSYM para simbolicar; envíos a App Store) |
| DeviceSupport | `~/Library/Developer/Xcode/{iOS,watchOS,tvOS,visionOS} DeviceSupport/<versión (build)>` [Verificado aquí: iOS] | Conservar la versión más reciente de cada plataforma; las anteriores son E3 | Papelera | Seguro (se regenera al conectar el dispositivo; tarda minutos) |
| Simuladores no disponibles | `~/Library/Developer/CoreSimulator/Devices/<UDID>` [Verificado aquí] | Runtime inexistente (E1) | `xcrun simctl delete unavailable` | Seguro |
| Otros simuladores | Igual | `xcrun simctl list devices --json` (campo de último arranque [VERIFICAR nombre, p. ej. `lastBootedAt`]) | `xcrun simctl delete <UDID>` | Revisar (datos de apps de prueba) |
| Runtimes de simulador | `/Library/Developer/CoreSimulator/{Volumes,Images,Cryptex}` [Verificado aquí] | `xcrun simctl runtime list`; `xcrun simctl runtime delete --notUsedSinceDays <N> --dry-run` [Verificado aquí en la ayuda] | `xcrun simctl runtime delete --notUsedSinceDays <N>` o `<id>` | Precaución (re-descarga de varios GB con Xcode o `xcodebuild -downloadPlatform`) |
| Cachés de CoreSimulator | `~/Library/Developer/CoreSimulator/Caches` | Siempre | Papelera | Seguro |
| Simuladores de Previews de SwiftUI | `~/Library/Developer/Xcode/UserData/Previews` (no existe en esta máquina) | Siempre | `xcrun simctl --set previews delete all` [VERIFICAR sintaxis con Xcode actual] | Seguro |
| XCTestDevices | `~/Library/Developer/XCTestDevices` [Verificado aquí] | Clones para tests en paralelo | Papelera, sin Xcode en ejecución | Seguro |
| Cachés y descargas de Xcode | `~/Library/Caches/com.apple.dt.Xcode`, `~/Library/Developer/DVTDownloads` [Verificado aquí] | Tamaño | Papelera | Seguro |
| DeviceLogs | `~/Library/Developer/Xcode/DeviceLogs` [Verificado aquí] | Antigüedad | Papelera | Precaución (logs de crash) |
| Versiones antiguas de Xcode | `/Applications/Xcode*.app` (más de 10 GB cada una) | No referenciada por `.xcode-version` (xcodes) y distinta de `xcode-select -p` | Desinstalador de apps (doc 02) | Precaución |
| CocoaPods: caché | `~/Library/Caches/CocoaPods` | Tamaño | `pod cache clean --all` | Seguro |
| CocoaPods: repos de specs | `~/.cocoapods/repos` [Verificado aquí: `~/.cocoapods`] | El repo git heredado `master` ocupa varios GB y no hace falta desde la CDN (CocoaPods ≥ 1.8) | `pod repo list` → `pod repo remove master` | Seguro |
| CocoaPods: `Pods/` | dentro del proyecto | Proyecto inactivo; hay `Podfile.lock`; **no** versionado | Papelera; regenerar con `pod install` | Seguro (Protegido si está versionado). Contexto: CocoaPods anunció que su *trunk* pasa a solo lectura [VERIFICAR fecha en blog.cocoapods.org], lo que dejará cachés obsoletas al migrar a SPM |
| Carthage | `~/Library/Caches/org.carthage.CarthageKit`; `Carthage/Build`, `Carthage/Checkouts` por proyecto | Proyecto inactivo; no versionado | Papelera | Seguro |
| SwiftPM: caché | `~/Library/Caches/org.swift.swiftpm` | Tamaño | `swift package purge-cache` | Seguro |
| SwiftPM: configuración y seguridad | `~/Library/org.swift.swiftpm/{configuration,security}` | — | **Nunca** (huellas TOFU de paquetes) | Protegido |
| SwiftPM: `.build/` | dentro del proyecto (con `Package.swift`) | Proyecto inactivo | `swift package clean` o Papelera | Seguro |

### 2.10 Android

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| SDK | `ANDROID_HOME` (`~/Library/Android/sdk`) [Verificado aquí]: `system-images/`, `platforms/`, `build-tools/`, `ndk/<v>`, `cmake/`, `sources/`, `emulator/` | Una system image sin ningún AVD que la use (`image.sysdir.1` en `config.ini`) es E1. Son E2: las versiones de NDK no referenciadas por `ndkVersion` y las `platforms` que no coinciden con ningún `compileSdk`. Los `build-tools` anteriores al mínimo usado también son E2 [VERIFICAR la versión de NDK por defecto de cada versión de AGP] | `sdkmanager --list_installed` → `sdkmanager --uninstall "<ruta;id>"` | Precaución (re-descarga) |
| AVD | `~/.android/avd/<n>.avd` + `<n>.ini` | E3 por mtime de `userdata-qemu.img` o de los snapshots | `avdmanager list avd` → `avdmanager delete avd -n <n>` | Revisar (datos del emulador) |
| Cachés | `~/.android/cache`, `~/.android/build-cache` (heredado) | Siempre | Papelera | Seguro |
| Android Studio (versiones antiguas) | `~/Library/Caches/Google/AndroidStudio<v>`, `~/Library/Logs/Google/AndroidStudio<v>`, `~/Library/Application Support/Google/AndroidStudio<v>` | Versión anterior a la instalada más reciente | Papelera | Seguro (cachés y logs); Precaución (Application Support) |
| Por proyecto | `build/`, `app/build/`, `.gradle/`, `.cxx/`, `.externalNativeBuild/` | Proyecto inactivo | Papelera | Seguro |
| Keystores | `*.jks`, `*.keystore`, `~/.android/debug.keystore` | — | **Nunca** (`debug.keystore` se regenera, pero cambia el SHA-1 registrado en las API keys) | Protegido |

### 2.11 Contenedores

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Docker Desktop: imágenes | VM: `~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw` (disperso; **no** se toca ni se mide con `stat` por TCC) | `docker system df -v`; imágenes *dangling*; imágenes sin contenedores y sin uso en N días (`--filter until=`) | `docker image prune` (dangling, Seguro); `docker image prune -a --filter "until=720h"` (Precaución) | Seguro / Precaución |
| Build cache | Dentro de la VM | `docker system df` | `docker builder prune --filter until=<h>` | Seguro |
| Contenedores parados | Dentro de la VM | `docker ps -a --filter status=exited` | `docker container prune` | Revisar (pueden tener estado) |
| Volúmenes | Dentro de la VM | `docker volume ls -f dangling=true` | `docker volume prune` (en Docker ≥ 23 solo anónimos; `-a` todos) | **Revisar**: bases de datos y estado; nunca en lote |
| Recuperar espacio en macOS | — | Un `prune` libera espacio dentro de la VM, pero `Docker.raw` no tiene por qué encogerse | Informar: Docker Desktop › Troubleshoot / Resources [VERIFICAR el comportamiento de *discard* o TRIM en la versión actual] | — |
| OrbStack | `~/Library/Group Containers/HUAQ24HBR6.dev.orbstack/…` [VERIFICAR] | CLI `docker` sobre el contexto `orbstack` | Igual que Docker; su disco crece y encoge de forma dinámica [VERIFICAR] | Igual |
| Colima / Lima | `~/.colima/<perfil>`, `~/.lima/<vm>/diffdisk` | `colima list`; CLI `docker` | `docker … prune`; `colima prune` (descargas en caché) [VERIFICAR]; `colima delete` = borra la VM | Seguro / Revisar |
| Podman | `podman machine inspect` [VERIFICAR la ruta de la máquina] | `podman system df` | `podman system prune` | Igual que Docker |
| Varios motores | — | `docker context ls` | Iterar por contexto | — |
| minikube / kind | `~/.minikube/{cache,machines}` | Clústeres parados | `minikube delete --all --purge` (Revisar); kind = contenedores | Revisar |

### 2.12 IDE y editores

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Familia VS Code: cachés | `~/Library/Application Support/<App>/{Cache,Code Cache,GPUCache,DawnGraphiteCache,DawnWebGPUCache,CachedExtensionVSIXs,Crashpad,logs}` con `<App>` ∈ {Code, Code - Insiders, VSCodium, Cursor, Windsurf, …} (se detectan por su estructura) [Verificado aquí: Code] | Siempre (logs antiguos por mtime) | Papelera con la app cerrada | Seguro |
| Familia VS Code: `CachedData/<commit>` | Igual | Commit distinto del de la app instalada (`product.json` → `commit`) → E1 | Papelera | Seguro |
| Familia VS Code: `User/workspaceStorage/<hash>` | Igual | Huérfano si la carpeta o el workspace de `workspace.json` ya no existe (E1) | Papelera | Precaución (estado de UI por workspace). **No** tocar `User/History` (historial local) |
| Extensiones | `~/.vscode/extensions`, `~/.cursor/extensions`, `~/.windsurf/extensions`… [Verificado aquí] | Versiones antiguas de la misma extensión que no figuran en `extensions.json`; carpetas listadas en `.obsolete` (E1) | Papelera con la app cerrada; desinstalar con `code --uninstall-extension <id>` (E4) | Seguro (versiones obsoletas); Precaución (desinstalar) |
| JetBrains | `~/Library/Caches/JetBrains/<Producto><AAAA.N>`, `~/Library/Logs/JetBrains/…`, `~/Library/Application Support/JetBrains/…` | Directorios de versiones anteriores a la instalada más reciente del mismo producto (E1). El propio IDE ofrece limpiar "directorios sobrantes" [VERIFICAR nombre] | Papelera | Seguro (Caches y Logs); Precaución (Application Support: ajustes y plugins) |
| JetBrains Toolbox | `~/Library/Application Support/JetBrains/Toolbox/apps` | Builds anteriores conservados para *rollback* | Ajuste de Toolbox o Papelera | Precaución |

### 2.13 Flutter/Dart, Terraform y Bazel

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Pub cache | `~/.pub-cache` [Verificado aquí] | Tamaño; `global_packages` es E4 | `dart pub cache clean` (o `flutter pub cache clean`) | Seguro |
| SDK de Flutter | `<flutter>/bin/cache` | Artefactos de motor de plataformas no usadas | Papelera; regenerar con `flutter precache` | Seguro |
| FVM | `~/fvm/versions` [VERIFICAR la ruta en FVM 3] | No referenciada en `.fvmrc` ni en `.fvm/fvm_config.json` | `fvm remove <v>` | Precaución |
| Flutter por proyecto | `build/`, `.dart_tool/` (con `pubspec.yaml`) | Proyecto inactivo | `flutter clean` o Papelera | Seguro |
| Terraform por proyecto | `.terraform/` (providers y módulos) | Proyecto inactivo; existe `.terraform.lock.hcl` | Papelera; regenerar con `terraform init` | Seguro con lock; Precaución sin lock |
| Estado de Terraform | `terraform.tfstate*`, `.terraform.tfstate.lock.info` | — | **Nunca** | Protegido |
| Caché de plugins de Terraform | `TF_PLUGIN_CACHE_DIR` (p. ej. `~/.terraform.d/plugin-cache`) | Tamaño | Papelera | Seguro |
| tfenv / tenv | `~/.tfenv/versions`, `~/.tenv` [VERIFICAR] | No referenciada por `.terraform-version` | `tfenv uninstall <v>` | Precaución |
| Bazel: output bases | `/private/var/tmp/_bazel_$USER/<md5>/` (macOS) | Huérfano si el workspace que indica su fichero `DO_NOT_BUILD_HERE` ya no existe (E1) [VERIFICAR el nombre y el contenido del fichero] | Desde el workspace, el usuario ejecuta `bazel clean --expunge` (arranca Bazel, posible `RNF-16`). Para huérfanos: Papelera tras restaurar permisos de escritura (Bazel deja ficheros de solo lectura) | Seguro |
| Bazel: install bases y Bazelisk | `/private/var/tmp/_bazel_$USER/install/<md5>`; `~/Library/Caches/bazelisk` [VERIFICAR] | Versiones no referenciadas por `.bazelversion` | Papelera | Seguro |

### 2.14 Modelos de IA y ML

| Elemento | Ruta(s) | Detectar "sin uso" | Limpieza preferente | Nivel / regenera |
|---|---|---|---|---|
| Hugging Face | `HF_HOME` (por defecto `~/.cache/huggingface`): `hub/models--<org>--<nombre>/{blobs,snapshots,refs}`, `xet/` [Verificado aquí] | `hf cache ls` (sustituye a `huggingface-cli scan-cache`). Su "último acceso" se basa en atime y no es fiable en APFS (§1.4); usar el mtime de `snapshots/*` | `hf cache rm <repo> --dry-run` → `hf cache rm`; `hf cache prune` (revisiones separadas) | Precaución (re-descarga de GB; los modelos con acceso restringido requieren autenticación; un modelo retirado del Hub es irrecuperable) |
| Credenciales de HF | `~/.cache/huggingface/{token,stored_tokens}` [Verificado aquí: existen] | — | **Nunca** | Protegido |
| Ollama | `~/.ollama/models/{blobs,manifests}` [Verificado aquí] | `ollama list` (nombre, tamaño, modificado). Sin dato de último uso; mtime del manifiesto | `ollama rm <modelo>`. **Nunca** borrar blobs sueltos: se comparten entre modelos | Precaución; Revisar si el modelo se creó en local (`ollama create`, no se puede volver a descargar) |
| Clave de identidad de Ollama | `~/.ollama/id_ed25519` [Verificado aquí] | — | **Nunca**; se incluye en la lista blanca de la higiene (§6) | Protegido |
| LM Studio | `~/.lmstudio/models/<publisher>/<repo>/*` [Verificado aquí]; heredado `~/.cache/lm-studio/models` | mtime | `lms ls` [VERIFICAR si existe `lms rm`]; Papelera (son ficheros planos) | Precaución |
| Otros (Could) | `~/.cache/torch/hub`, `~/.cache/whisper`, carpetas de modelos de ComfyUI o A1111 (ruta elegida por el usuario) | mtime | Papelera | Precaución |

Los modelos **nunca se preseleccionan**: son caros de volver a descargar y difíciles de verificar.

### 2.15 Ficheros que anclan versiones (para detectar toolchains "no referenciadas")

| Ecosistema | Ficheros y campos | Tipo |
|---|---|---|
| Node | `.nvmrc`, `.node-version`, `package.json` → `engines.node`, `package.json` → `volta.node`, `.tool-versions` (`nodejs`), `mise.toml`/`.mise.toml` (`[tools] node`) | Exacto o rango |
| Gestores JS | `package.json` → `packageManager` (Corepack), `volta.pnpm`/`volta.yarn` | Exacto |
| Python | `.python-version`, `pyproject.toml` → `requires-python`, `uv.lock` → `requires-python`, `Pipfile` → `[requires] python_version`, `runtime.txt`, `.tool-versions` | Exacto o rango |
| Rust | `rust-toolchain.toml` / `rust-toolchain`, *overrides* de rustup (`~/.rustup/settings.toml`), `Cargo.toml` → `rust-version` (MSRV; solo informativo) | Exacto |
| Go | `go.mod` y `go.work` → `go`, `toolchain`; variable `GOTOOLCHAIN` | Mínimo o exacto |
| JVM | `.java-version`, `.sdkmanrc`, `.tool-versions`, toolchain `languageVersion` (Gradle), `gradle-wrapper.properties` → `distributionUrl`, `.mvn/wrapper/maven-wrapper.properties` | Exacto o mayor |
| Ruby | `.ruby-version`, `Gemfile` → `ruby`, `.tool-versions` | Exacto o rango |
| Flutter | `.fvmrc`, `.fvm/fvm_config.json`, `pubspec.yaml` → `environment.flutter` | Exacto o rango |
| Terraform | `.terraform-version`, `required_version` | Exacto o rango |
| Bazel | `.bazelversion` | Exacto |
| Xcode | `.xcode-version` | Exacto |
| Android | `compileSdk`, `ndkVersion`, `buildToolsVersion` en `build.gradle(.kts)` | Exacto |

**Reglas de protección de versiones:**

- Un ancla exacta protege esa versión.
- Un rango protege la versión instalada **más alta** que lo satisface, y se avisa si hay varias.
- Los valores globales por defecto (`rustup default`, alias `default` de nvm, `pyenv global`, `~/.tool-versions`, `java_home`, `xcode-select -p`) siempre se protegen.
- Si un ancla no se puede interpretar, se protege todo lo que podría satisfacerla (criterio conservador).
- La UI muestra "Referenciado por: 3 proyectos" con su lista.

---

## 3. Descubrimiento de proyectos

### 3.1 Raíces de búsqueda

| Tipo | Rutas | Política |
|---|---|---|
| **Por defecto** (solo si existen) | `~/Developer` (convención de Apple), `~/workspace`, `~/Workspace`, `~/Projects`, `~/projects`, `~/src`, `~/code`, `~/Code`, `~/dev`, `~/git`, `~/repos`, `~/GitHub`, `~/go/src` (GOPATH heredado), `~/AndroidStudioProjects`, `~/Sites` | Activas sin preguntar: no requieren permisos TCC |
| **Sugeridas por evidencia** | Carpetas padre de proyectos recientes según: `workspace.json` de `workspaceStorage` (VS Code y derivados), `recentProjects.xml` de JetBrains (`~/Library/Application Support/JetBrains/<IDE>/options/`), `WorkspacePath` de DerivedData y los *tracked configs* de mise | Se **proponen** al usuario como raíces candidatas; no se escanean sin aceptación. Es una mejora frente a raíces fijas: encuentra proyectos sin adivinar nombres de carpeta |
| **Opt-in** | `~/Documents`, `~/Desktop`, `~/Downloads` (TCC "Archivos y carpetas"); volúmenes externos `/Volumes/*` | Solo con activación explícita y explicación del aviso de macOS |
| **Excluidas siempre** | iCloud Drive (`~/Library/Mobile Documents`) y File Provider (`RF-SEG-10`); `~/Library` (salvo las rutas de §2), `~/.Trash`, volúmenes de red, rutas de `RF-SEG-04` | — |

### 3.2 Marcadores de proyecto y catálogo de artefactos

**Marcadores de proyecto:** `.git` (directorio **o fichero** `gitdir:`), `package.json`, `deno.json`, `Cargo.toml`, `pyproject.toml`, `setup.py`, `requirements*.txt`, `Pipfile`, `go.mod`, `go.work`, `pom.xml`, `build.gradle(.kts)`, `settings.gradle(.kts)`, `Gemfile`, `Podfile`, `Cartfile`, `Package.swift`, `*.xcodeproj`, `*.xcworkspace`, `pubspec.yaml`, `composer.json`, `mix.exs`, `*.csproj`, `CMakeLists.txt`, `build.zig`, `*.tf`, `WORKSPACE`/`MODULE.bazel`, `flake.nix`.

**Artefactos.** Cada uno requiere un **marcador hermano** que lo valide. Sin él, no se propone.

| Directorio | Validación requerida | Regenerar con | Nivel si el proyecto está inactivo |
|---|---|---|---|
| `node_modules/` | `package.json` hermano | `npm ci` / `pnpm i --frozen-lockfile` / `yarn install --immutable` / `bun install --frozen-lockfile` | Seguro con lockfile; Precaución sin él |
| `.next/`, `.nuxt/`, `.svelte-kit/`, `.turbo/`, `.parcel-cache/`, `.angular/cache/`, `.expo/`, `coverage/`, `.nyc_output/` | `package.json` | build o test | Seguro |
| `dist/`, `build/` (JS) | `package.json` **y** ignorado por git **y** sin ficheros versionados | `npm run build` (lo ejecuta el usuario) | Revisar (ambiguo; puede ser lo que se publica) |
| `target/` | `Cargo.toml` + `target/CACHEDIR.TAG`; o `pom.xml` (Maven) | `cargo build` / `mvn package` | Seguro |
| `.build/` | `Package.swift` | `swift build` | Seguro |
| `build/`, `.gradle/`, `.cxx/`, `.externalNativeBuild/` | `build.gradle(.kts)` o `settings.gradle(.kts)` | Gradle | Seguro |
| `build/`, `.dart_tool/` | `pubspec.yaml` | `flutter pub get` / build | Seguro |
| `Pods/` | `Podfile` + `Podfile.lock`, no versionado | `pod install` | Seguro |
| `Carthage/Build/`, `Carthage/Checkouts/` | `Cartfile.resolved`, no versionado | `carthage bootstrap` | Seguro |
| `.venv/`, `venv/` | `pyvenv.cfg` dentro + manifiesto Python hermano | `uv sync` / `poetry install` / `pip install -r` | Seguro con lockfile; Precaución sin él |
| `__pycache__/`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`, `.tox/`, `.nox/` | Cualquier marcador Python o `CACHEDIR.TAG` | Automático | Seguro |
| `vendor/` | `go.mod` (`vendor/modules.txt`), `composer.json` o `Gemfile` (`vendor/bundle`); **no versionado** | `go mod vendor` / `composer install` / `bundle install` | Seguro si no está versionado; si lo está, no se ofrece |
| `.terraform/` | `*.tf` (+ `.terraform.lock.hcl`) | `terraform init` | Seguro con lock |
| `cmake-build-*/`, `build/` (CMake) | `CMakeLists.txt` + `CMakeCache.txt` dentro | cmake | Seguro |
| `.zig-cache/`, `zig-cache/`, `zig-out/` | `build.zig` | `zig build` | Seguro |
| `_build/`, `deps/` | `mix.exs` | `mix deps.get` | Seguro |
| `.stack-work/`, `dist-newstyle/` | `stack.yaml` / `*.cabal` | stack o cabal | Seguro |
| **Regla genérica** | Cualquier directorio con `CACHEDIR.TAG` válido dentro de un proyecto | — | Seguro, aunque no esté en el catálogo |

El catálogo se distribuye embebido en la app firmada (`RF-SEG-17`). El usuario puede añadir reglas locales (RF-DEV-27). `kondo` (Rust, MIT) es una referencia legítima para contrastar el catálogo.

### 3.3 Algoritmo y rendimiento

1. **Recorrido paralelo** por raíz con *work-stealing*: `rayon` y `ignore` (doc 05), con una segunda iteración que use `getattrlistbulk(2)` para leer nombre, tipo, tamaño asignado, mtime, inode y flags en una sola llamada por directorio (doc 05).
2. **Al encontrar un marcador**, se registra el proyecto y se **sigue bajando**, porque los monorepos tienen paquetes anidados.
3. **Al encontrar un artefacto validado**, se registra y se **poda** (no se entra en él). Su tamaño se calcula después, en segundo plano, y los resultados llegan a la UI en streaming.
4. **Límites:**
   - Profundidad: 8 bajo cada raíz por defecto, configurable de 1 a 20. Mole usa 6; se elige 8 por los monorepos (`apps/*/packages/*`).
   - Presupuesto de tiempo con resultados parciales.
   - No se siguen symlinks ni se cruzan dispositivos (`RF-SEG-06`).
   - Se saltan los ficheros *dataless* (`RF-SEG-10`) y no se entra en bundles (`*.app`, `*.xcarchive`).
5. **Índice persistente**: proyecto → marcadores, artefactos, tamaños, última actividad y último *event ID* de FSEvents. En la siguiente ejecución se reproduce el historial de FSEvents desde ese ID (`FSEventStreamCreate` con `sinceWhen`) y solo se re-escanean los subárboles que cambiaron. Si el historial ya no cubre ese ID, se hace un escaneo completo.
6. **Objetivos de rendimiento** (supuestos a validar con *benchmark* en un M1 con SSD; ver RF-DEV-26):

   | Métrica | Objetivo |
   |---|---|
   | Primer escaneo de raíces con unos 50 proyectos y 1 M de entradas | < 60 s en frío, < 20 s en caliente |
   | Escaneo incremental | < 5 s |
   | Memoria | < 300 MB |
   | Respuesta a cancelar | < 500 ms |

### 3.4 Actividad y proyectos "abandonados"

`última_actividad` = máximo de:

- fecha de committer del último commit en cualquier rama local (`gix` o `git for-each-ref --sort=-committerdate --count=1 --format='%(committerdate:unix)' refs/heads`);
- marca de tiempo de la última entrada de `.git/logs/HEAD`;
- mtime máximo de los ficheros **no ignorados** hasta profundidad 2, excluyendo artefactos y `.git` (muestreo acotado); recoge ediciones sin commitear;
- mtime de los "sellos" de compilación o instalación (`node_modules/.package-lock.json`, el más reciente de `target/*/.fingerprint`, `.venv/pyvenv.cfg`, `Pods/Manifest.lock`); recoge compilaciones recientes;
- "ahora", si hay un proceso con cwd dentro del proyecto.

Sin git, solo cuentan las señales de mtime.

| Estado | Inactividad | Tratamiento por defecto |
|---|---|---|
| Activo | < 7 días | Se muestra pero **nunca se preselecciona** (mismo umbral que Mole) |
| Reciente | 7–30 días | Se muestra sin marcar |
| Inactivo | 30–180 días | Se preseleccionan sus artefactos de nivel **Seguro** |
| Dormido | 180–365 días | Igual, con el tamaño total destacado |
| Abandonado | > 365 días | Vista propia: "Limpiar todos los artefactos". Nunca se ofrece borrar el proyecto en sí; archivarlo o comprimirlo queda para v2 |

Los umbrales son configurables. El valor por defecto (30 días) es **más conservador** que los 7 días de Mole: se prefiere no molestar al usuario a liberar algo más de espacio.

### 3.5 Protecciones (se suman a `RF-SEG`)

| # | Protección | Comprobación | Efecto |
|---|---|---|---|
| P1 | **Ficheros versionados** | `git ls-files -z -- <dir>` no vacío | El artefacto no se ofrece ("contiene N ficheros versionados"). Cubre `dist/` o `vendor/` versionados, `.yarn/cache` (*zero-installs*) y `Pods/` versionado |
| P2 | **Ignorado por git** | `git check-ignore -q <dir>` | Si **no** está ignorado ni versionado, se muestra sin marcar con el aviso "no está en .gitignore" |
| P3 | **Cambios sin commitear** | `git status --porcelain=v2 -z` no vacío | Distintivo "cambios locales". Bloquea las acciones sobre el proyecto entero; no bloquea artefactos ignorados |
| P4 | **Commits sin push / ramas sin upstream** | `git for-each-ref --format='%(refname:short) %(upstream) %(upstream:track)' refs/heads`; `git log --branches --not --remotes --oneline` | Distintivo "trabajo sin respaldo remoto" |
| P5 | **Stashes** | `refs/stash` o `git stash list` | Distintivo |
| P6 | **Repos anidados** | Cualquier `.git` (directorio o fichero) dentro del artefacto candidato | Se bloquea ese artefacto; el repo anidado se trata como proyecto propio |
| P7 | **Worktrees y submódulos** | `.git` como fichero `gitdir: <ruta>` [Verificado aquí: hay proyectos con `.git` fichero en `~/workspace/projects`]; `git worktree list --porcelain` desde el repo principal | Cada worktree es un proyecto con sus artefactos. Nunca se borra un directorio de worktree. Si el usuario borró uno a mano, se sugiere `git worktree prune --dry-run`. Un submódulo apunta a `.git/modules` del superproyecto y forma parte de él |
| P8 | **Secretos y claves en el artefacto** | Nombres `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.p8` (claves de App Store Connect), `*.jks`, `*.keystore`, `id_rsa*`, `id_ed25519*`, `*.mobileprovision`, `*service-account*.json`, `credentials*.json`, `*.tfstate*` | Se bloquea y se avisa; decide el usuario. Generaliza la protección de Mole para pares de claves de despliegue |
| P9 | **Proceso activo** | cwd de procesos bajo el proyecto (`proc_pidinfo`) y ficheros de bloqueo de la herramienta (`target/*/.cargo-lock`, procesos de Gradle o Xcode) | Se omite (`RF-SEG-08`) |
| P10 | **Regenerabilidad** | Hay lockfile; el lockfile resuelve desde registros públicos (`resolved` ≠ `registry.npmjs.org`, `source` ≠ crates.io, índices pip propios) | Sin lockfile o con registro privado → Precaución, con el aviso "regenerar requiere acceso a <host>" |
| P11 | **Exclusiones del usuario** | Lista de exclusiones de `RF-SEG` + fichero opcional de proyecto `.rotten-apple.toml` (`keep = ["dist"]`) | Prevalecen siempre |

**Ejecutar git sin ejecutar código del repo (`RNF-16`):**

- `git status` puede invocar `core.fsmonitor`, filtros *clean* (p. ej. git-lfs) y otros comandos definidos en `.git/config`. Un repo descargado como zip con `.git` incluido podría traer configuración maliciosa.
- Por eso las lecturas se hacen con **`gix`** (Rust puro; no ejecuta comandos externos) cuando la operación está soportada [VERIFICAR la madurez de `gix status` para *index-worktree*].
- Como alternativa se usa el CLI con `GIT_OPTIONAL_LOCKS=0` (no tomar `index.lock`), `-c core.fsmonitor=false`, `-c core.hooksPath=/dev/null` y la configuración del sistema desactivada [VERIFICAR qué filtros siguen ejecutándose y si deben neutralizarse].
- `/usr/bin/git` es un *shim*: si faltan las Command Line Tools, abre el diálogo de instalación. Por eso se comprueba antes `xcode-select -p`, o se usa directamente `gix`.

---

## 4. Dependencias "sin uso" dentro de un proyecto

| Ecosistema | Herramienta | Licencia | Método | Ejecuta código del proyecto | Notas |
|---|---|---|---|---|---|
| JS/TS | **knip** | ISC | Análisis estático con plugins por framework; dependencias, ficheros y exports sin uso | **Sí**: carga ficheros de configuración JS/TS (knip, eslint…) | Sustituto de depcheck, que está **archivado** (febrero de 2025) |
| Rust | **cargo-machete** | MIT | Búsqueda textual de `use` y nombres de crates | No | Muy rápido; falsos positivos con macros y crates renombrados (`[package.metadata.cargo-machete] ignored`) |
| Rust | **cargo-shear** | MIT | Parseo con `syn`; tiene `--fix` | No | Más preciso que machete |
| Rust | **cargo-udeps** | MIT/Apache-2.0 | Compila (nightly) | **Sí** (`build.rs`, macros) | Preciso y lento |
| Python | **deptry** | [VERIFICAR licencia; el repo `fpgmaas/deptry` se movió] | Compara imports con `pyproject` | No (lee el código fuente) | Bueno para proyectos con `pyproject` |
| Go | `go mod tidy -diff` (Go ≥ 1.23) | Nativo (BSD) | Muestra lo que `tidy` cambiaría | Usa la toolchain; puede descargar módulos | Sin terceros |
| Maven | `mvn dependency:analyze` | Nativo (Apache-2.0) | Bytecode | **Sí** (build) | — |
| Gradle | dependency-analysis-gradle-plugin | Apache-2.0 [VERIFICAR] | Plugin | **Sí** | — |
| Swift | Periphery | MIT [VERIFICAR] | Código muerto (indexa con compilación) | **Sí** | — |

**Recomendación:**

1. **Solo informar** (RF-DEV-24, Could, v2). No se modifican manifiestos (nunca `--fix`).
2. Por `RNF-16`, rotten-apple **solo ejecuta por sí mismo herramientas pasivas** (cargo-machete, cargo-shear, deptry) **si ya están instaladas** y con acción explícita por proyecto. Para el resto, muestra el comando para que el usuario lo ejecute en su terminal. Nunca instala herramientas ni usa `npx` (descarga y ejecuta código: riesgo de cadena de suministro).
3. **Valor real.** El ahorro de disco es bajo: una dependencia sin uso solo ocupa su parte de `node_modules`. Su valor está en la higiene y en reducir la superficie de ataque. Se cruza con §5: si una dependencia es **vulnerable y posiblemente sin uso**, la sugerencia es eliminarla en lugar de actualizarla.
4. **Límites conocidos:** imports dinámicos; dependencias referenciadas por nombre en configuraciones (plugins de eslint, babel o postcss); imports solo de tipos; CLI usadas en scripts; dependencias por plataforma u opcionales o peer; *features* de Cargo; `build.rs`; *hoisting* en monorepos. Todos los resultados se etiquetan "posible".

---

## 5. Seguridad de dependencias

### 5.1 Alcance

| Fuente de inventario | Cómo se obtiene (sin ejecutar código del proyecto) | Ecosistema en OSV | Viabilidad | Fase |
|---|---|---|---|---|
| Fórmulas de Homebrew | `brew vulns --json` (Homebrew ≥ 7.0). Alternativa: `Cellar/<f>/<v>/sbom.spdx.json` (SPDX-2.3, purl `pkg:brew/homebrew/core/<f>@<v>`, `downloadLocation`) + `INSTALL_RECEIPT.json` [Verificado aquí: 149 de 150 kegs tienen SBOM] | `Homebrew` (base propia de Homebrew) + `GIT` | Alta | MVP |
| Casks de Homebrew | Caskroom + `brew info --json=v2 --cask --installed` | Ninguno | Baja: solo "desactualizado" | v2 |
| npm -g / pnpm -g / bun -g | Lectura directa de `<prefix>/lib/node_modules/*/package.json` | npm | Alta | MVP |
| pipx / herramientas de uv | `site-packages/*.dist-info/METADATA` de cada venv | PyPI | Alta | v1 |
| `cargo install` | `~/.cargo/.crates2.json`; datos de `cargo auditable` dentro de los binarios (`auditable-info`) | crates.io | Alta | MVP (`crates2`) / v1 (auditable) |
| `go install` | Buildinfo de `~/go/bin/*` (parser propio o `go version -m` si Go está instalado) | Go | Alta | v1 |
| Gemas globales | `<gemdir>/specifications/*.gemspec` (nombre-versión en el nombre del fichero) | RubyGems | Media | v1 |
| Lockfiles de proyectos | Parseo (§5.2) | Varios | Alta | MVP: npm, pnpm, yarn, Cargo, uv, poetry, Pipfile, requirements con `==`, Gemfile, go.mod, composer. v1: bun.lock, Podfile.lock, Package.resolved, gradle.lockfile, pubspec.lock |
| Instalado en disco | `node_modules/**/package.json`, `site-packages/*.dist-info` | npm / PyPI | Media (coste de I/O) | v1 |
| Apps Electron | `<App>.app/Contents/Frameworks/Electron Framework.framework/Versions/A/Resources/Info.plist` → `CFBundleVersion` [Verificado aquí: VS Code 43.7.7, Claude 44.9.3] → `npm/electron` | npm | Media: solo avisos publicados por Electron | v1 |
| Apps en general | `Info.plist` (`CFBundleShortVersionString`) + CPE de NVD | — | **Baja** (§5.5) | v2 ("desactualizada", sin CVE) |
| macOS | `sw_vers` + feed SOFA | — | Alta | v1 (coordinar con doc 02) |
| Extensiones de VS Code y derivados | `~/.vscode/extensions/*/package.json` (`publisher.name@version`) | `VSCode` (solo `MAL-*`) | Media: malware conocido | v1 |
| Extensiones de navegador | `~/Library/Application Support/Google/Chrome/*/Extensions/<id>/<v>/manifest.json`, etc. | — (fuentes no investigadas en este análisis) | Desconocida | Won't (v2: investigar) |
| GitHub Actions de proyectos | `.github/workflows/*.yml` → `uses: owner/repo@ref` | `GitHub Actions` | Media | v2 |
| Imágenes Docker | — | — | Fuera de alcance (Trivy o Grype) | Won't |

### 5.2 Formatos de lockfile

| Fichero | Formato | Ecosistema OSV | ¿Distingue directa/transitiva? | Notas y riesgos |
|---|---|---|---|---|
| `package-lock.json` / `npm-shrinkwrap.json` | JSON. `lockfileVersion` 1: árbol `dependencies` anidado. 2 y 3: `packages` indexado por ruta `node_modules/a/node_modules/b` | npm | Sí: `packages[""]` lista las dependencias; flags `dev`, `optional`, `peer` | `resolved` revela el registro (privado o no); `link: true` en workspaces |
| `yarn.lock` (v1) | Formato propio parecido a YAML: `"a@^1, a@^1.2":` + `version "1.2.3"` | npm | No (requiere `package.json`) | Crate `yarn-lock-parser` |
| `yarn.lock` (Berry ≥ 2) | YAML con `__metadata`; `resolution: "a@npm:1.2.3"` | npm | No | Filtrar los protocolos `workspace:`, `patch:`, `portal:`, `link:` |
| `pnpm-lock.yaml` | YAML. `lockfileVersion` `'6.0'` (claves `/a@1.0.0`) o `'9.0'` (`packages` + `snapshots`, claves `a@1.0.0`) | npm | Sí: `importers.<ruta>.{dependencies,devDependencies}` | Sufijos de peers `(react@18…)` en las claves; *catalogs* |
| `bun.lock` | JSONC de texto (por defecto desde Bun 1.2) | npm | Sí (workspaces) | `bun.lockb` es binario heredado: no se parsea; se sugiere migrar al formato de texto [VERIFICAR el flag] |
| `deno.lock` | JSON (`npm:` y `jsr:`) | npm (jsr sin OSV) | Parcial | v2 |
| `Cargo.lock` | TOML, `version = 3/4` | crates.io | Sí: la raíz son los miembros del workspace (sin `source`) | Las dependencias `source = "git+…"` no son de crates.io: GIT o se omiten. Crate `cargo-lock` |
| `poetry.lock` | TOML | PyPI | No: hay que cruzar con `pyproject.toml` [VERIFICAR los grupos en Poetry 2] | Normalizar nombres (PEP 503) |
| `uv.lock` | TOML `[[package]]` | PyPI | Sí: paquete raíz *editable* o *virtual* + `dependencies` | `resolution-markers`: un paquete puede tener varias versiones por plataforma. Se evalúa la plataforma local y se informa del resto como "otras plataformas" |
| `requirements*.txt` | Texto PEP 508 | PyPI | No | Solo `==` es fiable; un rango se marca "no resuelto" (sin falso positivo). Hay que seguir `-r`/`-c`; `-e git+…` → GIT |
| `Pipfile.lock` | JSON (`default`, `develop`) | PyPI | Parcial (`develop` = desarrollo) | — |
| `pdm.lock` | TOML | PyPI | Parcial | v1 |
| `Gemfile.lock` | Texto propio (`GEM`/`specs`, `DEPENDENCIES`, `BUNDLED WITH`) | RubyGems | Sí: `DEPENDENCIES` son las directas | Sufijos de plataforma (`-arm64-darwin`) |
| `go.mod` (+ `go.sum`) | `require` con `// indirect` | Go (y `stdlib` según `go`/`toolchain`) | Sí (comentario `indirect`) | **No usar `go.sum` como inventario**: contiene versiones *consideradas*, no solo las seleccionadas, y genera falsos positivos. Desde Go 1.17, `go.mod` lista todos los módulos necesarios (grafo podado), así que basta como inventario. `go list -m all` es más exacto, pero ejecuta la toolchain y puede descargar |
| `Podfile.lock` | YAML (`PODS`, `DEPENDENCIES`, `SPEC REPOS`, `CHECKOUT OPTIONS`) | No hay ecosistema CocoaPods en OSV. Se podría mapear al repo de la podspec → `SwiftURL`/`GIT` [VERIFICAR viabilidad] | Sí (`DEPENDENCIES`) | Cobertura baja; se etiqueta |
| `Package.resolved` | JSON v1 (`object.pins`), v2/v3 (`pins`: `identity`, `location`, `state.version`/`revision`) | `SwiftURL` (el nombre es la URL del repo) | No (requiere `Package.swift` o `project.pbxproj`) | Ubicaciones: raíz o `*.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/` |
| `composer.lock` | JSON (`packages`, `packages-dev`) | Packagist | Parcial (dev) | — |
| `gradle.lockfile` | Texto `grupo:artefacto:versión=conf1,conf2` | Maven | No | Solo existe si el proyecto activó el *dependency locking*. Sin lockfile, Gradle o Maven requieren resolución, es decir, ejecutar el build (`RNF-16`): v2, o solo las directas declaradas |
| `pom.xml` | XML | Maven | Solo directas | Propiedades `${}` y BOMs; sin transitivas → v2 |
| `pubspec.lock` | YAML (`dependency: direct main` / `direct dev` / `transitive`) | Pub | Sí | — |
| `packages.lock.json` (NuGet) | JSON (`Direct`/`Transitive`) | NuGet | Sí | v2 |
| `mix.lock` | Término de Elixir | Hex | No | v2 |
| `.terraform.lock.hcl`, conda `environment.yml` | — | Sin ecosistema en OSV | — | Won't (mapear conda a PyPI genera falsos positivos) |

### 5.3 Fuentes de datos

| Fuente | Qué aporta | Acceso | Licencia de los datos | Tamaño y frecuencia (medido el 2026-11-01) | Uso propuesto |
|---|---|---|---|---|---|
| **OSV.dev API** | Agrega GHSA, PyPA, RustSec, Go vulndb, malicious-packages, distros… | `POST https://api.osv.dev/v1/querybatch`: devuelve **solo** `id` + `modified`, pagina con `next_page_token` por encima de unos 1.000 resultados por consulta o 3.000 por lote. `GET /v1/vulns/{id}` para el detalle. `POST /v1/query` | Mixta, según la fuente original (GHSA CC-BY-4.0, RustSec CC0…) | Tiempo real | **Principal** en modo online |
| **Volcados de OSV** | Todos los avisos por ecosistema | `https://storage.googleapis.com/osv-vulnerabilities/<ECO>/all.zip`; lista en `…/ecosystems.txt`; incremental `…/<ECO>/modified_id.csv` (`<fecha ISO>,<id>`, orden cronológico inverso) | Igual | Comprimidos: npm 220 MB · GIT 213 MB · PyPI 37,6 MB · Go 12,7 MB · Packagist 11,1 MB · Maven 10,5 MB · RubyGems 5,2 MB · crates.io 3,8 MB · NuGet 2,7 MB · Hex 0,9 MB · SwiftURL 113 KB · GitHub Actions 100 KB · Pub 26 KB · VSCode 23 KB · `all.zip` 2,86 GB. Exportación continua | **Principal** en modo offline |
| GitHub Advisory Database | GHSA, severidad, CVSS, CWE, avisos revisados | Repo `github/advisory-database` (JSON OSV); `GET https://api.github.com/advisories` (60 peticiones/h sin token) | **CC-BY-4.0** [Verificado: LICENSE.md] | Ya incluida en OSV | No duplicar; **atribución obligatoria** |
| RustSec | `RUSTSEC-*`, avisos informativos *unmaintained* y *unsound* | `github.com/rustsec/advisory-db`; crate `rustsec` | CC0-1.0 [VERIFICAR en el LICENSE del repo] | En OSV (crates.io) | Vía OSV; el crate `rustsec` es opcional para los avisos informativos |
| **Homebrew Advisory DB** | `BREW-<fórmula>-<CVE>`; tiene en cuenta los parches backport y las revisiones `_N` | `https://formulae.brew.sh/api/advisories.json`; repo `Homebrew/advisory-database` | **CC0** | 75 MB, 15.979 registros, 590 fórmulas; regeneración diaria. **No** está en OSV (`Homebrew/all.zip` → 404) | Homebrew offline y fallback (§5.4) |
| NVD API 2.0 | CVE, CVSS de NVD, CPE | `https://services.nvd.nist.gov/rest/json/cves/2.0` (límite de unas 5 peticiones/30 s sin clave y 50 con clave) [VERIFICAR] | Dominio público de EE. UU. [VERIFICAR] | — | Solo para enriquecer CVSS si OSV no lo trae; **sin** emparejamiento por CPE en MVP ni v1 |
| **CISA KEV** | CVE explotadas activamente; uso en ransomware | `https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json` | Gobierno de EE. UU. [VERIFICAR términos] | 1,7 MB, 1.766 entradas (catálogo 2026.10.30); varias veces por semana | Prioridad P0 |
| **EPSS** (FIRST) | Probabilidad de explotación a 30 días | `https://api.first.org/data/v1/epss?cve=…`; CSV diario `https://epss.empiricalsecurity.com/epss_scores-current.csv.gz` (el host de cyentia redirige) | Uso libre con atribución [VERIFICAR] | 2,8 MB gz al día | Priorización secundaria |
| **SOFA** (MacAdmins) | Versiones de macOS, CVE corregidas y `ActivelyExploitedCVEs` por versión; XProtect | `https://sofafeed.macadmins.io/v2/macos_data_feed.json` | [VERIFICAR licencia] | 337 KB; diario | Estado de parches del SO. [Verificado aquí: equipo con 27.0.1; la última es 27.2, del 2026-10-26, que corrige 263 CVE] |
| ossf/malicious-packages | Registros `MAL-*` (también de VS Code) | Vía OSV | Apache-2.0 [VERIFICAR] | — | Vía OSV |

### 5.4 Homebrew en detalle

**Cómo funciona `brew vulns`** [Verificado aquí en el código instalado, Homebrew BSD-2-Clause; PR [#23080](https://github.com/Homebrew/brew/pull/23080); publicado con Homebrew 7.0.0 en septiembre de 2026]:

- Deriva la URL del repositorio (GitHub, GitLab, Codeberg, GNOME, freedesktop, KDE) y la *tag* a partir de la URL de origen de la fórmula, o del `sbom.spdx.json` del keg si existe, y consulta OSV con `querybatch` (ecosistema `GIT`).
- Descarta los CVE que un `patch` de la fórmula declara resueltos (`--no-ignore-patches` para verlos).
- Filtra por tipo de corrección (`--fix-type released|patch|any|none|unreleased`) y por severidad (`-s`).
- Ofrece salida JSON con la cobertura del escaneo (`-j`) y la lista de fórmulas omitidas (`--list-skipped`).
- Omite las fórmulas de *taps* no confiables y **no escanea casks**.
- Termina con código de error si hay hallazgos abiertos.
- El módulo `vulns/` suma unas 4.300 líneas de Ruby (`match.rb` sola, 1.251), con fuentes adicionales: Repology, CPANSA y la base `BREW-*`.

**Estrategia:**

1. **MVP.** Si `brew --version` ≥ 7.0 y el usuario aceptó el tráfico de red, se ejecuta `HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1 brew vulns --json` y se mapea el resultado al modelo de §5.7 [VERIFICAR el esquema JSON capturando un *fixture* real]. Brew es quien contacta con OSV: hay que declararlo en Ajustes › Privacidad (`RNF-19`).
2. **Offline o fallback.** Se descarga `advisories.json` y se compara la versión instalada (nombre del directorio en Cellar, `versionInfo` del SBOM y revisión `_N`) con los rangos `ECOSYSTEM`, usando el orden de versiones de Homebrew (`1.81.6_5` < `1.81.6_6` < `1.82.0`). La cobertura es parcial (590 fórmulas) y se etiqueta como tal.
3. **Homebrew < 7.** Se sugiere `brew update`. Reimplementar el mapeo origen → repo + tag de Homebrew es Could (v2) y se desaconseja.
4. **Señales de mantenimiento.** Las fórmulas `deprecated` o `disabled` se muestran como "sin mantenimiento" aunque no tengan CVE.
5. **Recursos embebidos.** Homebrew 7 añade identificadores de los paquetes originales a sus SBOM (PR #23459). Esto permitiría consultar los recursos PyPI, npm o Cargo que una fórmula incluye [VERIFICAR el contenido real del SBOM] (v2).

### 5.5 Apps, extensiones y SO: viabilidad

| Objetivo | Método | Veredicto |
|---|---|---|
| **Apps Electron** | Versión de Electron en el `Info.plist` del framework → `POST /v1/query {"package":{"ecosystem":"npm","name":"electron"},"version":…}`. [Verificado aquí: la 30.0.0 devuelve 47 avisos GHSA] | **Viable (v1)**, con la etiqueta "indicativo": los GHSA de Electron no cubren cada CVE de Chromium. Es útil porque muchas apps llevan un Chromium antiguo embebido |
| Otros frameworks embebidos | `Sparkle.framework` (1 app aquí), Chromium Embedded Framework: versión del `Info.plist` → `SwiftURL`/GHSA [VERIFICAR la presencia de Sparkle en OSV] | Could (v2) |
| Apps en general → CVE | Emparejamiento por CPE de NVD | **No viable** de forma fiable: nombres de fabricante y producto ambiguos, formatos de versión distintos, retrasos de enriquecimiento en NVD [VERIFICAR el estado actual del backlog]. Alternativa (v2, docs 02 y desinstalador): "app desactualizada" según la versión de su cask o el appcast de Sparkle (`SUFeedURL`), sin afirmar CVE |
| **macOS** | `sw_vers` frente a SOFA v2 | **Viable (v1)**: "macOS desactualizado: la versión 27.2 corrige N CVE, M explotadas activamente". Hay que coordinarlo con el doc 02 para no duplicar avisos |
| **Extensiones de VS Code** | `publisher.name@version` → ecosistema OSV `VSCode` [Verificado aquí: volcado de 22,6 KB, solo `MAL-*`] | **Viable (v1)** como "extensión maliciosa conocida", no como cobertura de CVE. [VERIFICAR si los IDs de Open VSX (Cursor, Windsurf) coinciden con los del Marketplace] |
| Extensiones de navegador | No se han investigado fuentes de vulnerabilidades o malware en este análisis (pendiente). Consultar el estado en las tiendas requeriría una petición por extensión, lo que afecta a la privacidad | **Won't** en MVP y v1; investigar en v2 |

### 5.6 Modos online y offline, y privacidad

**Principio (doc 02, `RNF-17/19`).** El módulo viene **desactivado** y la red está en cero por defecto. Al activarlo se elige el modo y se muestra la lista exacta de destinos.

| Modo | Qué sale del equipo | Qué no sale | Requisitos |
|---|---|---|---|
| **Online** (por defecto al activar) | Por cada `paquete@versión` **único**: ecosistema, nombre y versión (o purl). La IP es visible para Google (OSV), Homebrew, CISA y FIRST | Rutas, nombres de proyecto, remotos git, usuario, secretos, hallazgos de higiene | Consentimiento; previsualización de la carga ("se enviarán 412 paquetes de 6 ecosistemas") |
| **Offline** | Solo la descarga de volcados (IP + qué ecosistemas) | Nada del inventario | Espacio local; comparadores de versión propios |
| **Híbrido** | Emparejamiento offline + consulta online solo para ecosistemas no descargados (opt-in por ecosistema) | Igual que online, restringido | — |

**Minimización y exclusiones (online):**

- Se deduplica entre proyectos (una consulta por paquete y versión únicos).
- **Se excluyen los paquetes privados**:
  - npm: *scopes* con registro propio en `.npmrc` o `resolved` fuera del registro público;
  - pip: índices propios;
  - Cargo: `source` distinto de crates.io;
  - Go: módulos que casan con `GOPRIVATE`/`GONOSUMDB`;
  - Composer: repositorios privados;
  - patrones configurables por el usuario.
- Un nombre interno como `@acme/billing-core` revelaría información de la empresa.
- Los detalles de cada aviso se cachean por `id` + `modified`.

**Base local (offline):**

- Solo se descargan los ecosistemas presentes en el equipo, más `advisories.json`, KEV y EPSS.
- **Tamaño descomprimido: no medido.** Estimación: JSON con compresión de 5 a 10×, es decir, npm podría ocupar de 1 a 2 GB descomprimido [VERIFICAR descargando y midiendo].
- Se almacena como índice compacto: SQLite con `(ecosistema, nombre normalizado)` → rangos, y el detalle comprimido. Objetivo: < 500 MB para un perfil típico (npm + PyPI + crates.io + Go + RubyGems + Homebrew).
- **Actualización** diaria (configurable) e incremental: se lee `modified_id.csv` hasta la última marca procesada y se descargan los registros cambiados (`…/<ECO>/<ID>.json`) [VERIFICAR el patrón HTTP de los registros individuales]. Refresco completo semanal o si hay un hueco.
- Se muestra siempre "Datos de: <fecha>". Si tienen más de 7 días, se avisa (RF-VUL-27).
- **Comparadores de versión locales** necesarios: npm semver, PEP 440, semver de Cargo, Go (con `+incompatible` y pseudo-versiones), RubyGems, Maven (`ComparableVersion`), Composer, Pub, Hex, Homebrew `_N` y SwiftURL. El riesgo de errores se mitiga con casos de prueba derivados de OSV [VERIFICAR si osv.dev publica vectores de prueba por ecosistema].
- **Redistribución:** en el MVP no se ofrece un espejo propio ni una base preconstruida dentro de la app. El cliente descarga directamente del bucket de OSV. Distribuir una base exigiría revisar las licencias de cada fuente (GHSA CC-BY-4.0 requiere atribución) → pregunta abierta.

**Relación con la IA (doc 04, RF-IA-36).** Solo se envían a un proveedor de IA hallazgos que el usuario elija explícitamente, con la misma minimización: paquete, versión, ID y resumen; nunca rutas ni secretos.

### 5.7 Modelo de hallazgo

| Campo | Contenido |
|---|---|
| `finding_id` | Hash estable de (ubicación, paquete, versión, vuln_id) |
| `vuln_id`, `aliases[]` | ID principal (el de OSV) y alias: `CVE-*`, `GHSA-*`, `RUSTSEC-*`, `PYSEC-*`, `GO-*`, `BREW-*`, `MAL-*` |
| `summary`, `details` | Del aviso (Markdown, saneado) |
| `severity` | Versión de CVSS (3.1/4.0), vector y puntuación base calculada (crate `cvss`); `source_severity` (p. ej. `database_specific.severity` de GHSA: LOW/MODERATE/HIGH/CRITICAL); "desconocida" si no hay ninguna |
| `kev` | `in_kev`, `date_added`, `knownRansomwareCampaignUse` |
| `epss` | Puntuación, percentil y fecha |
| `package` | Ecosistema, nombre, purl (crate `packageurl`) |
| `installed_version`, `affected_ranges` | — |
| `fixed_versions`, `fix_type` | Versión corregida más baja ≥ la instalada en el mismo rango; `released` / `patch` (commit) / `none` |
| `suggested_command` | Según el ecosistema y la ubicación (tabla siguiente); **solo para copiar** en MVP y v1 |
| `dependency` | `direct` / `transitive` / `unknown`; ruta raíz → … → paquete; ámbito `prod` / `dev` / `optional` |
| `location` | Tipo: `project` / `homebrew` / `global` / `binary` / `app` / `extension` / `os`; ruta del proyecto (solo local); fichero de manifiesto |
| `published`, `modified`, `withdrawn` | Los avisos `withdrawn` se descartan |
| `references` | Aviso, commit de la corrección |
| `status` | `open` / `suppressed` (motivo, caducidad) / `resolved` (desaparece al re-escanear) |
| `first_seen`, `last_seen`, `data_source`, `data_timestamp` | Trazabilidad |

**Deduplicación.** Se agrupan por alias (*union-find*): el mismo `paquete@versión` con IDs equivalentes es un solo hallazgo. La severidad se toma de la fuente con vector CVSS. Si hay varias, gana la más alta, y la UI lo explica.

**Prioridad determinista** (la "puntuación del doc 03" que usa el doc 04, F5):

| Prioridad | Regla |
|---|---|
| **P0 · Actuar ya** | Paquete malicioso (`MAL-*`) instalado o declarado, **o** CVE en KEV |
| **P1** | CVSS ≥ 9,0, **o** CVSS ≥ 7,0 con percentil EPSS ≥ 0,95 |
| **P2** | CVSS 7,0–8,9 |
| **P3** | El resto, o severidad desconocida |

**Modificadores** (siempre visibles en "¿Por qué esta prioridad?"):

- Dependencia solo de desarrollo: baja un nivel (nunca para `MAL-*` ni KEV).
- Proyecto abandonado (> 365 días): baja un nivel y sugiere limpiar sus artefactos.
- Sin corrección disponible: no cambia la prioridad, pero se marca.

**Comandos sugeridos** (copiar; ejecutarlos con confirmación y árbol git limpio sería Could en v2):

| Ubicación | Comando |
|---|---|
| npm directa / transitiva | `npm install <p>@<fixed>` / `npm update <p>` u `overrides` en `package.json` |
| pnpm | `pnpm update <p>` / `pnpm.overrides` |
| Yarn | `yarn up <p>` (Berry) / `resolutions` |
| Bun | `bun update <p>` |
| Cargo | `cargo update -p <crate> --precise <v>` (transitiva) / editar `Cargo.toml` (salto de versión mayor en una directa) |
| uv / Poetry / Pipenv / pip | `uv lock --upgrade-package <p>` / `poetry update <p>` / `pipenv update <p>` / `pip install '<p>>=<fixed>'` y actualizar `requirements` |
| Bundler | `bundle update --conservative <gema>` |
| Go | `go get <mod>@v<fixed> && go mod tidy`; `stdlib`: actualizar la directiva `toolchain` o Go |
| Composer | `composer update <vendor/p> --with-dependencies` |
| CocoaPods / SwiftPM | `pod update <Pod>` / `swift package update <Pkg>` (o el equivalente en Xcode) |
| Homebrew | `brew upgrade <f>` |
| Globales | `npm install -g <p>@latest` · `cargo install <crate> --locked --force` · `pipx upgrade <p>` · `uv tool upgrade <p>` · `gem update <gema>` |
| App Electron / macOS | Actualizar la app / Actualización de software |

### 5.8 Falsos positivos y supresiones

**Fuentes de falsos positivos y su tratamiento:**

- Rangos erróneos en los avisos.
- Backports: Homebrew los resuelve `brew vulns` o la base `BREW-*`.
- `go.sum` como inventario: no se usa.
- Rangos sin resolver en `requirements`: se marcan "no resuelto".
- Paquetes de otra plataforma en el lockfile (`@esbuild/linux-x64`, marcadores de uv): se marcan "otra plataforma" con prioridad mínima.
- `bundleDependencies` de npm.
- Avisos `withdrawn`: se descartan.
- Función vulnerable no usada: la alcanzabilidad es desconocida y se indica como tal.

**Modelo de supresión** (RF-VUL-14/15):

| Campo | Valores |
|---|---|
| Ámbito | Este hallazgo en este proyecto · esta vulnerabilidad en todas partes · este paquete en todas partes |
| Motivo (obligatorio) | `no_afecta`, `no_alcanzable`, `riesgo_aceptado`, `falso_positivo`, `mitigado`, `pendiente_upstream` + texto libre |
| Caducidad (obligatoria) | Por defecto 90 días, máximo 365. Al caducar, el hallazgo reaparece con la etiqueta "supresión caducada" |
| Metadatos | Fecha de creación, usuario local |
| Almacenamiento | Configuración local de la app (`RNF-18`); exportable e importable en TOML |
| Importación de proyecto | `osv-scanner.toml` (`[[IgnoredVulns]]` con `id`, `ignoreUntil`, `reason`) y `.cargo/audit.toml` (`[advisories] ignore = [...]`, sin caducidad: se le asigna la de por defecto y se marca "importada") [VERIFICAR los formatos actuales] |
| Exportación VEX | OpenVEX / CycloneDX VEX (v2) |

### 5.9 Exportación: informe y SBOM

**Informe:**

- **JSON** con esquema versionado (MVP).
- **Markdown/HTML** para personas y **CSV** para hojas de cálculo (v1).
- **SARIF 2.1.0** para los hallazgos de proyecto, útil para GitHub code scanning (v1, crate `serde-sarif`).
- Opción "anonimizar rutas" al exportar.

**SBOM** (RF-VUL-17):

- **CycloneDX JSON** (crate `cyclonedx-bom`, Apache-2.0 [VERIFICAR soporte de la especificación 1.6]), con `vulnerabilities[]` opcional.
- **SPDX 2.3 JSON**: escritor propio con structs serde. `spdx-rs` no se actualiza desde 2023; el crate `spdx` solo sirve para expresiones de licencia.
- Cada componente lleva su **purl**.
- Alcances:
  - **por proyecto**, desde el lockfile con su grafo;
  - **"SBOM del equipo"**: kegs de Homebrew (se reutiliza su `sbom.spdx.json`), globales, binarios auditables, apps Electron y extensiones. Posible diferenciador [VERIFICAR con el análisis competitivo del doc 01].

### 5.10 Reutilización: crates y herramientas

| Necesidad | Opción | Licencia | Estado (crates.io, 2026-11-01) | Recomendación |
|---|---|---|---|---|
| Parsear `Cargo.lock` | `cargo-lock` | Apache-2.0 OR MIT | v11.1.0, muy usado | **Reutilizar** |
| Base de RustSec | `rustsec` | Apache-2.0 OR MIT | v0.33.0 | Opcional: OSV ya incluye RustSec; útil para *unmaintained*/*unsound* |
| CVSS | `cvss` | Apache-2.0 OR MIT | v3.0.0 | **Reutilizar** [VERIFICAR soporte de CVSS 4.0] |
| Esquema y cliente OSV | `osv` (gcmurphy) | Apache-2.0 | v0.3.0, unas 72 000 descargas, mantenimiento modesto | **Implementar** structs serde propios del esquema OSV 1.7 (pequeño y estable), usando `osv` como referencia. `osv-db` (MIT/Apache) casi no tiene uso: descartado |
| purl | `packageurl` | MIT | v0.7.1 | **Reutilizar** |
| Versiones | `semver` (Cargo), `nodejs-semver` (npm, Apache-2.0), `pep440_rs` (Apache-2.0/BSD-2; última versión 2024-12; alternativa `uv-pep440`, crate interno de uv en 0.0.x, inestable) | — | — | **Reutilizar** con versión fijada. **Implementar** RubyGems, Maven, Go (pseudo-versiones), Composer y Homebrew `_N`. `versions` (MIT) solo como respaldo genérico, no exacto. No se encontró un crate verificado para el orden de Maven |
| yarn.lock v1 | `yarn-lock-parser` | MIT | v0.14.0 | **Reutilizar**; Berry y pnpm vía YAML |
| YAML | — | — | `serde_yaml` está archivado | Elegir un parser mantenido [VERIFICAR alternativas: `saphyr`, `serde_norway`, `yaml-rust2`] |
| SBOM | `cyclonedx-bom` | Apache-2.0 | v0.8.1 | **Reutilizar**; SPDX: implementar |
| SARIF | `serde-sarif` | MIT | v0.8.0 | Reutilizar (v1) |
| Metadatos de binarios | `auditable-info` (Rust) | MIT OR Apache-2.0 | v0.10.1 | Reutilizar. Buildinfo de Go: implementar (formato documentado en `debug/buildinfo`) |
| Git | `gix` | MIT OR Apache-2.0 | v0.90.0 | **Reutilizar** (solo lectura; no ejecuta comandos). `git2` enlaza libgit2 en C: descartado |
| Recorrido | `ignore` (Unlicense/MIT) + `getattrlistbulk` (doc 05) | — | `jwalk` se declara obsoleto en favor de `dua-core` | — |
| Limpiar `target/` | `cargo-sweep` (referencia) | MIT | v0.8.0 | Inspirar la heurística, no depender |
| Escáner externo | `osv-scanner` (Go) | Apache-2.0 | Activo; modo offline con `{dir}/osv-scanner/{eco}/all.zip` | **No empaquetar** en el MVP (segunda toolchain, binario grande, otro ciclo de actualización). v2: "motor externo" opcional si el usuario ya lo tiene, para contrastar. Lo mismo para Trivy, Grype y Syft (Apache-2.0) |
| Homebrew | `brew vulns` | BSD-2-Clause (vía CLI) | Integrado en Homebrew 7 | **Delegar** (§5.4) |
| Secretos (§6) | gitleaks (Go, MIT, activo), ripsecrets (Rust, MIT), noseyparker (Rust, Apache-2.0, **archivado en febrero de 2026**), trufflehog (**AGPL-3.0**) | — | — | Reglas propias de alta precisión; las de gitleaks y noseyparker pueden inspirar o reutilizarse **con atribución**. **No** usar trufflehog (AGPL) |

Las dependencias Apache-2.0 son compatibles con distribuir bajo MIT, pero exigen conservar NOTICE y atribución. Los datos CC-BY-4.0 (GHSA) exigen atribución visible (RF-VUL-25).

---

## 6. Higiene de seguridad del desarrollador (opcional)

**Principios** (RF-VUL-32):

- Todo es local.
- Se lee lo mínimo: cabeceras y nombres de campo, nunca valores completos.
- Los valores se **enmascaran** en la UI (`ghp_****…a1F3`) y nunca se registran en logs.
- Para deduplicar se guarda un HMAC con una clave local aleatoria, nunca el secreto ni su hash plano.
- Nada sale del equipo ni va a la IA (doc 04).
- **Nunca** se validan secretos contra la API de su proveedor, como hace trufflehog: eso transmitiría el secreto.
- Nunca se borra ni se modifica nada automáticamente: se guía al usuario.

| # | Comprobación | Cómo | Riesgo de falso positivo | Prioridad / fase |
|---|---|---|---|---|
| H1 | **Claves SSH privadas sin passphrase** | `~/.ssh/*` + `IdentityFile` de `~/.ssh/config`. Se determina por la cabecera, sin cargar la clave. OpenSSH: `BEGIN OPENSSH PRIVATE KEY` → base64 → *magic* `openssh-key-v1\0` y la primera cadena (`ciphername`); `none` = sin cifrar. PEM antiguo: `BEGIN RSA/EC/DSA PRIVATE KEY` sin `Proc-Type: 4,ENCRYPTED` = sin cifrar. PKCS#8: `BEGIN PRIVATE KEY` (sin cifrar) frente a `BEGIN ENCRYPTED PRIVATE KEY`. Las claves de hardware (`sk-…@openssh.com`) se consideran correctas. Alternativa: `ssh-keygen -y -P "" -f <clave>` (si funciona, no tiene passphrase; carga la clave en memoria, por eso es la segunda opción) | Bajo | Should, v1. Arreglo guiado: `ssh-keygen -p -f <clave>` + `ssh-add --apple-use-keychain` |
| H2 | **Permisos laxos** | `~/.ssh` ≠ 700; claves privadas ≠ 600; ficheros de credenciales legibles por grupo u otros | Bajo | Should, v1 (sugerir `chmod`) |
| H3 | **Algoritmos débiles** | DSA, RSA < 2048 bits (`ssh-keygen -l -f <clave>.pub`) | Bajo | Could, v1 |
| H4 | **Credenciales en texto plano en rutas conocidas** | Se detecta solo la **presencia del campo**: `~/.git-credentials` (`credential.helper store`; recomendar `osxkeychain`), `~/.docker/config.json` (`auths.*.auth` sin `credsStore`), `~/.npmrc` (`_authToken`), `~/.yarnrc.yml` (`npmAuthToken`), `~/.pypirc` (`password`), `~/.netrc`, `~/.aws/credentials` (claves `AKIA…` de larga duración; recomendar SSO o aws-vault), `~/.config/gh/hosts.yml` (`oauth_token` fuera del llavero), `~/.cargo/credentials.toml`, `~/.gem/credentials`, `~/.terraform.d/credentials.tfrc.json`, `~/.kube/config` (`token`, `client-key-data`), `~/.config/gcloud/application_default_credentials.json`, `~/.m2/settings.xml` (`<password>` sin cifrar), `~/.cache/huggingface/token` [Verificado aquí: existe] | Medio: muchas herramientas lo hacen por diseño; se presenta como "mejorable", no como "comprometido" | Should, v1 |
| H5 | **Lista blanca de claves gestionadas por apps** | `~/.ollama/id_ed25519` [Verificado aquí], certificados de Docker Desktop, claves de identidad de otras apps | — | Must si H1/H4 existen (evitar falsos positivos) |
| H6 | **Secretos versionados en proyectos** | `.env*` (salvo `.env.example`, `.sample`, `.template`) o claves privadas en `git ls-files`; `.env` no ignorado | Bajo | Should, v1. **Sin** escanear el historial git (`git log -p`): coste alto y es el terreno de gitleaks (Won't) |
| H7 | **Secretos sueltos en Descargas y Escritorio** | Primero por nombre: `*.pem`, `*.p12`, `AuthKey_*.p8`, `*.mobileprovision`, `*service-account*.json`, `*accessKeys*.csv`, `id_rsa*`. Después, contenido en ficheros de texto pequeños con reglas de alta precisión: cabeceras PEM, `AKIA[0-9A-Z]{16}`, `ghp_`/`github_pat_`, `glpat-`, `xox[baprs]-`, `npm_`, prefijos de claves de proveedores de IA [VERIFICAR los prefijos vigentes] | Medio | Could, v2, **opt-in** (TCC) |
| H8 | **Historial de shell** | `~/.zsh_history`, `~/.zsh_sessions/*.history`, bash, fish: líneas con `export *_TOKEN=`, `export *_KEY=`, `export *_SECRET=`, `Authorization: Bearer`, `mysql -p<pass>`, claves `AKIA…` | Medio | Could, v2, **opt-in**. Muestra el número de línea y el valor enmascarado; la edición la hace el usuario |
| H9 | Llavero, GPG, firma de commits | — | — | Won't (fuera de alcance; el llavero es zona protegida) |

**Prioridad:** v1 incluye H1, H2, H4, H5 y H6 (precisión alta, coste bajo, sin TCC). v2 incluye H3 si no entró en v1, H7 y H8 (opt-in y TCC).

---

## 7. Requisitos

**Formato:** ID · descripción · prioridad MoSCoW · fase · criterio de aceptación verificable.

**Dependencias de todos los RF-DEV con acción destructiva:**

- `RF-SEG-01` (previsualización), `RF-SEG-02/03` (Papelera; borrado permanente solo explícito), `RF-SEG-05/06/07` (raíces, rutas, anti-TOCTOU), `RF-SEG-08` (en uso), `RF-SEG-12` (tamaños), `RF-SEG-16` (dry-run idéntico).
- Ninguno de estos requisitos los redefine: añade protecciones (doc 02 §8).

### 7.1 Zona desarrollador (RF-DEV)

| ID | Descripción | Prioridad | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-DEV-01 | Descubrir proyectos en las raíces configuradas mediante los marcadores de §3.2, incluido `.git` como fichero; profundidad por defecto 8 (configurable de 1 a 20); sin seguir symlinks ni cruzar volúmenes | Must | MVP | En un *fixture* con proyectos a profundidades 1–10 bajo `~/Developer`: con profundidad 8 se detectan los de ≤ 8 y ninguno de 9–10. Un symlink a otro árbol no se recorre. Un worktree con `.git` fichero se reconoce como proyecto |
| RF-DEV-02 | Raíces por defecto sin TCC (§3.1). Documentos, Escritorio, Descargas y volúmenes externos solo con opt-in y explicación previa; iCloud y File Provider excluidos | Must | MVP | En un usuario de prueba con `tccutil reset All <bundle-id>`, el primer escaneo con ajustes por defecto no provoca ningún diálogo TCC (`log stream --predicate 'subsystem == "com.apple.TCC"'` sin solicitudes). Activar `~/Documents` muestra la explicación antes del aviso del sistema |
| RF-DEV-03 | Proponer raíces candidatas deducidas de proyectos recientes de IDE (VS Code y derivados, JetBrains, DerivedData, mise) sin escanearlas hasta que el usuario las acepte | Should | v1 | Con un `workspace.json` que apunta a `/Users/x/clientes/foo`, la app propone `/Users/x/clientes`. Antes de aceptarla no hay ningún acceso a esa ruta (`fs_usage` filtrado) |
| RF-DEV-04 | Catálogo de artefactos con validación por marcador hermano (§3.2) y regla genérica `CACHEDIR.TAG` | Must | MVP | *Fixture*: `build/` sin marcador → no se propone; `target/` con `Cargo.toml` + `CACHEDIR.TAG` → se propone como Seguro; un directorio desconocido con `CACHEDIR.TAG` válido dentro de un proyecto → se propone como Seguro con el motivo "marca de caché estándar" |
| RF-DEV-05 | Vista agrupada por proyecto: ruta, ecosistemas, última actividad, distintivos git, tamaño recuperable, artefactos con nivel y motivo | Must | MVP | Cada fila contiene todos los campos; se puede ordenar por tamaño y actividad y filtrar por ecosistema y estado. Cada artefacto muestra "¿Por qué?" con las señales usadas |
| RF-DEV-06 | Calcular la última actividad (§3.4), asignar estados (Activo/Reciente/Inactivo/Dormido/Abandonado) y preseleccionar solo artefactos Seguro de proyectos Inactivos o más antiguos | Must | MVP | *Fixture*: commit hace 3 días → Activo, nada preseleccionado. Último commit hace 200 días sin ediciones → Dormido, con `node_modules` (con lockfile) preseleccionado. Último commit hace 400 días pero ficheros editados ayer → Activo |
| RF-DEV-07 | Protección P1: no ofrecer artefactos con ficheros versionados | Must | MVP | Un `dist/` con 1 fichero versionado no se ofrece y se informa "contiene 1 fichero versionado" |
| RF-DEV-08 | Protecciones P6/P7: repos anidados, worktrees y submódulos | Must | MVP | Un repo con un worktree hermano y un submódulo da 2 proyectos (principal + worktree) y el submódulo queda dentro del principal. Un `node_modules` con un `.git` dentro queda bloqueado. Ningún directorio de worktree aparece como borrable |
| RF-DEV-09 | Protección P8: bloquear artefactos que contienen secretos, claves, keystores o `*.tfstate*` | Must | MVP | Un `target/` con `.env` o `release.keystore` dentro queda bloqueado, con un aviso que nombra el fichero (sin mostrar su contenido) |
| RF-DEV-10 | Distintivos git P3–P5: cambios sin commitear, commits sin push o ramas sin upstream, stashes | Should | MVP | Una rama local con 2 commits por delante muestra "2 commits sin push"; un stash muestra su distintivo. Los artefactos ignorados no se bloquean; las acciones sobre el proyecto entero sí |
| RF-DEV-11 | Protección P9: omitir proyectos con procesos activos (cwd) o bloqueos de herramienta | Should | v1 | Con `npm run dev` en marcha en el proyecto X, sus artefactos no se proponen y se indica "en uso por node (pid N)" |
| RF-DEV-12 | Protección P10: regenerabilidad (sin lockfile o registro privado → Precaución con el host) | Must | MVP | `node_modules` sin lockfile → Precaución, sin preseleccionar. `package-lock.json` con `resolved` en `npm.acme.internal` → Precaución con el aviso "requiere acceso a npm.acme.internal" |
| RF-DEV-13 | Leer git sin ejecutar código del repo (`gix`, o CLI neutralizado) y sin disparar el instalador de las Command Line Tools | Must | MVP | Repo con `core.fsmonitor` apuntando a un script que crea un fichero centinela: tras escanear, el centinela no existe. En un sistema sin CLT no aparece el diálogo de instalación |
| RF-DEV-14 | Inventario de cachés globales (§2) con presencia, tamaño y nivel. MVP: Homebrew, npm, pnpm, Yarn, Bun, pip, uv, Cargo, Go, Gradle, CocoaPods, SwiftPM, DerivedData y cachés de CoreSimulator. v1: el resto de §2 | Must | MVP (subconjunto) / v1 | Para cada elemento del subconjunto en un *fixture*, el tamaño mostrado difiere menos del 5 % del de `du -sk` (bloques asignados), contando los hardlinks una vez |
| RF-DEV-15 | Usar el comando nativo cuando exista: plan con el comando literal y su salida de dry-run o listado; tras ejecutar, espacio liberado medido frente a estimado; sin la herramienta o con el daemon parado, no se tocan sus ficheros | Must | MVP | El plan de Homebrew contiene literalmente la salida de `brew cleanup -n` y `brew autoremove -n`. Con Docker parado no hay accesos a `~/Library/Containers/com.docker.docker` (`fs_usage`). Al terminar se muestran "Estimado X / Liberado Y" |
| RF-DEV-16 | Homebrew: `autoremove` (Precaución, con opción "Conservar" → `brew tab --installed-on-request <f>`) y `cleanup` (Seguro) | Must | MVP | Elegir "Conservar" sobre una fórmula huérfana ejecuta `brew tab --installed-on-request <f>`, y `brew autoremove -n` deja de listarla |
| RF-DEV-17 | Homebrew avanzado: casks fantasma, taps sin uso, fórmulas obsoletas o deshabilitadas, desviación del Brewfile | Should | v1 | Un cask cuya app se borró a mano aparece como "cask fantasma". Un tap sin fórmulas instaladas aparece con `brew untap` como acción. Una fórmula `deprecated` muestra el motivo |
| RF-DEV-18 | Toolchains no referenciadas (rustup, nvm/fnm/Volta, pyenv, Pythons de uv, rbenv/rvm, asdf/mise, distribuciones del wrapper de Gradle, JDK, FVM, tfenv, NDK) cruzadas con los ficheros de §2.15, con la lista "Referenciado por" | Should | v1 | *Fixture* rustup con `1.93.0`, `1.95.0` y `stable` (por defecto), y un proyecto con `rust-toolchain.toml` `channel = "1.95.0"` → se propone `1.93.0` (Precaución, E2); `1.95.0` y `stable` quedan protegidas; la ficha de `1.95.0` muestra "Referenciado por: proyecto A" |
| RF-DEV-19 | Proteger los valores globales por defecto e interpretar rangos (§2.15) | Must | v1 (con RF-DEV-18) | El alias `default` de nvm es 20 → ninguna 20.x se propone. Con `engines.node: ">=18 <21"` e instaladas 18.20.0 y 20.11.0 → 20.11.0 protegida y 18.20.0 propuesta con el aviso "satisface un rango" |
| RF-DEV-20 | Xcode. MVP: DerivedData y `xcrun simctl delete unavailable`. v1: DeviceSupport antiguos (se conserva la más reciente por plataforma), runtimes con `--notUsedSinceDays`, Previews, XCTestDevices. Archives siempre Revisar | Must | MVP / v1 | El plan de runtimes reproduce la salida de `xcrun simctl runtime delete --notUsedSinceDays 90 --dry-run`. Los Archives nunca aparecen preseleccionados ni en "Limpiar todo". DerivedData no se toca con `xcodebuild` en ejecución |
| RF-DEV-21 | Contenedores (Docker Desktop, OrbStack, Colima, Podman) solo vía CLI y por contexto; imágenes dangling y build cache como Seguro; volúmenes y contenedores parados como Revisar, nunca en lote; explicación de cómo recuperar el espacio en macOS | Should | v1 | Con Docker activo, la vista desglosa `docker system df -v`. "Limpiar todo" no incluye volúmenes. Con Docker parado se muestra "Inicia Docker" y no hay accesos a su contenedor |
| RF-DEV-22 | IDE: `workspaceStorage` huérfanos, `CachedData` de commits antiguos, versiones obsoletas de extensiones (familia VS Code); directorios de versiones antiguas de JetBrains y Android Studio | Should | v1 | Una entrada de `workspaceStorage` cuyo `folder` no existe → se propone. `CachedData/<commit>` distinto del de `product.json` → se propone. `User/History` nunca se toca. Con la app abierta, sus elementos se omiten (`RF-SEG-08`) |
| RF-DEV-23 | Modelos de IA (Hugging Face, Ollama, LM Studio): lista con tamaño y fecha; borrado con CLI nativo; nunca se preseleccionan; credenciales protegidas | Should | v1 | Borrar un modelo de Ollama ejecuta `ollama rm` y ningún blob se borra directamente. `~/.cache/huggingface/token` y `~/.ollama/id_ed25519` nunca son seleccionables |
| RF-DEV-24 | Dependencias sin uso dentro de un proyecto, solo informativo: herramientas pasivas ya instaladas o comando sugerido; nunca modifica manifiestos (§4) | Could | v2 | Con cargo-machete instalado, en un crate con una dependencia sin uso el informe la lista como "posible". Los manifiestos quedan idénticos (mismo hash antes y después). Sin herramienta, se muestra el comando sugerido |
| RF-DEV-25 | Escaneo incremental con índice persistente y FSEvents | Should | v1 | Un segundo escaneo sin cambios tarda ≤ 20 % del primero. Tras crear un `node_modules` en un proyecto, el escaneo incremental lo detecta |
| RF-DEV-26 | Rendimiento y cancelación del escaneo de proyectos (§3.3) | Must | MVP | En el *benchmark* (50 proyectos, 1 M de entradas, M1 con SSD): primer escaneo < 60 s en frío; la UI responde en < 100 ms durante el escaneo; cancelar en < 500 ms; memoria < 300 MB |
| RF-DEV-27 | Configuración: umbrales de actividad, raíces, profundidad, exclusiones y reglas locales declarativas (TOML) validadas contra `RF-SEG-04` | Should | v1 | Una regla de usuario `dir=".cache-foo", requires="foo.toml", level="Seguro"` se aplica. Una regla que apunta a una ruta de `RF-SEG-04` se rechaza con un mensaje |
| RF-DEV-28 | Señal opcional de uso por historial de shell (local, opt-in) para elementos E4 | Could | v2 | Desactivada por defecto. Activada, una fórmula *leaf* invocada en el historial en los últimos 30 días aparece como "usada recientemente". El historial no se copia, no se guarda en el índice y no se transmite |
| RF-DEV-29 | Vista de proyectos abandonados (> 365 días) con "Limpiar todos los artefactos"; nunca se ofrece borrar el proyecto | Could | v1 | Un proyecto con 400 días de inactividad aparece en la vista con su tamaño total; la acción solo incluye artefactos Seguro/Precaución y ningún fichero versionado |
| RF-DEV-30 | CLI equivalente (subcomandos `dev` del binario común) con salida JSON versionada | Should | v1 | `<cli> dev scan --json` y `<cli> dev clean --dry-run --json` producen el mismo plan que la GUI sobre el mismo *fixture* (`RF-SEG-16`) |
| RF-DEV-31 | Entornos huérfanos o rotos: venvs con intérprete inexistente, venvs de pipenv/Poetry de proyectos inexistentes, entornos de conda de `environments.txt` inexistentes, output bases de Bazel huérfanos | Should | v1 | Un venv cuyo `pyvenv.cfg` → `home` no existe aparece como "roto (E1)". Un entorno de pipenv cuyo `.project` apunta a una ruta inexistente aparece como huérfano |
| RF-DEV-32 | Android: system images sin AVD, NDK y platforms no referenciados (vía `sdkmanager --uninstall`); AVD listados como Revisar (`avdmanager delete avd`) | Should | v1 | Una system image que no usa ningún `config.ini` se propone como E1. Un AVD nunca aparece preseleccionado |

### 7.2 Seguridad de dependencias (RF-VUL)

| ID | Descripción | Prioridad | Fase | Criterio de aceptación |
|---|---|---|---|---|
| RF-VUL-01 | Inventario MVP sin ejecutar código del proyecto: lockfiles de npm, pnpm, Yarn v1/Berry, Cargo, uv, Poetry, Pipfile, requirements (`==`), Gemfile, go.mod y Composer; globales de npm, pnpm y Bun; `~/.cargo/.crates2.json`; fórmulas de Homebrew | Must | MVP | Para cada formato del *fixture*, el número de pares `paquete@versión` extraídos coincide con la referencia generada con la herramienta nativa (p. ej. `npm ls --all --json`, `cargo metadata`). Ningún proceso del proyecto se ejecuta (`RNF-16`) |
| RF-VUL-02 | Consulta online a OSV: `querybatch` + `GET /v1/vulns/{id}` con caché por `id` + `modified`, paginación por `next_page_token` y reintentos con *backoff* | Must | MVP | Contra un servidor simulado: 5.000 paquetes generan lotes del tamaño configurado y se respetan los `next_page_token`. Una segunda ejecución sin cambios no vuelve a pedir detalles cuyo `modified` no cambió |
| RF-VUL-03 | Homebrew: delegar en `brew vulns --json` (≥ 7.0). Fallback offline con `advisories.json` y orden de versiones `_N`. Con Homebrew < 7, sugerir `brew update` | Must (delegar) / Should (fallback) | MVP / v1 | Con Homebrew 7.x, los hallazgos de `brew vulns --json` aparecen con ID, fórmula, versión y tipo de corrección. Offline: una fórmula instalada `X 1.2.3` frente a un registro `BREW-X-CVE-…` con `fixed: 1.2.3_1` → se reporta. Con `1.2.3_1` → no |
| RF-VUL-04 | Modo offline: descarga de volcados por ecosistema presente, índice local, actualización incremental con `modified_id.csv`, comparadores de versión locales | Should | v1 | Con la red cortada tras la descarga, un `Cargo.lock` con un crate afectado por un `RUSTSEC-*` conocido se reporta con ID y versión corregida, y `nettop` muestra 0 conexiones durante el escaneo. La actualización incremental solo descarga registros posteriores a la última marca |
| RF-VUL-05 | Privacidad: módulo desactivado por defecto; consentimiento con la lista de destinos y una previsualización de la carga; exclusión de paquetes privados; deduplicación | Must | MVP | Por defecto, 0 conexiones (`RNF-17`). Al activar, el diálogo muestra los destinos y el número de paquetes. En una captura con proxy solo aparecen los campos `ecosystem`/`name`/`version`/`purl`. Un paquete con `resolved` en un registro privado no aparece en la captura |
| RF-VUL-06 | Modelo de hallazgo completo (§5.7) | Must | MVP | La exportación JSON de cada hallazgo valida contra el JSON Schema publicado con los campos obligatorios de §5.7 |
| RF-VUL-07 | Deduplicación por alias | Must | MVP | Un paquete con `GHSA-x` y `CVE-y` como alias en OSV produce 1 hallazgo con ambos IDs |
| RF-VUL-08 | Severidad: CVSS 3.1/4.0 calculado desde el vector; si falta, la severidad de la fuente; si no, "desconocida" | Must | MVP | `CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H` → 9,8 Crítica. Sin vector y con severidad GHSA `HIGH` → "Alta (fuente: GHSA)". Sin nada → "Desconocida" |
| RF-VUL-09 | Priorización: KEV (P0) y EPSS (P1) con reglas deterministas y explicación | Should (KEV) / Could (EPSS) | MVP (KEV) / v1 (EPSS) | Un CVE presente en el KEV de prueba → P0 con el distintivo "Explotada activamente". El percentil EPSS se muestra con su fecha. "¿Por qué esta prioridad?" lista las reglas aplicadas |
| RF-VUL-10 | Dependencia directa o transitiva, con ruta, para los formatos que lo permiten | Should | MVP (npm, pnpm, Cargo, uv, Gemfile, go.mod) / v1 (resto) | En un *fixture* npm `a → b → c` con `c` vulnerable, el hallazgo muestra "transitiva: a › b › c" |
| RF-VUL-11 | Ámbito desarrollo/producción/opcional y su modificador de prioridad | Should | v1 | Una `devDependency` vulnerable con CVSS 9,1 → P2, con la explicación "solo desarrollo" |
| RF-VUL-12 | Versión corregida y comando sugerido por ecosistema y ubicación (§5.7), solo para copiar | Must | MVP | Para una dependencia Cargo transitiva se muestra `cargo update -p <crate> --precise <v>` con botón de copiar. No se ejecuta nada |
| RF-VUL-13 | Paquetes maliciosos (`MAL-*`) como P0 destacado, con la recomendación de rotar credenciales | Must | MVP | Un lockfile con un paquete listado en un `MAL-*` → P0, banner destacado y enlace a la guía de respuesta |
| RF-VUL-14 | Supresión con ámbito, motivo y caducidad obligatorios; reaparición al caducar | Must | MVP | No se puede suprimir sin motivo ni con fecha > 365 días. Con el reloj simulado pasada la fecha, el hallazgo reaparece con la etiqueta "supresión caducada" |
| RF-VUL-15 | Importar supresiones de `osv-scanner.toml` y `.cargo/audit.toml` | Could | v1 | Un `osv-scanner.toml` con `[[IgnoredVulns]] id="GHSA-…" ignoreUntil=…` suprime ese hallazgo con la fecha importada |
| RF-VUL-16 | Exportar informe: JSON (MVP); Markdown/HTML/CSV y SARIF 2.1.0 (v1); opción de anonimizar rutas | Should | MVP / v1 | El SARIF valida contra el esquema 2.1.0. Con "anonimizar", ninguna ruta absoluta aparece en el fichero |
| RF-VUL-17 | SBOM CycloneDX y SPDX 2.3 por proyecto y "SBOM del equipo", con purl en cada componente | Should | v1 | El CycloneDX pasa `cyclonedx validate`; el SPDX pasa el validador oficial de SPDX; el 100 % de los componentes tienen purl |
| RF-VUL-18 | Inventario de lo instalado en disco (`node_modules`, `site-packages`) y detección de discrepancias con el lockfile | Could | v1 | Un `node_modules` con una versión distinta de la del lockfile informa la versión instalada y la discrepancia |
| RF-VUL-19 | Binarios: Rust con `cargo auditable` (`~/.cargo/bin`) y buildinfo de Go (`~/go/bin`) | Could | v1 | Un binario Go en `~/go/bin` compilado con un módulo vulnerable se reporta con la ubicación "binario" |
| RF-VUL-20 | Apps Electron: versión embebida → `npm/electron`, con la etiqueta "indicativo" | Could | v1 | Una app con Electron 30.0.0 genera un hallazgo agregado: "N avisos de Electron, actualiza la app" |
| RF-VUL-21 | macOS desactualizado según SOFA: CVE corregidas y explotadas activamente | Should | v1 | Con un equipo en 27.0.1 y un feed con 27.2 → aviso con el número de CVE corregidas y explotadas activamente; un solo aviso combinado con el del doc 02 |
| RF-VUL-22 | Apps desactualizadas sin afirmar CVE (versión del cask o appcast de Sparkle) | Could | v2 | Una app con una versión inferior a la del cask aparece como "desactualizada", sin ningún CVE asociado |
| RF-VUL-23 | Extensiones de VS Code y derivados maliciosas conocidas (OSV `VSCode`) | Could | v1 | Una extensión `publisher.name@ver` presente en un registro `MAL-*` → P0 con la ruta de la extensión |
| RF-VUL-24 | GitHub Actions de proyectos (OSV `GitHub Actions`) | Could | v2 | Un workflow con `uses: owner/repo@<ref afectado>` se reporta |
| RF-VUL-25 | Pantalla de fuentes de datos y licencias (OSV, GHSA CC-BY-4.0, RustSec, Homebrew CC0, CISA KEV, FIRST EPSS, SOFA), visible antes de la primera descarga | Must | MVP | La pantalla lista cada fuente con licencia y enlace, y aparece en el flujo de activación |
| RF-VUL-26 | Escaneo periódico en segundo plano, solo lectura, con notificación solo de **nuevos** P0/P1 | Could | v1 | Con la programación semanal, solo hay notificación si aparece un P0/P1 nuevo. Ninguna acción se ejecuta (`RF-SEG-14`) |
| RF-VUL-27 | Frescura de datos visible; aviso si la base offline tiene más de 7 días | Must | MVP (fecha de consulta) / v1 (offline) | La vista muestra "Datos de: <fecha>". Con la base de hace 8 días aparece el aviso y la acción "Actualizar" |
| RF-VUL-28 | Higiene H1: claves SSH privadas sin passphrase, determinado por la cabecera | Should | v1 | En un *fixture* con una clave ed25519 sin cifrar y otra cifrada, solo se reporta la primera. Ninguna clave se carga en memoria salvo con el método alternativo explícito |
| RF-VUL-29 | Higiene H2/H3: permisos laxos y algoritmos débiles | Should (H2) / Could (H3) | v1 | `~/.ssh` con 755 → hallazgo con el `chmod` sugerido. Una clave DSA → hallazgo |
| RF-VUL-30 | Higiene H4/H5: credenciales en texto plano en rutas conocidas, con lista blanca de claves gestionadas por apps | Should | v1 | `~/.npmrc` con `_authToken` → hallazgo "mejorable" con el valor enmascarado. `~/.ollama/id_ed25519` no se reporta |
| RF-VUL-31 | Higiene H6: secretos versionados o no ignorados en proyectos descubiertos | Should | v1 | Un `.env` en `git ls-files` → hallazgo. `.env.example` → no |
| RF-VUL-32 | Higiene H7/H8: Descargas, Escritorio e historial de shell, con opt-in | Could | v2 | Desactivado por defecto. Activado, un `AuthKey_ABC123.p8` en Descargas se reporta con la ruta y sin el contenido |
| RF-VUL-33 | Reglas de privacidad de la higiene: enmascarado, sin logs, HMAC local, sin transmisión, sin validación contra APIs, excluido de la IA | Must | v1 (con la higiene) | Con un secreto de prueba sembrado, buscarlo en logs, journal, índice, exportaciones y tráfico de red da 0 coincidencias |

**Nota de coherencia con el doc 05.** El doc 05 sitúa `ra-vuln` en "v1 según 03". Este documento propone un **MVP mínimo** de vulnerabilidades (RF-VUL-01/02/03/05/06/07/08/12/13/14/25/27, solo online y opt-in) porque es el diferenciador frente a Mole, que no tiene análisis de vulnerabilidades. Si el equipo prefiere aligerar el MVP, todo RF-VUL puede pasar a v1 sin romper dependencias: ningún RF-DEV del MVP depende de RF-VUL. → Pregunta abierta Q1.

---

## 8. Necesidades de UI, riesgos y preguntas abiertas

### 8.1 Pantallas y componentes para el diseñador

| Pantalla o componente | Contenido | Acciones | Estados especiales |
|---|---|---|---|
| **Inicio de la zona Desarrollador** (modo Experto; en Simple, sugerencia si se detectan toolchains, doc 02) | Tarjetas por categoría: Proyectos, Cachés de paquetes, Toolchains, Xcode y simuladores, Android, Contenedores, IDE, Modelos de IA. Cada una con "recuperable de forma segura" y "para revisar" | Escanear, abrir la categoría | Sin herramientas detectadas → la zona se oculta; primer escaneo con progreso por raíz |
| **Lista de proyectos** | Árbol proyecto → artefactos. Columnas: nombre, ruta, ecosistemas (iconos), última actividad (relativa + estado), distintivos git (cambios locales, sin push, stash, worktree), tamaño recuperable, nivel por artefacto | Marcar o desmarcar, filtrar (ecosistema, estado, tamaño), ordenar, Revelar en Finder, Abrir en terminal o editor, Excluir (`RF-SEG`), Limpiar seleccionados | Artefacto bloqueado con su motivo (P1/P6/P8); "en uso" (P9); panel lateral "¿Por qué?" con las señales (inactivo 143 días, `CACHEDIR.TAG`, hay lockfile) y el comando para regenerar |
| **Toolchains** | Tabla por herramienta → versiones: tamaño, "por defecto", "Referenciado por: N" (expandible a proyectos y ficheros), nivel de evidencia E1–E4 | Desinstalar con el comando nativo (se muestra), Conservar (añadir a exclusiones) | Versión que satisface un rango, con aviso |
| **Plan con comando nativo** (modal) | Comando literal, salida del dry-run, elementos afectados, estimación | Confirmar, cancelar, copiar comando | Herramienta ausente o daemon parado; ejecución con log en vivo; resultado "Estimado / Liberado" |
| **Homebrew** | Huérfanas (con "Conservar" → `brew tab`), versiones antiguas, casks fantasma, taps, obsoletas o deshabilitadas | Las de §2.1 | Homebrew < 7 → sugerencia de actualizar |
| **Contenedores** | Desglose de `docker system df` por tipo y contexto | Prune por tipo; los volúmenes en una sección "Revisar" aparte | Docker parado; explicación de `Docker.raw` |
| **Modelos de IA** | Lista por fuente (HF, Ollama, LM Studio): nombre, tamaño, fecha, origen (descargado o creado en local) | Borrar con CLI nativo | "No se puede volver a descargar" para modelos locales |
| **Seguridad: panel** | Contadores P0–P3, KEV, `MAL`, modo (online/offline/híbrido), "Datos de: fecha", cobertura (ecosistemas escaneados y omitidos) | Escanear, actualizar base, exportar | Módulo desactivado → explicación + Activar; base caducada |
| **Seguridad: hallazgos** | Lista agrupable por proyecto, paquete o vulnerabilidad. Columnas: prioridad, ID(s), paquete@versión, corregida en, ubicación, directa o transitiva, ámbito | Filtrar, Ignorar (motivo + caducidad), Copiar comando, Abrir proyecto, Exportar | Agrupación de muchas transitivas; "otra plataforma"; "no resuelto" |
| **Detalle de hallazgo** | Resumen, detalle (Markdown), vector CVSS desglosado, EPSS, KEV, ruta de dependencia, referencias, "¿Por qué esta prioridad?" | Copiar comando, Ignorar, "Explicar con IA" (doc 04, opt-in) | Aviso retirado (`withdrawn`); sin corrección |
| **Activación y privacidad** | Destinos de red, previsualización ("se enviarán 412 paquetes de 6 ecosistemas"), exclusiones privadas, elección de modo, espacio para la base offline | Aceptar, elegir modo, editar exclusiones | — |
| **Supresiones** | Lista con ámbito, motivo, caducidad, origen (manual o importada) | Editar, reactivar, exportar o importar | Caducadas destacadas |
| **Exportar** | Asistente: alcance (proyecto, equipo, selección), formato (JSON, MD, HTML, CSV, SARIF, CycloneDX, SPDX), anonimizar | Guardar | — |
| **Higiene** | Hallazgos con valor enmascarado, severidad "mejorable" o "riesgo", guía de arreglo (`ssh-keygen -p`, `credential.helper osxkeychain`) | Revelar en Finder, marcar como aceptado (con caducidad) | Nunca hay botón de borrar el secreto; opt-in para Descargas, Escritorio e historial |
| **Fuentes de datos** | Fuentes, licencias, enlaces, fecha de cada base | — | — |

**Requisitos transversales de UI:** cada nivel con icono + texto (`RNF-22`); tablas navegables con VoiceOver y teclado (`RNF-20/21`); explicaciones en lenguaje llano para quien no es experto (p. ej. "node_modules: librerías que el proyecto descarga; se recuperan con `npm ci`").

### 8.2 Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Borrar trabajo no regenerable (`dist/` o `vendor/` versionados, `node_modules` parcheado a mano) | Alto | P1, P2, P10; Papelera por defecto; solo se preselecciona Seguro; catálogo con marcador hermano |
| La regeneración falla (registro privado sin VPN, paquete retirado del registro, runtime antiguo ya no descargable, modelo retirado del Hub) | Medio | P10 con el host; Precaución para re-descargas grandes; texto "puede no estar disponible" |
| `brew autoremove` elimina algo que el usuario usa directamente | Medio | Precaución + "Conservar" (`brew tab`) |
| Avisos TCC molestos o sorpresa de privacidad | Medio | Raíces sin TCC, opt-in explicado, medir Docker por CLI |
| Ejecución de código del repo (fsmonitor, filtros, scripts) | Alto | `RNF-16`, `gix`, CLI neutralizado, nunca `./gradlew` ni `npx` |
| Falsos positivos de vulnerabilidades (go.sum, rangos, backports, otras plataformas) | Medio: erosiona la confianza | Reglas de §5.8, delegar Homebrew, etiquetas "no resuelto" u "otra plataforma", supresiones con caducidad |
| Falsos negativos (sin cobertura: casks, librerías C embebidas, CocoaPods) | Medio | Mostrar siempre la **cobertura** ("escaneado / no escaneado / por qué"); nunca "estás seguro" |
| Fuga de información por consultas online (nombres privados, huella de dependencias) | Alto en empresas | Opt-in, exclusiones privadas, deduplicación, modo offline |
| Disponibilidad o límites de OSV, o cambios de API | Medio | Caché, backoff, modo offline; el esquema OSV es estándar |
| Licencias de datos (atribución CC-BY, redistribución) | Bajo-medio | RF-VUL-25; sin espejo propio en MVP (Q4) |
| Errores en comparadores de versión offline | Medio | Reutilizar crates probados; casos de prueba; contrastar con el modo online en CI |
| Rendimiento en monorepos enormes o discos lentos | Medio | Poda, presupuesto de tiempo, resultados en streaming, índice incremental |
| Cambios de rutas o formatos de herramientas (fnm, pipx, CocoaPods, Xcode) | Medio | Resolver rutas con el propio CLI cuando exista (`fnm env`, `pipx environment`, `pnpm store path`, `go env`); pruebas de contrato por versión |
| Higiene percibida como invasiva | Medio | Solo local, enmascarado, opt-in para Descargas, Escritorio e historial, sin validación online |

### 8.3 Preguntas abiertas para el usuario o el equipo

| # | Pregunta | Opciones y recomendación |
|---|---|---|
| Q1 | ¿Entra en el MVP un núcleo mínimo de vulnerabilidades (online, opt-in) o todo RF-VUL va a v1, como supone el doc 05? | Recomendado: MVP mínimo, por ser el diferenciador |
| Q2 | ¿Umbral de preselección por inactividad: 30 días (propuesto) o 7 (como Mole)? | 30 por defecto, configurable |
| Q3 | ¿Modo por defecto al activar vulnerabilidades: online u offline? | Online por simplicidad; offline recomendado en entornos corporativos |
| Q4 | ¿Se distribuirá alguna vez una base de vulnerabilidades preconstruida o un espejo propio? | Requiere revisar la licencia de cada fuente; en MVP, no |
| Q5 | ¿Se quiere ejecutar actualizaciones (`npm install`, `cargo update`) desde la app en algún momento? | v2 como mucho, con árbol git limpio y confirmación. En MVP y v1, solo copiar el comando |
| Q6 | ¿Se incluye la higiene de secretos? ¿Hasta dónde: solo rutas conocidas (v1) o también Descargas e historial (v2, opt-in)? | v1 limitada a H1, H2, H4, H5 y H6 |
| Q7 | ¿Profundidad por defecto 8 frente a 6? | 8 con poda |
| Q8 | ¿Se aceptan como raíces sugeridas las que salen de los recientes de los IDE (implica leer sus ficheros de estado)? | Sí, como propuesta que el usuario confirma |
| Q9 | ¿Hace falta Docker y Podman en v1, o basta Docker Desktop y OrbStack? | Docker CLI con contextos cubre la mayoría |
| Q10 | ¿Público objetivo de la higiene: solo desarrolladores (modo Experto) o también usuario común? | Solo Experto |

---

## 9. Fuentes

**Homebrew**
- Homebrew 7.0.0 (anuncio, `brew vulns`, advisory database, BrewUI): https://brew.sh/2026/09/13/homebrew-7.0.0/
- PR "Add brew vulns command": https://github.com/Homebrew/brew/pull/23080
- A. Nesbitt, "brew vulns: CVE scanning for Homebrew": https://nesbitt.io/2026/01/08/brew-vulns-cve-scanning-for-homebrew
- BleepingComputer, Homebrew 7.0.0: https://www.bleepingcomputer.com/news/security/homebrew-700-gets-built-in-gui-better-security-controls/
- Homebrew Advisory Database (README, formato, CC0): https://github.com/Homebrew/advisory-database
- Feed de avisos de Homebrew: https://formulae.brew.sh/api/advisories.json
- Tap Trust: https://docs.brew.sh/Tap-Trust
- Código instalado de Homebrew 7.0.11: `/opt/homebrew/Library/Homebrew/cmd/vulns.rb` y `vulns/*.rb` (solo lectura)

**OSV y bases de vulnerabilidades**
- OSV, descarga de datos (volcados, `modified_id.csv`, ecosistemas): https://google.github.io/osv.dev/data/
- OSV API `querybatch`: https://google.github.io/osv.dev/post-v1-querybatch/
- Lista de ecosistemas: https://storage.googleapis.com/osv-vulnerabilities/ecosystems.txt (tamaños medidos con peticiones HEAD a `…/<ECO>/all.zip` el 2026-11-01)
- Esquema OSV: https://ossf.github.io/osv-schema/
- osv-scanner, modo offline: https://google.github.io/osv-scanner/usage/offline-mode/
- GitHub Advisory Database (CC-BY-4.0): https://github.com/github/advisory-database
- RustSec advisory-db: https://github.com/rustsec/advisory-db
- CISA KEV (JSON): https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json
- EPSS API: https://api.first.org/data/v1/epss · CSV diario: https://epss.empiricalsecurity.com/epss_scores-current.csv.gz
- SOFA macOS feed v2: https://sofafeed.macadmins.io/v2/macos_data_feed.json
- NVD API 2.0: https://services.nvd.nist.gov/rest/json/cves/2.0

**Herramientas y ecosistemas**
- Rust 1.88 (GC automático de la caché de Cargo): https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/
- Variables de entorno de pipx (`PIPX_HOME` en macOS): https://pipx.pypa.io/latest/reference/environment-variables.html
- CLI de Hugging Face (`hf cache ls/rm/prune`): https://huggingface.co/docs/huggingface_hub/package_reference/cli
- Cache Directory Tagging Specification: https://bford.info/cachedir/
- Disco de Docker for Mac (`Docker.raw`, fuente secundaria): https://docker-docs.uclv.cu/docker-for-mac/space/
- depcheck archivado y migración a knip: https://github.com/depcheck/depcheck · https://github.com/SAP/ui5-server/pull/779 · knip: https://github.com/webpro-nl/knip
- Licencias y estado de repos (API de GitHub, 2026-11-01): gitleaks (MIT), ripsecrets (MIT), noseyparker (Apache-2.0, archivado), osv-scanner, Trivy, Grype y Syft (Apache-2.0), cargo-machete (MIT), kondo (MIT)
- TruffleHog AGPL-3.0 (fuentes secundarias): https://appsecsanta.com/sast-tools/gitleaks-vs-trufflehog
- crates.io (versión y licencia, 2026-11-01): `cargo-lock`, `rustsec`, `cargo-audit`, `cvss`, `osv`, `osv-db`, `cyclonedx-bom`, `cargo-cyclonedx`, `spdx`, `spdx-rs`, `packageurl`, `purl`, `semver`, `nodejs-semver`, `pep440_rs`, `pep508_rs`, `uv-pep440`, `versions`, `yarn-lock-parser`, `auditable-info`, `cargo-auditable`, `gix`, `git2`, `ignore`, `jwalk`, `trash`, `serde-sarif`, `cargo-machete`, `cargo-shear`, `cargo-udeps`, `cargo-sweep`, `kondo`, `sbom-walker` — https://crates.io/crates/<nombre>

**Verificaciones locales (solo lectura, 2026-11-01)**
- Semántica de atime en APFS (`touch -a -t`, `cat`, `stat -f %Sa`).
- Presencia de rutas (§2): `ls` sobre `~/.cargo`, `~/.rustup/toolchains`, `~/Library/pnpm`, `~/.cache/uv`, `~/.ollama`, `~/.lmstudio`, `~/Library/Developer/*`, etc.
- `CACHEDIR.TAG` en `target/`, `.venv/`, `.ruff_cache/`, `.mypy_cache/` y `.pytest_cache/`.
- Kegs de Homebrew: `sbom.spdx.json` (SPDX-2.3, purl `pkg:brew/…`) e `INSTALL_RECEIPT.json`.
- Ayuda de `brew vulns`, `brew cleanup`, `brew autoremove`, `brew tab`, `brew list` y `xcrun simctl runtime`.
- Versión de Electron en VS Code y Claude.app; consulta OSV `npm/electron@30.0.0` (47 avisos).
- Volcado OSV `VSCode` (solo `MAL-*`).
- Estructura de los feeds KEV y SOFA y de `advisories.json`.
