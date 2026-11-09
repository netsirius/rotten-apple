# CLI de Rotten Apple · mensajes en español (T-03, RF-CLI-09, RNF-24).
#
# Los mismos ids que en.ftl (lo comprueba una prueba). Los argumentos
# enteros eligen el plural y se escriben con la agrupación del idioma; los
# tamaños y las fechas llegan ya formateados.

## Unidades y cantidades

size-bytes = { $n ->
    [one] { $n } byte
   *[other] { $n } bytes
}
size-unit = { $value } { $unit }
count-items = { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}

## Común

error-line = rotten: { $message }
confirm-prompt = { $question } [s/N]
cancelled = Cancelado; no se ha tocado nada.
needs-yes = sin terminal interactiva hace falta --yes (o --dry-run para solo revisar)
permanent-needs-yes = --permanent borra sin pasar por la Papelera y exige --yes
running-as-root = no se ejecuta como root: Rotten Apple trabaja con tus permisos (usa --allow-root si de verdad lo necesitas)
invalid-path = { $path }: { $detail }
dry-run-nothing-changed = Simulación: no se ha cambiado nada.
dry-run-nothing-moved = Simulación: no se ha movido nada.
item-problem = ! { $path } — { $problem }
not-freed-yet = Aún no se ha liberado espacio: eso ocurre al vaciar la Papelera.
to-undo = Para deshacer: rotten undo { $op }
to-free = Para liberar ya el espacio (no se puede deshacer): rotten free { $op }
moved-to-trash = Movido a la Papelera: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }).
confirm-trash = ¿Mover a la Papelera { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size })?

## scan

scan-header = Escaneo · catálogo { $version } · { $status ->
    [complete] completo
    [partial] parcial
   *[unavailable] no disponible
}
# $available: lo de Finder (libre más purgable); $free, como diskutil.
scan-volume = Disco: { $used } usados de { $capacity } · libre { $free } · purgable { $purgeable } · disponible { $available }
scan-volume-basic = Disco: { $used } usados de { $capacity } · libre { $free }
# $reclaimable: lo que se liberaría de verdad (al vaciar la Papelera).
scan-category = { $id }  { $title }  { $reclaimable } en { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} · preseleccionado: { $preselected }
scan-note = ! { $note }
scan-config-corrupt = ! { $path } está corrupto ({ $reason }): no se aplican exclusiones y no se podrá limpiar hasta arreglarlo
scan-summary = Preseleccionado: se liberarían { $reclaimable } ({ $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}) al vaciar la Papelera. Revisa con `rotten clean --dry-run`.

## Revisión del plan (clean, trash put)

plan-header = Plan { $hash } · { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} · { $size } { $action ->
    [trash] a la Papelera
   *[delete] BORRADO PERMANENTE
}
plan-item = [{ $id }] { $path }  { $category }  { $size }
plan-item-would-skip = [{ $id }] { $path }  { $category }  { $size }  (se omitiría: { $reason })
plan-skipped = Omitidos ({ $count }):
plan-skipped-item = { $path } — { $reason }
plan-not-selected = Sin marcar en { $category }: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }) — añádelos con --category { $category }
plan-rules = Qué hay en este plan:
plan-rule = { $title } · { $risk ->
    [safe] Seguro
    [caution] Precaución
   *[review] Revisar
}
plan-rule-what = Qué es: { $text }
plan-rule-if-deleted = Si se borra: { $text }
plan-rule-regenerates = ¿Vuelve a aparecer? { $text }
plan-rule-why-risk = Por qué este riesgo: { $text }

## clean

clean-nothing = Nada que limpiar.
confirm-delete = ¿BORRAR PARA SIEMPRE { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size })?
deleted-permanently = Borrado permanentemente: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }).

## trash put

trash-nothing = Nada que mover a la Papelera.

## trash

trash-volume = { $path }: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size })
trash-volume-unreadable = ! { $path }: no se puede leer sin Acceso total al disco (ejecuta `rotten doctor`); solo se muestra lo que Rotten Apple anotó en su historial
trash-by-op = Rotten Apple, { $op }: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }) · para liberarlo: rotten free { $op }
trash-others = Otros: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size })
trash-partial = ! Algo dentro no se pudo leer entero: el tamaño real es mayor y eso no se vaciará
to-empty = Para vaciarla (no se puede deshacer): rotten trash empty

## trash empty

