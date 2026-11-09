# Rotten Apple CLI · English messages (T-03, RF-CLI-09, RNF-24).
#
# Same ids as es.ftl (a test checks it). Integer arguments select the
# plural form and are formatted with the language's digit grouping; sizes
# and dates arrive already formatted.

## Units and counts

size-bytes = { $n ->
    [one] { $n } byte
   *[other] { $n } bytes
}
size-unit = { $value } { $unit }
count-items = { $count ->
    [one] { $count } item
   *[other] { $count } items
}

## Common

error-line = rotten: { $message }
confirm-prompt = { $question } [y/N]
cancelled = Cancelled; nothing was touched.
needs-yes = no interactive terminal: pass --yes (or --dry-run to only review)
permanent-needs-yes = --permanent deletes without the Trash and requires --yes
running-as-root = refusing to run as root: Rotten Apple works with your own permissions (use --allow-root if you really need it)
invalid-path = { $path }: { $detail }
dry-run-nothing-changed = Dry run: nothing was changed.
dry-run-nothing-moved = Dry run: nothing was moved.
item-problem = ! { $path } — { $problem }
not-freed-yet = No space is freed yet: that happens when the Trash is emptied.
to-undo = To undo: rotten undo { $op }
to-free = To free the space now (cannot be undone): rotten free { $op }
moved-to-trash = Moved to the Trash: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }).
confirm-trash = Move { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) to the Trash?

## scan

scan-header = Scan · catalog { $version } · { $status ->
    [complete] complete
    [partial] partial
   *[unavailable] unavailable
}
# $available: Finder's figure (free plus purgeable); $free, as diskutil.
scan-volume = Disk: { $used } used of { $capacity } · free { $free } · purgeable { $purgeable } · available { $available }
scan-volume-basic = Disk: { $used } used of { $capacity } · free { $free }
# $reclaimable: what would really be freed (once the Trash is emptied).
scan-category = { $id }  { $title }  { $reclaimable } in { $count ->
    [one] { $count } item
   *[other] { $count } items
} · preselected: { $preselected }
scan-note = ! { $note }
scan-config-corrupt = ! { $path } is corrupt ({ $reason }): no exclusions are applied and nothing can be cleaned until it is fixed
scan-summary = Preselected: { $reclaimable } would be freed ({ $count ->
    [one] { $count } item
   *[other] { $count } items
}) once the Trash is emptied. Review with `rotten clean --dry-run`.

## Plan review (clean, trash put)

plan-header = Plan { $hash } · { $count ->
    [one] { $count } item
   *[other] { $count } items
} · { $size } { $action ->
    [trash] to the Trash
   *[delete] PERMANENT DELETE
}
plan-item = [{ $id }] { $path }  { $category }  { $size }
plan-item-would-skip = [{ $id }] { $path }  { $category }  { $size }  (would skip: { $reason })
plan-skipped = Skipped ({ $count }):
plan-skipped-item = { $path } — { $reason }
plan-not-selected = Not selected in { $category }: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) — add them with --category { $category }
plan-rules = What this plan contains:
plan-rule = { $title } · { $risk ->
    [safe] Safe
    [caution] Caution
   *[review] Review
}
plan-rule-what = What it is: { $text }
plan-rule-if-deleted = If deleted: { $text }
plan-rule-regenerates = Does it come back? { $text }
plan-rule-why-risk = Why this risk: { $text }

## clean

clean-nothing = Nothing to clean.
confirm-delete = DELETE { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) FOREVER?
deleted-permanently = Permanently deleted: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }).

## trash put

trash-nothing = Nothing to move to the Trash.

## trash

trash-volume = { $path }: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size })
trash-volume-unreadable = ! { $path }: cannot be read without Full Disk Access (run `rotten doctor`); only what Rotten Apple recorded in its history is shown
trash-by-op = Rotten Apple, { $op }: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) · to free it: rotten free { $op }
trash-others = Other: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size })
to-empty = To empty it (cannot be undone): rotten trash empty

## trash empty

empty-header = Empty the Trash: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) will be deleted for good
empty-item = [{ $id }] { $path }  { $size }
empty-item-op = [{ $id }] { $path }  { $size }  (from { $op })
empty-item-would-skip = [{ $id }] { $path }  { $size }  (would be skipped: { $reason })
confirm-empty = Empty the Trash and delete { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) for good? This cannot be undone.
empty-nothing = The Trash is already empty.

## free

free-header = Free { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }): they will be deleted from the Trash for good
free-item = [{ $id }] { $path }  (was { $original })  { $size }
free-item-would-skip = [{ $id }] { $path }  { $size }  (would be skipped: { $reason })
free-not-freeable = { $op } [{ $item }] cannot be freed — { $reason }
confirm-free = Delete { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }) from the Trash for good? This cannot be undone.
free-nothing = Nothing to free.
freed = Deleted for good: { $count ->
    [one] { $count } item
   *[other] { $count } items
} ({ $size }).
# $measured: how much the volume's free space grew (statfs).
freed-space = Space freed: { $measured } measured · { $estimated } estimated
freed-space-unmeasured = Space freed: { $estimated } estimated (could not be measured)
freed-space-less = ! Much less was freed than estimated: hard links, APFS clones or snapshots may be holding the blocks

## history

history-empty = No operations yet.
history-op = { $op }  { $date }  { $kind ->
    [clean] clean
    [trash_put] trash put
    [free] free
    [trash_empty] empty Trash
   *[undo] undo { $of }
}  { $count ->
    [one] { $count } item
   *[other] { $count } items
}  { $size }  { $state ->
    [restorable] can be undone
    [partial] can be partly undone
    [irreversible] no longer reversible
    [restored] restored
    [partly_restored] partly restored
   *[not_applicable] —
}
history-journal-problem = ! journal: { $problem }

## undo

undo-header = Undo { $op } · { $count ->
    [one] { $count } item
   *[other] { $count } items
} to restore
undo-ready = [{ $id }] { $path }
undo-not-recoverable = [{ $id }] not recoverable — { $reason }
undo-nothing = Nothing to restore.
undo-confirm = Restore { $count ->
    [one] { $count } item
   *[other] { $count } items
}?
undo-restored-as = [{ $id }] restored as { $path }
undo-item-problem = ! [{ $id }] { $problem }
undo-done = Restored: { $count ->
    [one] { $count } item
   *[other] { $count } items
}.
not-recoverable-not-in-trash = it is no longer in the Trash or it has changed
not-recoverable-deleted = it was deleted permanently
not-recoverable-already-restored = it was already restored
not-recoverable-unknown = whereabouts unknown after an abrupt shutdown
not-recoverable-unknown-detail = whereabouts unknown after an abrupt shutdown: { $detail }
not-recoverable-never-moved = it was never moved
not-recoverable-other-trash = it was sent to a different Trash than the one in use
not-recoverable-volume-not-mounted = its Trash is not available (is the volume mounted?)

## doctor

doctor-version = rotten { $version } · catalog { $catalog }
doctor-platform = macOS { $macos } · { $arch } · supported platform (D-01): { $supported ->
    [yes] yes
   *[no] no
}
doctor-fda = Full Disk Access: { $state ->
    [granted] granted
    [denied] NOT granted (grant it to your terminal in System Settings › Privacy & Security)
   *[undetermined] undetermined
}
doctor-home = Home: { $path } ({ $source })
doctor-state = App state: { $path }
doctor-config = Config: { $path } ({ $state ->
    [ok] ok
    [missing] none yet
   *[corrupt] corrupt
})
doctor-config-error = Config: { $path } (corrupt: { $error })
doctor-journal = Journal: { $path } ({ $records ->
    [one] { $records } record
   *[other] { $records } records
}, { $problems ->
    [one] { $problems } unreadable line
   *[other] { $problems } unreadable lines
})
doctor-trash-system = Trash: the system one
doctor-trash-dir = Trash: { $path }
doctor-trash-unreadable = ! Interrupted operations with items that could not be looked up in the Trash (Full Disk Access missing): { $ops }
doctor-lock = Execution lock free: { $free ->
    [yes] yes
   *[no] no
}
doctor-apps = Running apps detected: { $count }
doctor-apps-unavailable = Cannot detect running apps: { $detail }