empty-header = Vaciar la Papelera: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }) se borrarán para siempre
empty-item = [{ $id }] { $path }  { $size }
empty-item-op = [{ $id }] { $path }  { $size }  (de { $op })
empty-item-would-skip = [{ $id }] { $path }  { $size }  (se omitiría: { $reason })
confirm-empty = ¿Vaciar la Papelera y borrar para siempre { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size })? No se puede deshacer.
empty-nothing = La Papelera ya está vacía.

## free

free-header = Liberar { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }): se borrarán para siempre de la Papelera
free-item = [{ $id }] { $path }  (era { $original })  { $size }
free-item-would-skip = [{ $id }] { $path }  { $size }  (se omitiría: { $reason })
free-not-freeable = { $op } [{ $item }] no se puede liberar — { $reason }
confirm-free = ¿Borrar para siempre { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }) de la Papelera? No se puede deshacer.
free-nothing = Nada que liberar.
freed = Borrado para siempre: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} ({ $size }).
# $measured: lo que ha aumentado el espacio libre del volumen (statfs).
freed-space = Espacio liberado: { $measured } medido · { $estimated } estimado
freed-space-unmeasured = Espacio liberado: { $estimated } estimado (no se pudo medir)
freed-space-less = ! Se ha liberado bastante menos de lo estimado: enlaces duros, clones APFS o instantáneas pueden retener los bloques

## update

update-homebrew = Instalado con Homebrew ({ $prefix }).
update-would-run = Se ejecutaría: { $command }
update-confirm = ¿Ejecutar { $command }? Homebrew descargará e instalará la versión nueva.
update-running = Ejecutando: { $command }
update-done = Homebrew ha terminado.
update-app = Instalado dentro de la app { $app }.
update-app-howto = La app se actualizará sola con Sparkle, que aún no está disponible. Mientras tanto, sustituye la app por la versión nueva.
update-cargo = Instalado con cargo install ({ $dir }).
update-run-yourself = Para actualizar, ejecuta: { $command }
update-other = No se sabe cómo se instaló { $exe }.
update-other-brew = Para recibir actualizaciones, instálalo con Homebrew: { $command }
update-other-source = Si lo compilaste tú, vuelve a compilarlo con el código actualizado.
error-update-no-exe = no se sabe dónde está el ejecutable de rotten: { $detail }
error-update-no-brew = no se encuentra brew en { $path }
error-update-brew-spawn = no se pudo ejecutar { $command }: { $detail }
error-update-brew-failed = brew upgrade terminó con el código { $code }
cli-cmd-update = Actualiza Rotten Apple por la vía con la que se instaló (Homebrew, app o cargo)
cli-cmd-update-arg-dry_run = Dice qué se haría y con qué comando, sin ejecutar nada

## history

history-empty = Aún no hay operaciones.
history-op = { $op }  { $date }  { $kind ->
    [clean] limpieza
    [trash_put] a la Papelera
    [free] liberar
    [trash_empty] vaciar la Papelera
   *[undo] deshacer { $of }
}  { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}  { $size }  { $state ->
    [restorable] se puede deshacer
    [partial] se puede deshacer en parte
    [irreversible] ya no es reversible
    [restored] restaurada
    [partly_restored] restaurada en parte
   *[not_applicable] —
}
history-journal-problem = ! journal: { $problem }

## undo

undo-header = Deshacer { $op } · { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
} para restaurar
undo-ready = [{ $id }] { $path }
undo-not-recoverable = [{ $id }] no recuperable — { $reason }
undo-nothing = Nada que restaurar.
undo-confirm = ¿Restaurar { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}?
undo-restored-as = [{ $id }] restaurado como { $path }
undo-item-problem = ! [{ $id }] { $problem }
undo-done = Restaurados: { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}.
not-recoverable-not-in-trash = ya no está en la Papelera o ha cambiado
not-recoverable-deleted = se borró permanentemente
not-recoverable-already-restored = ya se restauró
not-recoverable-unknown = paradero desconocido tras un cierre abrupto
not-recoverable-unknown-detail = paradero desconocido tras un cierre abrupto: { $detail }
not-recoverable-never-moved = no se llegó a mover
not-recoverable-other-trash = la mandó a otra Papelera distinta de la que se usa ahora
not-recoverable-volume-not-mounted = su Papelera no está disponible (¿está montado el volumen?)

## doctor

doctor-version = rotten { $version } · catálogo { $catalog }
doctor-platform = macOS { $macos } · { $arch } · plataforma soportada (D-01): { $supported ->
    [yes] sí
   *[no] no
}
doctor-fda = Acceso total al disco: { $state ->
    [granted] concedido
    [denied] NO concedido (concédelo a tu terminal en Ajustes del Sistema › Privacidad y seguridad)
   *[undetermined] no determinable
}
doctor-home = Carpeta personal: { $path } ({ $source })
doctor-state = Estado de la app: { $path }
doctor-config = Configuración: { $path } ({ $state ->
    [ok] correcta
    [missing] aún no existe
   *[corrupt] corrupta
})
doctor-config-error = Configuración: { $path } (corrupta: { $error })
doctor-journal = Journal: { $path } ({ $records ->
    [one] { $records } registro
   *[other] { $records } registros
}, { $problems ->
    [one] { $problems } línea ilegible
   *[other] { $problems } líneas ilegibles
})
doctor-trash-system = Papelera: la del sistema
doctor-trash-dir = Papelera: { $path }
doctor-trash-unreadable = ! Operaciones interrumpidas con elementos que no se pudieron buscar en la Papelera (falta Acceso total al disco): { $ops }
doctor-lock = Cerrojo de ejecución libre: { $free ->
    [yes] sí
   *[no] no
}
doctor-apps = Apps abiertas detectadas: { $count }
doctor-apps-unavailable = No se pueden detectar las apps abiertas: { $detail }

## exclude

exclude-none = No hay exclusiones.
exclude-item = { $value }
exclude-result = { $change ->
    [added] Añadida: { $value }
    [present] Ya estaba: { $value }
    [removed] Quitada: { $value }
   *[absent] No estaba: { $value }
}
exclude-result-dry = { $change ->
    [added] Se añadiría: { $value } (simulación)
    [present] Ya estaba: { $value }
    [removed] Se quitaría: { $value } (simulación)
   *[absent] No estaba: { $value }
}

## config

config-value = { $value }
config-list-empty = (lista vacía)
config-set-result = { $change ->
    [changed] Guardado: { $key } = { $value }
   *[unchanged] Sin cambios: { $key } ya era { $value }
}
config-set-result-dry = { $change ->
    [changed] Se guardaría: { $key } = { $value } (simulación)
   *[unchanged] Sin cambios: { $key } ya es { $value }
}
config-path = Configuración: { $path }
config-link-linked = Enlace: { $link } → { $path }
config-link-missing = Enlace: { $link } (no existe; lo crea «rotten config set»)
config-link-conflict = ! { $link } es { $found }, no el enlace a la configuración: no se limpia nada hasta que lo quites de ahí (la configuración que se usa es { $path })
config-link-created = Enlace creado: { $link } → { $path }
config-link-error = ! No se pudo crear el enlace { $link }: { $error }
config-link-parent-not-directory = ! No se puede crear el enlace { $link }: { $parent } es un fichero, no una carpeta
config-link-not-applicable = Enlace: no aplica con ROTTEN_STATE_DIR (ni se crea ni se comprueba)
config-unknown-keys = ! { $path } tiene claves que esta versión no conoce y no usa (se conservan): { $keys }
config-link-found-file = un fichero
config-link-found-directory = una carpeta
config-link-found-symlink = un enlace a { $target }
config-link-found-other = algo que no es un fichero
config-link-found-unreadable = algo que no se puede comprobar ({ $detail })
config-expected-one-of = uno de estos: { $values }
error-config-link-conflict = { $link } es { $found }, no el enlace a la configuración ({ $path }): no se hace nada hasta que lo quites de ahí
error-unknown-config-key = clave de configuración desconocida «{ $key }»; las claves son: { $keys }
error-read-only-config-key = «{ $key }» se cambia con «rotten exclude add|remove», no con «rotten config set»
error-invalid-config-value = valor no válido «{ $value }» para «{ $key }»: se espera { $expected }
cli-cmd-config = Configuración: leerla, cambiarla y dónde está
cli-cmd-config-get = El valor de una clave (o el que se aplica por defecto)
cli-cmd-config-get-arg-key = La clave: language o exclusions.*
cli-cmd-config-set = Cambia una clave, tras validarla; crea el enlace en ~/.config/rotten/ si no hay nada
cli-cmd-config-set-arg-key = La clave: language
cli-cmd-config-set-arg-new_value = El valor nuevo
cli-cmd-config-set-arg-dry_run = Muestra el cambio sin guardarlo
cli-cmd-config-path = Dónde está la configuración y su enlace en ~/.config/rotten/
cli-value-key = CLAVE
cli-value-new_value = VALOR

## Por qué se omite o se protege algo