## exclude

exclude-none = No exclusions.
exclude-item = { $value }
exclude-result = { $change ->
    [added] Added: { $value }
    [present] Already present: { $value }
    [removed] Removed: { $value }
   *[absent] Not present: { $value }
}
exclude-result-dry = { $change ->
    [added] Would add: { $value } (dry run)
    [present] Already present: { $value }
    [removed] Would remove: { $value } (dry run)
   *[absent] Not present: { $value }
}

## Why something is skipped or protected

failed = failed: { $error }
crosses-device = it is or contains a mount point
unreadable = cannot be read: { $detail }
skip-missing = it no longer exists
skip-symlink-in-path = there is a symbolic link in the path
skip-identity-changed = it changed since the review
skip-contents-changed = its contents changed since the review
skip-conflict-exists = something already exists at the original path
skip-outside-trash = it is not in the Trash in use
skip-in-use = in use by { $process } (pid { $pid })
skip-in-use-unknown = cannot tell whether it is in use: { $detail }
skip-overlaps = it is inside { $path }, which is already in the plan
block-too-recent = modified { $age ->
    [one] { $age } day
   *[other] { $age } days
} ago (this rule waits { $min ->
    [one] { $min } day
   *[other] { $min } days
})
block-never-name = it contains { $name }, which this rule never touches
app-running = the app { $bundle } is open
app-unknown = cannot tell whether the app is open: { $detail }
flag-immutable = it is locked (immutable)
flag-append-only = it is append-only
flag-restricted = it is protected by macOS (SIP)
flag-no-unlink = the system does not allow deleting it
flag-firmlink = it is a system firmlink
flag-dataless = it is an iCloud file that is not downloaded
protected-inside = it is inside a protected path ({ $root })
protected-ancestor = it contains a protected path ({ $root })
protected-user-folder = it is in a documents folder ({ $root }): only by choosing it yourself
protected-user-folder-root = it is a user folder ({ $root }): its contents can be moved, not the folder
protected-home = it is the home folder
protected-volume-root = it is the root of a volume
protected-app-state = it is Rotten Apple's own data or journal
protected-bundle = it is a photo library or a Time Machine backup
protected-invalid-path = invalid path: { $detail }
protected-outside-home = it is outside the home folder: only paths inside ~ can be chosen
excluded-path = you excluded { $value }
excluded-contains-path = it contains { $value }, which you excluded
excluded-glob = you excluded { $value }
excluded-contains-glob = it contains something matching { $value }, which you excluded
excluded-rule = you excluded the rule { $value }
excluded-category = you excluded the category { $value }
excluded-bundle = you excluded the app { $value }

## Errors

error-locked = another Rotten Apple process is running an operation
error-unknown-category = unknown category: { $category }
error-invalid-exclusion = invalid exclusion: { $detail }
error-no-paths = give at least one path
error-relative-path = the path must be absolute: { $path }
error-config-corrupt = { $path } is corrupt ({ $reason }): nothing is done until it is fixed
error-wrong-home = the plan belongs to another home folder ({ $path })
error-plan-changed = the plan changed since the review: review it again
error-plan-expired = the review is too old ({ $minutes ->
    [one] { $minutes } minute
   *[other] { $minutes } minutes
}): review the plan again
error-plan-invalid = invalid plan: { $detail }
error-selection-protected = { $path }: { $reason }. Nothing is moved.
error-selection-user-folder-link = { $path }: it is a link to { $root }, a user folder that cannot be chosen itself. Nothing is moved.
error-unresolvable = cannot resolve { $path }: { $detail }. Nothing is moved.
error-unclassifiable = cannot classify { $path }: { $detail }. Nothing is moved.
error-nothing-to-undo = there is no operation to undo
error-unknown-operation = operation { $op } does not exist
error-not-undoable = operation { $op } cannot be undone
error-unknown-item = operation { $op } has no item { $item }
error-no-operations = give at least one operation
error-not-freeable = operation { $op } did not move anything to the Trash
error-trash-unreadable = cannot read the Trash ({ $path }): grant Full Disk Access to your terminal and run `rotten doctor`. Nothing is deleted.
error-empty-unreadable = cannot read the Trash ({ $path }): grant Full Disk Access to your terminal and run `rotten doctor`. Nothing is deleted. To free only what Rotten Apple sent there: rotten free <operation>.
error-no-home = the home folder is unknown: set HOME
error-env-path = { $var }: { $path }: { $detail }