failed = falló: { $error }
crosses-device = es o contiene un punto de montaje
unreadable = no se puede leer: { $detail }
skip-missing = ya no existe
skip-symlink-in-path = hay un enlace simbólico en la ruta
skip-identity-changed = ha cambiado desde la revisión
skip-contents-changed = su contenido ha cambiado desde la revisión
skip-conflict-exists = ya hay algo en la ruta original
skip-outside-trash = no está en la Papelera que se usa
skip-not-verifiable = no se puede comprobar que lo que tiene dentro siga como al revisarlo
skip-in-use = lo usa { $process } (pid { $pid })
skip-in-use-unknown = no se puede saber si está en uso: { $detail }
skip-overlaps = está dentro de { $path }, que ya está en el plan
block-too-recent = modificado hace { $age ->
    [one] { $age } día
   *[other] { $age } días
} (esta regla espera { $min ->
    [one] { $min } día
   *[other] { $min } días
})
block-never-name = contiene { $name }, que esta regla nunca toca
app-running = la app { $bundle } está abierta
app-unknown = no se puede saber si la app está abierta: { $detail }
flag-immutable = está bloqueado (inmutable)
flag-append-only = solo admite añadir datos (append-only)
flag-restricted = lo protege macOS (SIP)
flag-no-unlink = el sistema no permite borrarlo
flag-firmlink = es un firmlink del sistema
flag-dataless = es un fichero de iCloud sin descargar
protected-inside = está dentro de una ruta protegida ({ $root })
protected-ancestor = contiene una ruta protegida ({ $root })
protected-user-folder = está en una carpeta de documentos ({ $root }): solo eligiéndolo tú
protected-user-folder-root = es una carpeta del usuario ({ $root }): se puede mover su contenido, no la carpeta
protected-home = es la carpeta personal
protected-volume-root = es la raíz de un volumen
protected-app-state = son datos o el journal del propio Rotten Apple
protected-bundle = es una fototeca o una copia de Time Machine
protected-invalid-path = ruta no válida: { $detail }
protected-outside-home = está fuera de la carpeta personal: solo se pueden elegir rutas dentro de ~
excluded-path = excluiste { $value }
excluded-contains-path = contiene { $value }, que excluiste
excluded-glob = excluiste { $value }
excluded-contains-glob = contiene algo que coincide con { $value }, que excluiste
excluded-rule = excluiste la regla { $value }
excluded-category = excluiste la categoría { $value }
excluded-bundle = excluiste la app { $value }

## Errores

error-locked = otro proceso de Rotten Apple está ejecutando una operación
error-unknown-category = categoría desconocida: { $category }
error-invalid-exclusion = exclusión no válida: { $detail }
error-no-paths = indica al menos una ruta
error-relative-path = la ruta debe ser absoluta: { $path }
error-config-corrupt = { $path } está corrupto ({ $reason }): no se hace nada hasta arreglarlo
error-wrong-home = el plan es de otra carpeta personal ({ $path })
error-plan-changed = el plan ha cambiado desde la revisión: revísalo de nuevo
error-free-plan-changed = el plan ha cambiado desde la revisión; ya no están: { $items }. No se borra nada: revísalo de nuevo
error-trash-gone = la carpeta de la Papelera { $path } ya no está (¿se ha desmontado su volumen?). No se borra nada.
error-plan-expired = la revisión es demasiado antigua ({ $minutes ->
    [one] { $minutes } minuto
   *[other] { $minutes } minutos
}): revisa el plan de nuevo
error-plan-invalid = plan no válido: { $detail }
error-selection-protected = { $path }: { $reason }. No se mueve nada.
error-selection-user-folder-link = { $path }: es un enlace a { $root }, una carpeta del usuario que no se puede elegir en sí. No se mueve nada.
error-unresolvable = no se puede resolver { $path }: { $detail }. No se mueve nada.
error-unclassifiable = no se puede clasificar { $path }: { $detail }. No se mueve nada.
error-nothing-to-undo = no hay ninguna operación que deshacer
error-unknown-operation = la operación { $op } no existe
error-not-undoable = la operación { $op } no se puede deshacer
error-unknown-item = la operación { $op } no tiene el elemento { $item }
error-no-operations = indica al menos una operación
error-not-freeable = la operación { $op } no mandó nada a la Papelera
error-trash-unreadable = no se puede leer la Papelera ({ $path }): concede Acceso total al disco a tu terminal y ejecuta `rotten doctor`. No se borra nada.
error-empty-unreadable = no se puede leer la Papelera ({ $path }): concede Acceso total al disco a tu terminal y ejecuta `rotten doctor`. No se borra nada. Para liberar solo lo que mandó Rotten Apple: rotten free <operación>.
error-no-home = no se sabe cuál es la carpeta personal: define HOME
error-env-path = { $var }: { $path }: { $detail }

## Ayuda de la línea de órdenes (clap)

cli-heading-usage = Uso:
cli-heading-commands = Comandos
cli-heading-arguments = Argumentos
cli-heading-options = Opciones
cli-about = Rotten Apple: limpia tu Mac sin miedo. Todo va a la Papelera y se puede deshacer.
cli-arg-json = Salida JSON (automática si la salida no es una terminal)
cli-arg-yes = Ejecutar sin preguntar
cli-arg-quiet = Menos texto
cli-arg-no_color = Sin colores (también con NO_COLOR)
cli-arg-debug = Log técnico detallado, también por la salida de errores
cli-arg-lang = Idioma de la salida para personas (es o en)
cli-arg-allow_root = Permitir ejecutar como root (RF-CLI-06)
cli-arg-help = Muestra la ayuda
cli-arg-version = Muestra la versión
cli-cmd-help = Muestra esta ayuda o la de los subcomandos indicados
cli-cmd-help-arg-subcommand = El subcomando cuya ayuda mostrar
cli-cmd-scan = Escaneo inteligente: qué se puede limpiar, por categoría
cli-cmd-scan-arg-categories = Solo estas categorías (CAT-01, CAT-03, CAT-08, CAT-10)
cli-cmd-clean = Mueve a la Papelera lo seleccionado tras revisarlo
cli-cmd-clean-arg-dry_run = Muestra el plan exacto sin cambiar nada
cli-cmd-clean-arg-permanent = Borra sin pasar por la Papelera (irreversible; exige --yes)
cli-cmd-clean-arg-categories = Todo lo «Seguro» de estas categorías, no solo lo preseleccionado
cli-cmd-clean-arg-include_review = Incluye también lo que no es «Seguro» de esas categorías
cli-cmd-history = Operaciones anteriores y si aún se pueden deshacer
cli-cmd-undo = Devuelve a su sitio lo que una operación mandó a la Papelera
cli-cmd-undo-arg-op_id = Operación (por defecto, la última que se puede deshacer)
cli-cmd-undo-arg-items = Solo estos elementos (ids separados por comas)
cli-cmd-undo-arg-dry_run = Muestra qué se restauraría sin mover nada
cli-cmd-free = Borra para siempre de la Papelera lo que mandaron allí estas operaciones (no se puede deshacer)
cli-cmd-free-arg-op_ids = Operaciones cuyos elementos liberar
cli-cmd-free-arg-dry_run = Muestra el plan exacto sin borrar nada
cli-cmd-doctor = Permisos, plataforma, configuración e integridad del journal
cli-cmd-exclude = Exclusiones: lo que nunca se toca
cli-cmd-exclude-add = Añade una exclusión: ruta, patrón glob, rule:<id>, cat:<CAT-xx> o bundle:<id>
cli-cmd-exclude-add-arg-value = La exclusión que añadir
cli-cmd-exclude-add-arg-dry_run = Muestra el cambio sin guardarlo
cli-cmd-exclude-list = Lista las exclusiones
cli-cmd-exclude-remove = Quita una exclusión
cli-cmd-exclude-remove-arg-value = La exclusión que quitar
cli-cmd-exclude-remove-arg-dry_run = Muestra el cambio sin guardarlo
cli-cmd-trash = Papelera: sin subcomando, lo que hay por volumen y lo que mandó Rotten Apple
cli-cmd-trash-empty = Vacía la Papelera entera, tras revisarla (no se puede deshacer)
cli-cmd-trash-empty-arg-dry_run = Muestra el plan exacto sin borrar nada
cli-cmd-trash-put = Mueve a la Papelera las rutas indicadas, tras revisarlas; se puede deshacer
cli-cmd-trash-put-arg-paths = Ficheros o carpetas que mover a la Papelera
cli-cmd-trash-put-arg-dry_run = Muestra el plan exacto sin cambiar nada
cli-value-categories = ID
cli-value-items = IDS
cli-value-op_id = OPERACIÓN
cli-value-op_ids = OPERACIÓN
cli-value-paths = RUTA
cli-value-value = EXCLUSIÓN
cli-value-lang = IDIOMA
cli-value-subcommand = COMANDO

## rotten completion (T-14)

cli-cmd-completion = Script de autocompletado para zsh, bash o fish
cli-cmd-completion-arg-shell = La shell: zsh, bash o fish
cli-value-shell = SHELL