## Command-line help (clap)

cli-heading-usage = Usage:
cli-heading-commands = Commands
cli-heading-arguments = Arguments
cli-heading-options = Options
cli-about = Rotten Apple: clean your Mac without fear. Everything goes to the Trash and can be undone.
cli-arg-json = JSON output (automatic when the output is not a terminal)
cli-arg-yes = Run without asking
cli-arg-quiet = Less text
cli-arg-no_color = No colours (the current output uses none)
cli-arg-debug = More detail in errors
cli-arg-lang = Language of the output for people (es or en)
cli-arg-allow_root = Allow running as root (RF-CLI-06)
cli-arg-help = Print help
cli-arg-version = Print version
cli-cmd-help = Print this message or the help of the given subcommand(s)
cli-cmd-help-arg-subcommand = The subcommand whose help to print
cli-cmd-scan = Smart scan: what can be cleaned, by category
cli-cmd-scan-arg-categories = Only these categories (CAT-01, CAT-03, CAT-08, CAT-10)
cli-cmd-clean = Moves the selection to the Trash after reviewing it
cli-cmd-clean-arg-dry_run = Shows the exact plan without changing anything
cli-cmd-clean-arg-permanent = Deletes without the Trash (irreversible; requires --yes)
cli-cmd-clean-arg-categories = Everything “Safe” in these categories, not only the preselection
cli-cmd-clean-arg-include_review = Also includes what is not “Safe” in those categories
cli-cmd-history = Previous operations and whether they can still be undone
cli-cmd-undo = Puts back what an operation sent to the Trash
cli-cmd-undo-arg-op_id = Operation (by default, the last one that can be undone)
cli-cmd-undo-arg-items = Only these items (comma-separated ids)
cli-cmd-undo-arg-dry_run = Shows what would be restored without moving anything
cli-cmd-free = Delete from the Trash for good what these operations moved there (cannot be undone)
cli-cmd-free-arg-op_ids = Operations whose items to free
cli-cmd-free-arg-dry_run = Show the exact plan without deleting anything
cli-cmd-doctor = Permissions, platform, configuration and journal integrity
cli-cmd-exclude = Exclusions: what is never touched
cli-cmd-exclude-add = Adds an exclusion: path, glob pattern, rule:<id>, cat:<CAT-xx> or bundle:<id>
cli-cmd-exclude-add-arg-value = The exclusion to add
cli-cmd-exclude-add-arg-dry_run = Shows the change without saving it
cli-cmd-exclude-list = Lists the exclusions
cli-cmd-exclude-remove = Removes an exclusion
cli-cmd-exclude-remove-arg-value = The exclusion to remove
cli-cmd-exclude-remove-arg-dry_run = Shows the change without saving it
cli-cmd-trash = Trash: with no subcommand, what is in it per volume and what Rotten Apple sent
cli-cmd-trash-empty = Empties the whole Trash after reviewing it (cannot be undone)
cli-cmd-trash-empty-arg-dry_run = Shows the exact plan without deleting anything
cli-cmd-trash-put = Moves the given paths to the Trash after reviewing them; it can be undone
cli-cmd-trash-put-arg-paths = Files or folders to move to the Trash
cli-cmd-trash-put-arg-dry_run = Shows the exact plan without changing anything
cli-value-categories = ID
cli-value-items = IDS
cli-value-op_id = OPERATION
cli-value-op_ids = OPERATION
cli-value-paths = PATH
cli-value-value = EXCLUSION
cli-value-lang = LANG
cli-value-subcommand = COMMAND
