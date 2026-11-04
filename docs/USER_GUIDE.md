# Directory Navigator

Native Rust terminal file manager inspired by DOS Navigator 1.51.
Original Pascal references are kept locally in `original-dn/` (excluded from Git).
Directory Navigator is licensed under the [MIT license](../LICENSE).

```sh
cargo build --release
./target/release/dn
```

Starts in the current directory. Minimum terminal: 80×25. Unicode names, native
paths, mouse menus, panel clicks, double-click opening and wheel scrolling work.
Ctrl+click marks files. F4 opens the native UTF-8 editor.

## First start

Directory Navigator shows a Welcome page in the native Help window when `config.conf` does not exist. It lists the active profile’s controls, opening behavior and
current deletion mode. **Start**, Esc or the title close control dismisses it;
**Keyboard settings** and **Help** return to the welcome when closed. **≡ → Welcome…**
opens it again without changing panel state. Tab/Shift+Tab select the action links;
Enter or a click activates one. PageUp/PageDown and the mouse wheel scroll.
Welcome and Help share the centered 72×20 frame, colors and shadow; the footer
shows the controls for the active page. Both support resizing and the shared
Expand/Restore title button.

Dismissing the welcome writes no files. Normal settings saving on exit prevents
it appearing on later launches. If saving is disabled and no configuration exists,
it appears next time. Existing, invalid or inaccessible `.conf` settings do not
trigger it; settings errors are reported separately. Load desktop never triggers it.
There is no startup delay or splash timer. CLI help/version bypass the interface.

## Keyboard profiles

Keyboard-profile and regional choice fields use native Turbo Vision controls.
Left/Right or Space keeps the existing cycle behavior. Press F4, or click the
field's arrow, to open a choice list; arrows select and Enter or a click accepts.
Esc closes that list and restores focus to its parent. A choice updates the
dialog's draft; the parent dialog's OK applies it, and Cancel discards it.

When a dialog has an Enter-default action, its button is highlighted immediately
with the DN yellow caption and cyan pointers where space permits. Input fields
and lists keep keyboard focus. Selecting another button moves the highlight to
that button; unsaved-editor confirmation highlights its existing Cancel default.
Tab and Shift+Tab cycle forward/backward through enabled fields, lists and buttons,
wrapping at the ends. Enter activates the focused button, or the dialog's default
from a field/list. Space changes checkbox/radio options. Esc cancels the current
dialog and returns to its parent; previously completed operations stay completed.

**Options → Configuration → Keyboard shortcuts** selects **Automatic**, **Classic
DN** or **Mac-friendly**. Apply changes the profile immediately; Cancel preserves
it. Save/Load desktop and normal settings persistence include `[application]` with `shortcuts = "auto"`,
`shortcuts = "classic_dn"` or `shortcuts = "mac"` in `config.conf`. Missing values
use Classic DN on every platform, including macOS. Explicitly saved choices are
preserved. Automatic remains available and selects Mac-friendly on macOS, Classic
DN elsewhere. Select Mac-friendly explicitly when preferred, including over SSH.

The Mac profile uses **Ctrl+O, release, then another key**. `^O` in menus and
function bars means this prefix. A context guide appears while waiting; Esc
or a mouse click dismisses it without editing text. In the file manager,
Ctrl+O twice reveals terminal output; the existing letter/number sequences remain.
Text prompts retain their normal input behavior. Existing DN and modern Ctrl aliases remain
available; Command stays with the terminal, and no terminal settings are modified.

| After Ctrl+O | File manager |
| --- | --- |
| 1 … 9, 0 | F1 … F9, F10: Help, User menu, View, Edit, Copy, Move, MkDir, Trash/delete, Menu, Quit |
| S / F | Quick filename search / recursive Find |
| I / Z | File information / Sort menu |
| B / L / E | Bookmarks / filename filter / columns |
| H / J / A / V | Command / directory / edit / view histories |
| O / P / Y | OS default application / list export / Tetris |
| K / M / R | Keyboard settings / menu / refresh |
| Ctrl+O | Reveal terminal output (Ctrl+O twice total) |
| Space | Mark highlighted entry, even when ordinary Space selection is disabled |
| Up / Down / Left / Right | Page up / page down / first entry / last entry (also `[` / `]` / `,` / `.`) |

The editor retains the same numeric function-key meanings: **2 Save**, **3 Open**,
**6 Duplicate**, **7 Find**, **0 Menu**. Letter alternatives: **A Save As**, **W Close**,
**X Quit DN**, **N Next match**, **P Reverse**, **R Replace**, **G Go to line**, and
**C Clear selection**. Ctrl+S/F/G/Z/Y/C/X/V keep their existing editor meanings.
After Ctrl+O, Left/Right moves by words; add Shift to the arrow to select.
Up/Down (or `[`/`]`) page, `,`/`.` move to line start/end, and H/E (or `<`/`>`) to document start/end.
Open, Close and Quit retain unsaved-change guards.

In the viewer, **2 Wrap**, **3 Close**, **7 Find**, **0 Quit DN** follow the existing
function keys; **N/P** continue/reverse search and **W** closes. The prefix does
not change controls in games or text-entry dialogs. In the user menu, Ctrl+O then 2 switches global/local, 4 edits its definition, R reloads and W returns/cancels; commands still require Run confirmation. Use ordinary arrows, Tab,
Enter, Esc and the existing clickable controls there.

The control tables below describe the Classic DN keys, also retained as aliases.

Enter or double-click browses ZIP/TAR/TAR.GZ/TGZ archives inside DN; other files
use a configured custom association, or open in the operating system's default
application. Custom associations can also override archive opening. Ctrl+Enter
or File → Open with default app always bypasses custom rules and archive browsing.
F3 explicitly opens DN's viewer; F4 explicitly opens DN's editor. Ordinary
directories stay inside DN. Links to files open externally, links to directories
navigate normally, and broken links report an error. On macOS, `.app` bundles
launch instead of being browsed as directories.

Opening uses macOS `/usr/bin/open`, Linux `xdg-open` (requires xdg-utils and a
desktop session), or Windows ShellExecute. Native paths are passed directly,
without shell command substitution. Launching is asynchronous; opener errors
are reported in DN without falling back to the viewer/editor.

### Custom file associations

Use **Options → File associations → Edit associations…** to edit
`associations.conf` beside `config.conf`. Only this global file supplies rules;
files inside browsed directories cannot change launching behavior. The first Edit
creates a commented template without enabling any rules. Save and close the editor
to reload automatically, or use **Reload associations** after an external edit.
Rules also load at startup and with Load desktop.

```conf
.txt = @editor
.log = @viewer
.bin = @hex
.zip = @default
.md = code --wait %f
```

Match the final extension, case-insensitively; `<none>` matches extensionless names.
`@viewer`, `@hex`, `@editor` use the built-in tools; `@default` uses the OS opener.
Directories, symlinks and macOS application bundles retain their normal behavior.
F3/F4 always use DN's viewer/editor, independently of these rules.

External targets use an executable followed by arguments. Quotes group arguments;
`%f` is the full native file path, `%d` its directory and `%n` its filename stem.
Each placeholder must occupy a whole argument, for example `tool --file %f`.
Unlike RDN's textual command substitution, these values remain separate native
arguments. No shell, wildcard or environment-variable expansion is implicit.
Use an explicit shell for scripts or pipelines. Commands run with your permissions
in the active directory using the existing terminal handoff; Enter returns to DN.

Limits: UTF-8 configuration, 64 KiB, 128 unique rules, 256 arguments per rule.
Duplicate extensions and invalid rules report errors; Ctrl+Enter remains available
for the OS default. Failed custom commands do not silently launch another tool.

Directory names appear without a leading slash. While browsing, both panels
refresh quietly in the background roughly every two seconds. Surviving marks,
the highlighted filename and scroll position are retained where possible.
If the displayed directory is removed, DN shows its nearest existing parent
and reports the recovery. Menus/dialogs defer automatic updates; Ctrl+R requests
an immediate refresh.

| Key | Action |
| --- | --- |
| Tab / Shift+Tab | Switch panel |
| Arrows, Home/End, PgUp/PgDn | Navigate |
| Enter / Backspace | Open directory, archive or default application / parent directory |
| Ctrl+Enter | Open with OS default application (also in File menu) |
| Insert / Space | Toggle mark and advance |
| Ctrl+A / Ctrl+U | Mark all / clear marks |
| + / - | Select / unselect group by name pattern |
| Alt+Backspace | Directory history (session only) |
| Alt+PageUp / Alt+PageDown | Edited / viewed file history (session only) |
| Ctrl+1…9 / Alt+1…9 | Set / go to directory bookmark (also Panel → Quick directories) |
| Alt+Delete | Apply a cached filename filter; empty or * clears |
| Ctrl+K / Ctrl+B / Alt+Y | Calculator / ASCII–CP437 table / memory information |
| Ctrl+C / Alt+L | Compare directories / export file list |
| Ctrl+F7 / Ctrl+Q | Recursive Find / opposite-panel quick view |
| Alt+E / Alt+K | Read-only file information / column presets |
| F1 / F2 / F3 | Help / user menu / text or hex viewer |
| Shift+F2 | Cycle sorting (Name → Extension → Size → Date) |
| F5 / F6 | Copy / move marked items (or current entry) |
| Shift+F6 | Rename in current directory |
| F7 / F8 | Create directory / confirm move to Trash (default) |
| F9 / F10 | Menu / exit |
| Alt+F/U/P/M/O | File / Utilities / Panel / Manager / Options |
| Alt+X | Exit |
| Alt+0 | Open the session window list |
| Ctrl+F3 / Ctrl+F4 / Ctrl+F5 / Ctrl+F6 | Sort by name / extension / newest date / largest size |
| Ctrl+H / Ctrl+L / Ctrl+R | Toggle hidden / go to directory / refresh |
| Esc | Close viewer/dialog or cancel operation |

Independent tools are desktop windows: Directory Tree, Find/results, Directory
Sizes, file information, Calculator, character table,
system/memory information, bookmarks, histories and standalone Help. They remain
open when you switch away. Reopening activates the existing tool;
file-information windows distinguish file paths. Navigation and View/Edit retain
the originating tool's selection and results. Quick directories closes after
Go To successfully navigates its source pane; empty slots or failed navigation
leave the window available for another choice.

Click an exposed window to raise it; the first client click activates without
operating a control. Clicking exposed file-manager background focuses the panes
without covering floating windows. Alt+F6 / Alt+Shift+F6 cycle in creation order;
Ctrl+Tab / Ctrl+Shift+Tab also work when the terminal reports their modifiers.
Ordinary Tab still navigates controls. Alt+0 opens the window list. The manager
remains first; closing a window returns to the most recently used live window.

Tools, Tetris and Music Player expose the main DN menu. Editors/viewers keep their
own menus. Main-menu file/panel actions target the manager's current pane. F6 and
Shift+F6 keep their Move/Rename meanings in that pane. Settings, confirmations,
Copy/Move forms, destination pickers and Tetris pages still block switching.
Help from a blocking form stays with that form; ordinary Help is a switchable
window and closes with its originating document if that document is retired.

File → Find searches filename patterns beneath the originating directory, with
subdirectory and hidden-entry options. Advanced criteria include literal UTF-8
content, optional case matching, entry type, minimum/maximum byte size and
inclusive modification dates. Sizes accept bytes or K/M/G (1024-based); dates
use YYYY-MM-DD in UTC. Blank fields are unrestricted. Content matching uses
Unicode lowercase when Match text case is off; it is literal, not regex.
Alt+F7 remains cached quick search.
Results arrive in the background; Stop retains partial results and Close cancels.
Enter/double-click jumps to the containing folder and highlights the result;
F3/F4 view/edit regular files, starting at the first matching line when content
was searched. Existing editor/viewer file limits still apply. New search returns
to the retained criteria. Jumping clears that panel's filename filter and
reveals hidden results when necessary. Patterns are case-sensitive Unicode `*`/`?`.
Results stop at 10,000; skipped paths and the first error are reported. Traversal
does not follow directory links and reports nesting deeper than 512 as skipped.
Content reads use bounded buffers, reject special files/final symlinks, check
cancellation between chunks and report files changed during reading as skipped.

Panel → Calculate directory sizes snapshots visible marked directories, or the
highlighted directory. It reports apparent bytes, file/directory/link counts and
skipped entries without changing listing columns. Hard links count separately;
links contribute their own metadata length, without following targets. Partial
totals are labelled. Stop/Close are available; Recalculate starts a fresh snapshot.

Manager → Quick view (Ctrl+Q) previews the highlighted entry over the inactive
panel after a short debounce. Its directory/cursor/marks/layout remain intact.
Wheel over the preview to scroll; F3 opens the ordinary full viewer. Ctrl+Q or
the title close control restores the panel; Tab closes preview and switches.
Directories/links show information without recursive reads or link-target
previews. Existing text/hex preview limits apply; preview reads do not enter
viewer history. These tools are session-only and add no configuration keys.

Copy/move dialogs default to the other panel's directory. A typed path is
relative to that base unless absolute. A single item can use a new destination
name; multiple items require an existing destination directory. Copy and
Move/rename use DN's expanded dialog with these supported choices:

- **Refuse existing destinations** (default): reject the batch before writing if
  any top-level destination already exists.
- **Skip all existing files**: leave existing files, directories and broken links
  untouched, copying/moving only the remaining items. Transfer Center records
  any skips. Existing directories are skipped as a whole; their contents are never
  merged. Skipped sources are retained.
- **Check free disk space**: estimate regular-file lengths before writing. This is
  advisory: filesystem overhead and later space changes can still cause errors.
  An unavailable space reading stops the operation before writing.
- **Verify disk writes**: read back and compare staged file bytes before publishing.
- **Remove source files (Copy only)**: force verification, copy the whole selected
  batch, then use the guarded move cleanup. A failed or cancelled copy retains its sources;
  errors during cleanup retain remaining sources and completed destinations.
- **Tree**: choose a directory without navigating either panel. The history arrow
  or **F4** recalls up to 16 attempted destinations for this session. **F1/Help**
  explains the options. Child dialogs return to the current draft.

Overwrite, append, refresh and description copying are displayed disabled.
Nothing replaces an existing destination, including a destination created after
preflight. Move/rename defaults to Refuse and offers the same explicit Skip choice.
Its **Remove source files** box is checked and locked, so the operation stays a
move. **F6** uses the other panel's directory; **Shift+F6** uses the active panel's
directory for renaming. Both dialogs share destination history, as in DN.

Move/rename prefers native exclusive rename. Its space check runs only when
copying is required; a native rename needs no copy capacity. Selecting **Verify
disk writes** uses staged copying even on the same volume, then guarded source
cleanup after the whole copied batch succeeds. Cancellation/errors keep completed
native renames and published copies; there is no automatic rollback.

Copy and Move open the **Transfer Center**, a switchable desktop window. One
transfer runs at a time; additional requests join its queue. Browse, edit and
switch windows while it runs. **Manager → Transfers…** or the clickable status
beside the command line reopens the session's results. Esc, the title close
control and Close hide the window without cancelling work.

Select a request to see its phase, current path, item/skip counts, bytes and
elapsed time. The native progress bar uses estimated totals where available;
large, unreadable or changing trees can have unknown totals. Speed counts actual
copied bytes, not native renames. Remaining time is shown only when meaningful.
Enter opens details; F1 opens Help. Cancel selected requests cancellation of
active work and waits for acknowledgement, or removes a queued request without
writing. Errors and cancellation pause the queue; Resume queue starts waiting
requests without retrying failures. Completed work is retained.

The queue is session-only: up to 64 active/pending requests, 100 finished
summaries and 100 skip/error details per request, with omitted-detail counts.
Queued roots and destination mappings are revalidated before execution. Directory
contents are inspected when execution starts, rather than frozen at queue time. A
completion refreshes affected locations without changing foreground focus or
unrelated selections. Delete, Trash and Mkdir keep blocking progress dialogs and
remain unavailable while transfers are pending. Editor saves retain their normal
independent safeguards.

Quitting with transfers pending offers **Wait and quit**, **Cancel transfers and
quit**, and **Return to DN** (the default). Cancellation waits for the worker;
failure while waiting aborts exit. Dirty-document review, recovery and settings
save checks still apply. No transfer queue is restored after restarting DN.

Moves prefer native no-overwrite rename, with a copy fallback when needed.
Symlinks are copied as links and are not recursively traversed. Copy metadata
support varies by platform. No overwrite, automatic retry or concurrent transfers
are introduced.

F8 and Delete move items to Trash by default, with confirmation. Change
**Options → File Manager → Setup → Move deleted files to Trash** to select
permanent deletion instead; confirmation remains mandatory. Save/Load desktop
persists the choice. Older desktop files default to Trash. Confirmation windows
state which mode will run; changing settings does not change an open confirmation.
Trash failure never falls back to permanent deletion. Cancellation stops between
items; completed items remain in Trash. Restore using your desktop file manager.

macOS uses native NSFileManager through the `trash` crate (no Finder automation
or external command); Finder's Put Back availability varies, so restoration may
require dragging the item out of Trash. Unrepresentable native paths are refused.
Linux uses FreeDesktop home Trash with original-path/date metadata and exclusive
rename; cross-volume Trash is currently refused rather than copying/deleting.
Windows uses native Recycle Bin operations. Linux/Windows runtime behavior has
not yet been validated here. No in-app restore browser or empty-Trash action.

White directories, green executables, cyan source, magenta archives, red media,
blue links and yellow marks use the DOS-derived palette. Actual ANSI shades
follow terminal preferences; high background intensity is steady, not blinking.

The viewer loads at most 2 MiB / 100,000 lines, clips very long lines and labels
bounded previews. Binary files display in hex. Arrows/page keys scroll; Esc or
the Close button returns to panels. Preferences save automatically on normal exit by default.
Options → Save desktop saves immediately; Options → Configuration → System Setup
can disable saving on exit. Save desktop also stores this opt-out. This saves
preferences, editor options, bookmarks and panel layouts/sorts, not open documents,
current directories, filters or session histories. Cancelled dialogs do not save
their draft changes.
Options → Load desktop reloads those preferences. Options → Configuration
contains System Setup, Interface and the read-only Confirmation policy. Options → File
Manager contains Setup and Highlight Groups. Enter applies setup changes;
Esc/Cancel discards them. Menus follow `dlgMainMenu` in the original English
resource, with cascades, mnemonic letters and aligned shortcuts. Dropdowns show
implemented commands; unsupported placeholders are omitted. Arrow keys navigate;
Right opens a submenu, Left/Esc
returns, and letters activate the highlighted mnemonics.
Defaults load from XDG_CONFIG_HOME/dn/config.conf, HOME/.config/dn/config.conf,
or APPDATA/Directory Navigator/config.conf on Windows. Only `config.conf` is loaded;
`config.txt` is ignored and never read, migrated or modified. Without a `.conf`
file, defaults and the first-run welcome apply.
The UTF-8 TOML file requires `config_version = 2`, grouped sections and lossless
native bookmark encoding. Unknown TOML values are kept. Invalid contents are preserved under an `_old_`
prefix and defaults load; inaccessible files still report errors.
Writes use a private sibling file and atomic replacement; read-only files and
symlink destinations are refused. A short-lived `config.conf.lock` coordinates
saves, and external changes require Load desktop before saving again. If a crash
leaves a lock, remove it only after confirming no other copy is saving.
Save failures cancel exit and retain the session; retry or disable saving on exit.
Unchanged files are not rewritten. Force-killing the process does not save changes.

Group selection matches visible files and folders in the cached listing, without
recursion. `*` matches any sequence; `?` matches one Unicode scalar character.
Matching is case-sensitive; `*.*` requires a dot. Existing unrelated marks are
preserved. Undecodable native names match universal `*` and remain selectable
with Insert; other patterns skip them.

File Attributes / Info reads fresh metadata in the background, without following
the selected symlink. It shows exact bytes, local modification time with UTC
offset, permissions and the link target. Arrows/page keys scroll, Left/Right
reveals long paths, Home resets scrolling, and Enter/Esc/Close dismisses it.

Setup columns and Options → File Manager → Column defaults open the same
two-panel preset dialog. Name only, Name + Size, and Name + Size + Modified
preview immediately; Cancel restores both panels. Dates include time when a
panel's inner width reaches 50 columns. Large sizes use compact units; file
information retains exact bytes. Save/Load desktop stores both presets; old
configuration files default to Name + Size. Chrono provides local date formatting.

Validation: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
`cargo build --release`. Testing so far is on macOS ARM64 only.

Directory comparison uses visible cached file metadata, never file contents or recursive scans.
List export refuses existing destinations and paths that cannot be represented as UTF-8 lines.
Like editor Save As, complete list publication requires filesystem hard-link support.

Utilities also provides System Information. Information dialogs are read-only snapshots;
Refresh updates them. Unsupported or denied native fields display their errors.
Calculator and character-table Copy use DN's internal editor clipboard (Ctrl+V).

The character table (Ctrl+B) uses Turbo Vision's native Table control. Choose
ASCII or DOS CP437, then use the arrow keys or click a cell. Home/End select the
first/last byte; PageUp/PageDown move through rows. Tab moves between the grid,
page choices, Copy and Close. Byte values, Unicode code points and UTF-8 bytes
appear below the grid. ASCII controls cannot be copied; CP437 displays the DOS
font symbols for low bytes. Each page remembers its selection during the session.

The calculator (Ctrl+K) has a compact DN-style expression field and a five-row
result box: DEC, HEX, BIN, OCT and EXP. Choose a row, then Copy or Ctrl+C. EXP is
approximate scientific notation with 15 significant digits; DEC keeps exact
64-bit integers. Long values are shortened on screen but copied in full.
The arrow beside the expression opens session history; choosing a previous
expression does not evaluate it. Up/Down in the field still recall history.
Tab/Shift+Tab move focus, Enter evaluates, F1 opens calculator help and Esc closes
history first, then the calculator. Other utilities retain their existing layout.

Text viewer: F2 wraps, F7 searches the loaded preview, Shift+F7 continues and
Ctrl+F7 searches backward. Search and wrapping are disabled in Hex view.
Filters preserve excluded marks; file operations/export use visible marks only.
Bookmarks persist only when explicitly saving the desktop.

**Options → File Manager → Highlight groups** defines five ordered filename
rules. Enable a group, edit semicolon-separated Unicode `*`/`?` masks and choose
a foreground color (0–15) with arrows, Space or a click. Matching ignores case;
the first enabled matching group wins. Unmatched files retain their existing
type colors. Folders/links, marks and the active selection retain priority.
Group foregrounds use the current terminal palette and preserve the configured
panel background. The global Highlight file types switch controls both built-in
and custom rules. Defaults resets the five sample groups to disabled; OK applies,
Cancel/Esc discards the draft. Accepted rules save with the existing settings
flow in TOML v2; old configurations retain their existing file colors.

DN colors use fixed terminal color indices, with gray panels and red/gray
function bars by default. **Options → Colors** follows DOS Navigator’s group/item
layout: choose an implemented component and item, then edit its foreground and
background in the 4×4 color grids. Changes preview immediately; the sample shows
the selected attribute. Tab/Shift+Tab changes controls, arrows navigate lists or
color grids, and Enter activates the focused button (otherwise OK). G/I focuses
the group/item list; F/B focuses foreground/background. Lists support scrolling.
**Color** restores DN defaults; **B/W** and **Mono** provide grayscale presets.
**Default colors set** restores all default UI attributes. OK keeps the changes;
Cancel, Esc or the close icon restores the complete previous color table.
Save desktop or normal exit with Save on exit persists accepted colors in the
existing configuration. The former Classic/Terminal selector is removed; older
`palette="terminal"` settings load DN defaults. VGA hardware palette controls and
blinking are omitted. Interface → Show clock controls the upper-right clock.

Screen savers: Options → Configuration → Screen savers follows DN 1.51 with
**Selected savers** on the left, **Available savers** on the right, and **Add / Remove**
between them. The four built-in effects are available on macOS, Linux and Windows:

- **Star flight** (formerly Star Sky): accelerating stars moving out from the center.
- **Flash-light**: a moving light reveals a frozen snapshot of DN on a black screen.
  Resizing clips or pads that same snapshot; it never follows live file/editor changes.
- **Clock**: moving local `HH:MM` time in 24-hour format, with a blinking colon.
- **Blackness**: a plain black screen.

Add appends the highlighted available effect without duplicates; Remove deletes the
highlighted selected effect. Each automatic or manual start randomly chooses one
of the selected effects; repeats are possible. An empty selection disables automatic
screen rest, and a manual start explains where to select an effect. External DOS
`SSAVERS/*.SS` programs and hardware mouse-corner controls are not supported.

Tab / Shift+Tab moves between controls; arrows select list entries; Insert / Delete
adds / removes. **Preview** (P) shows the last highlighted effect from either list,
even if it has not been added. F1 or Help explains the controls. OK applies the draft;
Cancel, Esc or the close button discards it. Preview does not apply or save changes.
Never / 1 / 2 / 5 / 10 / 30 / 60 minute idle delays remain available. Existing settings
without a selected list retain Star flight and their previous delay; new settings
default to Star flight and 10 minutes. Save desktop or normal exit with Save on exit
persists accepted choices. The ≡ menu’s Screensaver starts screen rest immediately.
Any key, paste or mouse input dismisses it and is consumed. Idle activation waits
until jobs, games, menus and dialogs finish; editor/viewer contents are preserved.

Panel scrollbars reserve a fixed right-hand column in Brief and Details layouts.
The marker tracks the highlighted entry, like original DN. Click arrows to move,
click the track to page, or drag the marker; the wheel moves three entries.


Directory tree: Ctrl+T, Manager → Directory tree.
Arrow keys navigate; Right/+ expands, Left/− collapses or selects the parent,
and Space toggles a branch. Enter or double-click opens the selected folder in
the originating panel. Ctrl+R/Refresh rereads the selected branch; Ctrl+H/Hidden
changes visibility inside the tree. Tab selects buttons; Esc/Ctrl+F4 or the title
close control cancels. Wheel, branch markers and scrollbar arrows/track work by
mouse. Branch loading preserves the highlight's screen position.

The tree follows original DN's folding/location controls with a modern lazy
browser: one-level background reads, native volume roots and paths, bounded
10,000-directory branches/20,000 cached children, and cancellation on close.
Directory links are leaves; Enter follows one explicitly. Initial location resolves
path aliases. Errors stay visible and can be retried. The tree is session-only;
no disk-wide cache, recursive expansion, mutations or drive selector is added.


User menu: F2 or Utilities → User menu. Like original DN, `dn.mnu` uses
`>1` headings, `>2` submenus (up to eight levels), `~letter~` hotkeys and `;`
comment lines. `>1 -` creates a separator. Arrow keys/Enter and mouse clicks
navigate; Left/Esc returns. F1 shows help. Within the menu, F2 switches between
local/global definitions, F4 edits the definition and Ctrl+R reloads it. Options
also provides Global/Local menu definition. Editor/viewer function keys retain
their existing meanings.

The nearest `dn.mnu` in the active directory or its ancestors takes precedence;
the global fallback is `dn.mnu` beside `config.conf`. Missing definitions show
built-in examples without creating files; F4 creates a missing definition
exclusively. Existing definitions use the editor's guarded save flow and reload
after closing. Invalid definitions remain editable. Menus must be UTF-8 regular
files (not symlinks), at most 128 KiB/256 items/32 commands per action.

Example for macOS/Linux:

```text
>1 ~F~ile tools
>2 ~L~ist marked paths
printf '%s\n' '{selected}'
>2 File ~i~nformation
ls -ld -- '{file}'
>1 -
>1 ~P~rint directory
printf '%s\n' '{dir}'
```

Each command line is an executable and separate arguments. Single/double quotes
group arguments; backslash escapes quotes, backslash or unquoted whitespace.
Placeholders occupy whole arguments: `{file}`, `{name}`, `{dir}`, `{other_file}`,
`{other_dir}`, `{selected}`, `{other_selected}`, `{shell}`. `{selected}` expands to visible marked
entries in listing order, falling back to the highlighted entry; `..` is excluded.
Filtered-out marks alone produce an error. Native filename bytes remain intact.
Expansion is bounded to 2,048 arguments/64 KiB per command and 256 KiB per action.
`{other_selected}` uses the same rules for the opposite pane.
`{selected_list}` and `{other_selected_list}` provide a path to a temporary,
newline-delimited file list. Lists use native filename bytes, reject names with
CR/LF, and contain at most 2,048 paths/256 KiB. They are created in private
directories only after Run and removed after completion or failure.

An action can declare up to eight parameter prompts before its command lines:

```text
>1 ~S~earch selected files
? query "Text to find" "TODO"
grep -n -- '{param:query}' '{selected}'
```

Each `? name "Label" "default"` declares a field; the default is optional.
`{param:name}` occupies one whole argument, including spaces or Unicode.
Parameters and pane context are captured for the action; editing a parameter
never evaluates shell syntax. Enter/Review opens the existing command review;
Run remains a separate step. Esc from parameters cancels without execution or
temporary-file creation. Nested Help preserves the draft and focus.

Every action shows its working directory, panel context, executable and arguments
before Run. Commands run sequentially in the active directory, stopping on failure.
DN restores the normal terminal for interactive programs, then waits for Enter
before returning and refreshing the panels. On macOS/Linux, Ctrl+C interrupts the
command and returns to that prompt. Windows command interruption is unverified.

There is no implicit shell or DOS batch expansion. For pipelines/built-ins/scripts,
invoke a shell explicitly, e.g. `/bin/sh -c 'printf "%s\n" "$DN_FILE"'`.
`DN_FILE`, `DN_DIR`, `DN_OTHER_FILE` and `DN_OTHER_DIR` are native environment
values; quote their expansions. Windows has a small `cmd` starter menu. Original
DOS `!`, `$`, `%1`…`%9` and batch flags are not translated; use the native
parameter and selection placeholders above instead.


ZIP, TAR, TAR.GZ and TGZ archives open directly in the active file pane with
Enter, double-click or File → Browse archive. Each pane navigates independently;
the header shows the archive and its internal directory. Sorting, filename filters,
quick search and normal marking controls work inside archives. Enter opens folders
or previews regular members; F3 opens the bounded read-only text/hex viewer.
Closing the viewer returns to the same pane. Backspace and `..` go up; at the archive
root they return to disk and highlight the archive. Ctrl+PgUp returns to disk from
any depth. The outer cursor, marks, filter and scroll position are restored against
a refreshed directory listing. Esc cancels indexing or preview work; Ctrl+R rereads
the archive. F1 explains archive-pane controls.

F5 extracts marked items, or the current item when there are no marks, through the
shared Copy dialog and Transfer Center. Folders include descendants and empty
subdirectories. Paths are relative to the current archive directory: `a.txt` inside
`docs` extracts as `a.txt`; selecting `docs` at the root includes `docs/a.txt`.
The other disk pane supplies the default destination. When both panes show archives,
enter an explicit absolute disk path or use Tree/history. Copy to Temp Drive works
through the same extraction path. Accepted jobs continue after leaving the archive.
Refuse stops on existing top-level destinations; Skip leaves each existing file or
folder untouched, without merging directories. Preview and Cancel never extract.

Archives remain read-only: extract before editing or running commands on members.
Move, deletion, mkdir, copying into archives, source removal, encrypted members,
links/special-file extraction, archive creation and nested archive browsing are
unsupported. Unsupported or unsafe selected trees and incomplete catalogs are
rejected before extraction. Portable output names exclude Windows device names,
case-folding collisions and names invalid on Windows. No archive ownership or
privileged permission bits are restored. Available times and ordinary permissions
are retained. Files are streamed into private staging on the destination volume;
checksums and source identity are checked before guarded no-overwrite publication.
Cancellation removes owned staging; already published items remain if a later
publication fails. Previewed members stay in memory and do not enter file history.

Limits remain 512 MiB per archive source; 10,000 members; 20,000 virtual nodes;
16 MiB tree metadata; 64 path levels/4 KiB paths. TAR traversal/decompression is
limited to 256 MiB. Indexing/preview reads have ten-second work budgets; full
extraction streams separately with cancellation and a one-hour worker read budget.
ZIP indexing validates the central directory within a 64 MiB read budget; full
payload reads are separately bounded by the accepted source size. Preview remains
limited to 2 MiB/100,000 lines, while extraction can read full members. ZIP supports
Stored and Deflate; ZIP64, multi-volume/self-extracting layouts and duplicate ZIP
names are unsupported. ZIP timestamps use local time; TAR timestamps use UTC.
The native codecs use the existing `zip`, `tar` and `flate2` dependencies.


Menu scope: ≡, File, Utilities, Panel, Manager and Options. Directory tree lives
under Manager; Tab/Shift+Tab switch panels. Historical aliases such as Panel Setup and
Options → File Manager → Setup remain. The editor keeps its original headings;
Block (Alt+B) offers selection, cut, copy and paste. Unassigned editor function
keys have no action labels or mouse targets. Paragraph alone remains
intentionally dimmed, with no dropdown. Editor preferences remain under Options.
Confirmation policy explains safeguards; it does not offer switches to disable
them. Save/Load desktop continues to persist colors and settings.

Unsupported commands are not menu placeholders. Disk administration/recovery,
DOS communications/country/printer setup and built-in printing are outside this
app's scope. Save all remains outside this pass; desktop movement and zoom use
the shared window controls described below. Future work includes permission editing
and archive creation.
Split/combine and line drawing remain low priority. No settings format changes.

**Ctrl+O** in the Classic profile reveals the terminal's real primary screen and
scrollback. In the Mac profile, press **Ctrl+O twice**; **Alt+F5** and
**Manager → Terminal output** also reveal it. **Ctrl+O, Esc or Enter** returns to DN.
Use your terminal's own scrolling, text selection and copy controls while output
is visible. The view is read-only: typing, paste and clicks do not execute commands
or edit the saved command draft. It starts no shell and does not replay output.
Background work continues; dialogs and editors keep their state, and resize causes
a full redraw on return. Blocking dialogs must be closed before revealing output.

Output includes commands run from DN and earlier shell scrollback. Programs using
their own alternate screen control what they leave in the primary terminal.
After a DN command finishes, Enter, Esc or Ctrl+O returns immediately to the panes.

The bottom row is a command prompt with a blinking cursor, not a status line.
Type a single-line command and press Enter to run it in the active directory,
using `$SHELL -c` (or `%COMSPEC% /D /S /C` on Windows). Commands run with your
normal permissions in the real terminal, without file-operation safeguards.
After completion, output stays visible until Ctrl+O, Esc or Enter. Background
polling continues; returning restores the panes, with listings refreshed.
An empty Enter still opens the highlighted entry. Esc clears the command;
Left/Right/Home/End, Backspace and Delete edit it when nonempty. Up/Down still
navigate files. Ctrl+E/Ctrl+X recall previous/next submitted commands (100 unique,
session-only); Shift+Insert pastes the internal clipboard. Bracketed paste never
executes commands, and multiline/control-character input is rejected.
Space/+/-/* retain selection actions when the prompt is empty. `cd`, `cd ~` and
`cd "directory with spaces"` navigate the active panel; other shell commands use
a child process, so environment changes do not persist. Long paths and commands
scroll/clip to keep the insertion cursor visible. No command history is saved.

## Language and regional formats

Options → Configuration → Language / date & time offers English and Russian.
Use Up/Down or Tab to choose a row; Left/Right or Space cycles its value. Clicking
a row also cycles it. Enter/OK applies the draft; Esc/Cancel discards it. The
preview shows the chosen format before applying. Language and formats are independent.
English menu shortcuts still work alongside Russian mnemonics.

Choose YMD/DMY/MDY, date separators `-`, `.` or `/`, 12/24-hour time and time
separators `:` or `.`. Footer dates use a compact two-digit year; column dates
use four digits. File information keeps the timezone offset. Archive dates retain
their original timezone meaning. Machine timestamps, filenames, commands and file
contents are unaffected. The 12-hour clock reserves extra header space for AM/PM.

Preferences persist through the usual settings store and Save/Load desktop:
`language=en|ru`, `date_order=ymd|dmy|mdy`, `date_separator`, `time_format=24|12`,
and `time_separator`. Older configurations retain English/ISO date/24-hour defaults.
Unknown language/format values use safe defaults.

UTF-8 catalogs in `locales/en.lang` and `locales/ru.lang` are compiled into the
binary. Each record is an English message ID, a tab, and its translation.
Backslash/newline/tab use `\\`, `\n`, `\t` escapes; `~` marks a mnemonic.
Keep corresponding IDs and unique menu mnemonics when editing translations, then
rebuild. Catalogs are not loaded from the current directory. Missing resources
fall back to English; some detailed diagnostics remain English and OS errors are
shown unchanged. No system locale or timezone settings are changed.

## Command history and copying names

Utilities → Commands History (Alt+F8) lists the current session’s commands, most
recent first, with up to 100 unique entries. Type or paste a filter to match any
part of a command, ignoring case. Arrows, Home/End and page keys select entries;
Tab moves between the filter, list and buttons. Enter/Use or double-click puts
the exact command back in the prompt for editing. It never runs automatically.
Esc/Cancel keeps the previous prompt and cursor. Ctrl+E/Ctrl+X recall still works.
History is not written to disk.

File → Copy names / paths → Names / Full paths copies visible marked entries,
or the highlighted entry if no marks exist. The order matches the panel listing.
Multiple entries are separated by newlines, without a final newline or shell
quoting. Parent `..` is excluded; marks hidden by filtering do not cause fallback
to the highlighted entry. Invalid Unicode, embedded CR/LF and text over 2 MiB
are rejected without changing the clipboard.

This uses DN’s internal text clipboard, shared with the editor and utilities,
not the operating system clipboard. Shift+Insert pastes into the command prompt
only when its existing single-line validation permits the text. Copying paths
does not insert or execute a command.

## Classic panel footer

Below the listing, each panel shows the highlighted entry in cyan, a centered
yellow selection summary with white numbers, then a gray volume-space line with
white figures. With no marks it reads “No files selected”; marked files show
their cached byte total and count. Selected folders are counted separately and
never sized recursively. Unknown file lengths show `+ ?`. Only visible marks
contribute; the filter separator continues to report hidden marks. Quick search
temporarily uses the selection row.

The last line shows space available to the current user and total capacity for
the filesystem containing that panel’s directory. Exact byte counts are used
when they fit, otherwise rounded binary units (KiB/MiB/GiB/TiB) are shown. This
is volume capacity, not the size of the directory. On APFS it may represent a
shared container; operating-system storage tools can account for purgeable space
differently. Snapshots refresh in the background about every five seconds, on
directory changes and after Refresh. Rendering and clock ticks perform no volume
queries or directory scans. Failed queries display “Volume space unavailable”.
On Windows only, each panel’s bottom border contains assigned drive letters,
with the current drive highlighted. Click a letter to open its root, or click
`*` for the full drive picker. Alt+F1 selects a drive for the left panel and
Alt+F2 for the right; choose with the arrows/Tab and Enter, or type a letter.
Escape cancels. Assigned letters refresh about every two seconds; disconnected
or unready drives report a directory error and leave the previous folder open.
UNC paths remain supported and do not highlight a drive letter. This strip and
these shortcuts are absent on macOS and Linux.

## SmartPad

**Utilities → SmartPad** or **Alt+Q** opens a floating scratch editor on every
platform. In the Mac-friendly profile, **Ctrl+O**, release, then **;** is an
alternative. Existing Ctrl+O then Q quick-view behavior is unchanged.

There is one SmartPad per session. Calling it again reuses the same document,
including text, undo history and cursor position. Alt+Q from SmartPad returns to
the document that called it; if that document has closed, it returns to the file
manager. Switching away retains unsaved notes without a confirmation.

SmartPad uses the normal editor: Unicode, search, undo, F2 / Ctrl+S save, Save As,
window movement/zoom and the Open windows list. It starts with a plain empty
buffer and uses the active panel directory as the initial Save As location.
No destination file is created until you save. Save As retains the SmartPad identity and
normal no-overwrite checks. Closing or quitting protects unsaved notes through
the existing save/discard/cancel guard. Closing the document ends that scratchpad;
the next invocation creates a new empty one.

Existing editor crash recovery also checkpoints hidden SmartPad drafts. Recovered
notes open as ordinary editor documents. There is no separate notes file,
automatic startup restore or new configuration preference. Blocking dialogs,
file pickers, confirmations and exit review retain focus and prevent invocation.

## Music Player

Enable **Music Player** in **Options → Configuration → Plugins** first; it is
experimental and disabled by default. Audio playback currently requires macOS.
Linux/Windows retain the window and queue UI with audio controls disabled.

Supported files: **MP3**, native **FLAC**, **WAV/WAVE** (PCM or floating-point),
and **Ogg Vorbis** (`.ogg` / `.oga`). Headers identify the actual media format;
extensions determine which files Enter and folder imports consider. Opus,
AAC/M4A, Ogg FLAC, and chained/multiplexed Ogg streams are outside this support.
No FFmpeg or external codec installation is needed. Utilities → Music Player
uses the new menu mnemonic S; the old Alt+U, 3 sequence remains an alias.
The saved plugin ID and settings section stay `mp3_player`, so existing enable,
opening, volume and seek preferences continue to work.


On macOS, enable **Play music files in internal player** directly below the plugin
checkbox to use Enter/double-click for internal playback. It is off by default
and unavailable while the plugin is disabled. It overrides custom associations for supported music files;
Ctrl+Enter still opens the OS default app. Only the current regular supported music file is
used, regardless of marks. The player retains its queue, reuses an entry with the
same native path, and starts it immediately. New files are imported first without
interrupting current audio. A busy import/full queue reports a message. Archive
members and symbolic links keep their existing opening behavior.

Mark music files or folders in a file panel and choose **Utilities → Add music to player**.
Without marks, the command uses the file under the cursor. Files are appended in
panel order; extension matching is case-insensitive. Folders are scanned recursively in name order;
symbolic links and other extensions are skipped. A progress page offers Cancel,
which discards the whole batch without changing the queue. Imports also cancel
when the player closes. Music and background tasks keep running during scanning.
**Open** in the expanded player and **List → Add** use the same panel selection.
Adding does not start playback or interrupt the current track. The queue allows
64 entries, including duplicates. Scanning stops at the queue capacity, 10,000
directory entries or 64 directory levels, and reports limits/unreadable entries.
Valid files from a partial scan are added together on completion.

**Play** starts the selected track or resumes a pause. **Pause** suspends it;
**Stop** resets its position. The outer arrows select the previous/next track
without wrapping by default; the inner arrows use the saved seek step (initially five seconds). Changing tracks while
playing continues playback; otherwise it selects the track without starting it.
**Volume** opens a 0–100% dialog, initially 50%, affecting only DN audio. OK applies
the new value; Cancel discards it. By default, tracks play in order and stop after the last.
A file/device error stops playback and appears in the expanded window; the queue
is retained. There are no global media keys.

**Mode** opens Shuffle and Repeat settings. OK applies them; Cancel discards the
draft. Repeat Off stops at the end; Repeat One restarts the current track when it
ends; Repeat All loops the queue. Manual Previous/Next bypass Repeat One and wrap
only with Repeat All. Shuffle keeps the visible queue unchanged, visits each entry
once and lets Previous retrace the same order. Repeat All repeats that shuffled
cycle. Changing the queue or toggling Shuffle rebuilds the cycle starting with the
current track. Mode settings reset when the player reopens.

Artist/title from ID3, Vorbis comments (FLAC/Ogg), or RIFF INFO/ID3 (WAV) replace filenames in the queue and player when available.
**Info** shows title, artist, album and the native file path. Tags are read in the
background without modifying source files. ID3v1 and ID3v2.2/2.3/2.4 are supported,
including UTF-8 and UTF-16 text. Missing, malformed, unsupported or oversized tags
fall back to filenames; missing individual fields may fall back to ID3v1.

**Set up** edits the seek step (1/5/10/30 seconds) and default volume (0–100%).
OK applies both, including the current volume; Cancel/Esc discards edits. The
**Volume** dialog changes only the current session volume. **Time** toggles elapsed
and remaining track time; unknown remaining time shows `--:--`.

FLAC duration comes from STREAMINFO sample counts, WAV from frame counts, and
Ogg from the final granule position after validating page boundaries and CRCs.
Text metadata is bounded; artwork is skipped and source files are never modified.

**List** opens a **Play order** draft with numbered descriptions and track durations.
Duration is read in the background from format metadata without an audio device;
unsupported/truncated streams show `--:--`. Long durations retain all minute digits. Delete removes a draft row; OK commits; Cancel/Esc discards changes.
When stopped, OK also selects the highlighted row for Play. Removing the playing
track and accepting the draft stops it. Save remains unavailable.
Playlists are not saved to disk.

Music continues while other windows or dialogs are active. Closing the player,
disabling the plugin or quitting DN stops playback. Reopening starts an empty
queue with the saved default volume and elapsed timing. The internal-opening
checkbox, seek step and default volume follow normal DN configuration saving
(including Save desktop and save-on-exit); playlist and playback modes are not saved.

Use Alt+Z, the title zoom button, a title double-click or Alt+0 → Zoom to switch
between compact and expanded views. Tab/Shift+Tab choose enabled controls,
Enter/Space activate them, F1 opens help and Esc closes the current page/player.
Ctrl+F5 or title dragging moves it. Tile/Cascade retain the fixed window sizes.
Opening an existing player activates its single window.

## Tetris / Pentix and high scores

The game uses a classic DN-style board with Info, Next and Best panels and
New / Setup / Top 10 / Pause buttons. Tab/Shift+Tab cycle through the board and
these buttons; Enter activates the selected button. Gravity pauses while button
focus is active. Esc returns button focus to the board; from the board Esc returns
to files. Clicking the board also restores play focus. Top 10 shows Name, Start, End and Score with dotted leaders; the highlighted
entry's line count remains below the table. Pending scores keep a Retry save
button; Tab/Shift+Tab choose Retry save or OK, and Enter activates that choice.
Esc, OK or the title close control returns to the game.

Setup provides ten named levels (1–9 and 0 for level 10), a Classic Tetris /
Pentix radio group and Piece preview. Tab/Shift+Tab visit Level → Game style →
Preview → OK → Cancel. Arrows select within the focused radio group; Space toggles
the style or preview. Enter activates the focused button, otherwise the default
OK; Esc cancels. Setup initially shows the current level. OK starts a new game and
saves these choices; Cancel preserves the existing game and preferences.

Classic uses seven four-cell figures. Pentix uses all 27 original DN figures,
from one to five cells, with its own Top Ten. Both retain the 12×19 well, original
rotation bounds, timing and scoring. Down/Space drops to the bottom and restarts
the gravity timer: the next tick locks the piece. Horizontal adjustment and valid
rotation remain possible until then. No wall kicks, seven-bag or all-clear bonus
are added. The Next well is black and supports five-cell pieces.

Alt+F9 opens the game. Game over shows a compact score message. Enter/OK asks for
a name only when the score qualifies for the active mode's Top Ten; otherwise it
returns to the completed board. Esc dismisses game-over without submitting.
Type or paste a Unicode name (up to 30 graphemes / 240 UTF-8 bytes). Tab/Shift+Tab
visit Input → OK → Cancel → Help → Skip name. Enter activates the selected button;
from the input it saves. Cancel/Esc returns to game-over without recording the
score. Skip name explicitly records Anonymous; blank names also use Anonymous.
Accepted results open the active mode's Top Ten with the new entry highlighted.
F1/Help preserves its parent page, draft and focus, including the selected Setup
mode. F4 shows Top Ten during play. Closing an unfinished game adds no result.

`tetris.conf` beside `config.conf` stores the two leaderboards, last player name
and game preferences, independently of preference auto-save. Versions 1 and 2
remain readable as Classic Tetris. Writes use version 3: existing `score` rows
remain Classic; `pentix_score` rows use the same points,lines,start,end,name
fields. Each table holds at most ten entries. `mode=tetris|pentix`,
`start_level=1..10` and `preview=true|false` remember Setup. Defaults are Classic,
level 1 and preview on. Imported line counts use `-`; `dn_tet_encoding` remembers
the import encoding. Names may contain commas; control characters are rejected.
Older builds that do not support v3 refuse this format rather than rewriting it.

Accepted scores save immediately, even if preference auto-save is disabled.
Writes use a private staging file and lock, merging the latest disk scores before
keeping the top ten; equal scores retain existing order. Read-only, malformed,
unsupported and symlinked files are not silently replaced. A failure leaves the
score or settings draft in memory and shows an error with Retry save in Top Ten.
The play window points to F4 when a write is pending. Unsaved results remain available
when closing/reopening the game during the session; retry after fixing the error.
An explicitly abandoned name prompt or exiting the app before acceptance does not
commit that result.

To import original scores, copy `dn.tet` or `DN.TET` beside `config.conf`, then
open/reopen Tetris. The original countdown-XOR binary format is read directly:
20 short-string/level/signed-score records: ten Classic and ten Pentix slots
merge into their respective tables. Packed 740-byte and word-aligned
760-byte tables are supported. The source file is never written, renamed or deleted.
If both filenames exist on a case-sensitive filesystem, lowercase takes priority.

Imported scores merge with the existing top ten, preserving ties and duplicate
slots while avoiding repeated imports on subsequent opens. The modern file is
saved through its normal lock/staging mechanism, independently of preference
auto-save. Corrupt original files report an error; existing modern scores remain
visible. An invalid modern score file is never replaced by an import.

The original has no encoding label or line count. Choose the correct interface
language before the first import: English uses DOS CP437; Russian uses CP866.
The encoding choice is then remembered in `tetris.conf`, so changing the interface
language cannot create duplicate entries. Original line counts display “—”.
New scores still support Unicode names, full line counts and modern integer limits.


## Grouped settings and DOS preference import

`config.conf` is UTF-8 TOML, with `config_version = 2` at the root. Sections are
`application`, `interface`, `file_manager`, `panels.left`, `panels.right`, `editor`,
`regional`, `screensaver` and `bookmarks`. For example:

```toml
config_version = 2

[application]
save_on_exit = true
shortcuts = "classic_dn"

[interface]
palette = "classic"
show_clock = true

[panels.left]
layout = "brief"
sort = "name"
columns = "name"

[editor]
auto_indent = true
syntax = true
expand_tabs = true
tab_size = 8

[regional]
language = "en"
date_order = "ymd"
date_separator = "-"
time_format = 24
time_separator = ":"

[screensaver]
minutes = 10
selected = ["star_flight"]
```

Omitted fields use current defaults. Known fields require the right TOML type
and a supported value; invalid settings trigger recovery of the whole file rather
than partial application. Unknown TOML values are kept on save, although formatting/comments may
be regenerated. Bookmark slots `1` through `9` retain the existing lossless native
path encoding; set them through Quick directories rather than typing DOS paths.
Settings and other configuration filenames/locations have not changed.

Only TOML with `config_version = 2` is supported. Invalid, oversized, older flat, empty,
unversioned or unsupported-version configurations are renamed to `_old_config.conf`
and defaults load. Existing recovery files are never overwritten: collisions use
`_old_1_config.conf`, `_old_2_config.conf`, and so on. Recovery uses the settings
lock; read-only files, symlinks, inaccessible files or failed renames report errors
and are not discarded. Recovery does not reopen the first-run welcome. There is no
format migration or `config.txt` fallback. Missing files use defaults;
Save desktop or normal saving on exit creates a new TOML configuration.
Locks, external-edit checks, read-only/symlink protections and the 1 MiB bound
continue to apply. Load desktop reloads the file; it does not import DOS settings.

Choose **Options → Import DOS settings…**, enter the original `DN.CFG` path, then
review **current → imported** values and skipped records. The reader runs in the
background; close/cancel discards pending replies. Arrows, PageUp/PageDown,
Home/End and the wheel scroll the preview; Left/Right pan long lines. Tab chooses
Apply or Cancel. The title close control also cancels. Apply is unavailable for
invalid files or when there are no changes.

Supported imports are clock visibility, hidden-file visibility, Name/Extension/
Size/Date sorting for both panels, file highlighting, selection markers, Space
marking, Delete-key handling, editor auto-indent, tab width 1–16 and screensaver
delay. Original delay selectors 0/1/2/3/4 map to disabled/1/2/5/10 minutes.
Unfamiliar record layouts, unsupported modes and invalid tab widths are reported
as skipped. Truncated data, duplicate supported blocks, invalid termination and
unsupported signatures are rejected. Input is limited to 1 MiB and must be a
regular file, not a symlink.

Apply changes supported preferences in memory. Save desktop or normal saving on
exit persists them. The original DOS file is never written, renamed or deleted.
Trash mode, confirmations, shortcuts, language/date formatting, palette and other
preferences stay as they were. DOS drive paths, `.DSK` windows, hardware options,
commands, macros and custom palette files are not imported. User menus,
associations and Tetris score files retain their separate formats. Support is
currently verified against fixtures derived from the original format; real-world
`DN.CFG` compatibility remains provisional until tested with a supplied file.

## Opening and saving from the editor

The editor's **Open** and **Save As** commands use Turbo Vision's file dialog.
**Save** also opens this dialog for an untitled document. Type an absolute or
relative path in **Name**, or browse the **Files** list. Enter on a directory
opens it; `..` moves to its parent. A wildcard such as `*.txt` filters the list.
The **Show hidden** checkbox controls hidden entries for this dialog.

Tab and Shift+Tab move between the native controls. Arrows and PageUp/PageDown
navigate the list; Enter activates the selected item or button. Esc or Cancel
closes the dialog. F1 opens editor help and returns to the same dialog when
dismissed. Keyboard mnemonics and mouse controls are available in both languages.
Unicode filenames and pasted paths are supported.

Open keeps the current document and activates a separate editor, or reuses the
editor already holding that file. Save As requires a new destination: existing
files and destinations owned or reserved by another editor are refused. An error
leaves the document's changes intact.

## Switching open windows

**Manager → Windows…** or **Alt+0** opens the window list from the file panels,
editor or viewer. The editor also has **File → Windows…**. In the Mac-friendly
profile, press **Ctrl+O**, release, then **U**.

Choose a row with the arrows, Home/End or PageUp/PageDown, then Enter or Switch.
A double-click also switches; Esc, Cancel or the title close control leaves the
current window active. A `*` marks an editor with unsaved changes.

Switching keeps every editor and viewer open, including its cursor, scroll
position, search and undo history. Panel paths, marks, filters and the command
draft remain in place. F4 opens the highlighted file; opening a different file
from the editor's Open dialog creates another document. Reopening the same file
activates its existing editor or viewer. Explicit text/hex viewer requests reload
that viewer in the requested mode.

The manager appears first, followed by documents in creation order. New untitled
editors have distinct numbered labels. Closing a document affects only that
window and uses the normal unsaved-change guard. Save As cannot claim a file
already open or being saved in another editor.

Exiting reviews all unsaved editors, starting with the most recently used.
Cancel keeps all documents, including buffers already approved for discard;
completed saves remain saved. Buffers also survive a settings-save failure.
Dialogs block switching until closed, and nested help returns to its parent.

Editors, viewers and the single game window share a floating desktop over the
fixed file panels. Editors and viewers open maximized. Drag their title to restore
and move them; drag the bottom-right corner to resize. Double-click the title,
click its zoom control, or press **Alt+Z** to maximize or restore. The window list
also offers **Size/Move** and **Zoom** when the selected window supports them.

**Ctrl+F5** starts Size/Move for the active document or top dialog. Arrows move one
cell; Shift+arrows resize supported windows; Ctrl+Left/Right move eight columns.
Home/End align left/right, PgUp/PgDn align top/bottom, and Tab/Shift+Tab move
horizontally. Enter accepts; Esc restores the starting geometry. While dragging
or using Size/Move, the captured frame and title turn green; releasing the mouse,
accepting or cancelling restores their normal colors. Editing and paste
are suspended during movement. Ctrl+F5 still sorts by date in the file manager.

Clicking an exposed background document raises it; its first client-area click
only activates it. Clicking exposed file panels selects the manager while leaving
floating windows visible. Dialogs block the background, and new children center
over their parent. Help can be resized; ordinary forms and game pages keep their
sizes. Tetris pauses during movement and while another window has focus.

Geometry lasts until a window closes. Terminal resizing clamps windows while
retaining their preferred floating geometry for a later expansion. Open documents
and window positions are not restored after restarting. File panes remain fixed.

**Manager → Tile / Cascade**, also available in **Alt+0 → Open windows**, arrange
all document windows (editors, viewers and Tetris). Tile divides the desktop into a
balanced grid in creation order, adjusting cells for each document's minimum size.
If a balanced grid cannot fit the game alongside other documents, Tile tries a
separate strip for the fixed-size game before refusing the arrangement.
Cascade follows the current stacking order with diagonally offset title bars and
a shared bottom-right corner, like DOS Navigator. Both preserve the active
document, stacking order, edits, undo history and scroll state. Arranged bounds
become each resizable window's restore size for Alt+Z. Tetris participates at its
fixed 54×23 size: Tile places it at its cell's top-left, and Cascade limits its
offset to keep the entire board on screen. Dialogs and file panels are excluded.
Close blocking dialogs or finish Size/Move first. If the documents
cannot fit their minimum sizes, a message appears and every document keeps its
previous geometry. Resize the terminal or close some documents before retrying;
terminal resizing never automatically retiles the desktop.

## Context help

F1 opens help for the current screen or dialog. Press F1 again for Contents.
Tab and Shift+Tab select links; Enter follows the selected link. A single mouse
click selects a link and a double click follows it. Alt+F1 or Backspace returns
to the previous topic with its scroll position. Arrows, PageUp/PageDown and the
mouse wheel scroll. Esc or the title close control returns to the parent dialog
or document. Help blocks document switching until closed.

## Hiding the menu bar

Options → Configuration → Interface → **Hide menu bar** gives the top row to the
file panes. Enter applies the setting; Esc cancels the draft. It is saved with the
other interface preferences. F9, Alt plus a menu letter, or a click on the top edge
reveals the menu temporarily. Esc or choosing a command hides it again without
moving the panes. Uncheck the option to keep the bar visible.

The manager menu is hidden while Tetris is open. The game's buttons use DOS
Navigator's dark faces, yellow shortcut letters and offset black shadows.

Dialog buttons share DOS Navigator's dark faces and black offset shadows.
Yellow captions with cyan inward arrows identify the focused/default action;
disabled buttons have dim captions. In Colors, OK is the Enter default while
browsing or editing attributes. Tab to Cancel or press Esc to restore the previous
color table.

## Related selection and block commands

Panel → Selection → **Select/Unselect same highlight type** matches the first enabled filename
group, falling back to the built-in file categories. Distinct groups stay distinct
when they use the same color. **Select/Unselect same extension** ignores extension
case, excludes directories/links and groups extensionless files together. Both
operate on visible entries and preserve marks outside the current filter.
Ctrl+Plus/Minus selects/unselects the same type; Ctrl+Shift+Plus/Minus uses the
extension, when the terminal reports these modifiers distinctly. Menu actions
are available in every terminal. Ordinary Plus/Minus still open wildcard prompts.

Editor → **Block** provides Indent/Unindent, Uppercase/Lowercase/Capitalize words
and Sort selected lines. Tab indents selected lines; Shift+Tab unindents selected
lines or the current line. With no selection, Tab retains ordinary tab insertion.
Indentation follows the current tab width and Expand tabs setting. Case conversion
supports Unicode, including characters whose converted form changes length.
Sorting uses stable, case-sensitive Unicode order on whole selected lines,
retaining each position's original line ending and the final newline state.
Ambiguous blank-line moves that would fuse separate CR/LF endings are refused
without changing the buffer.
A selection ending at the next line's start excludes that next line. Every change
is a single undoable edit; none saves the file automatically.

Editor → **Misc → Matching bracket** (Ctrl+]) jumps between nested `()`, `[]` and
`{}` pairs at or immediately before the cursor. Supported language lexers exclude
quoted strings/comments, independently of syntax-color visibility. Plain text
matches structurally. Mismatched nesting leaves the cursor unchanged.

Viewer **Ctrl+G / Alt+G** (Mac prefix: Ctrl+O, then G), or its **Go to** function-bar
button, opens an owned navigation dialog. Text mode accepts 1-based line numbers;
Hex mode accepts 0-based decimal byte offsets or `0x` hexadecimal addresses and
highlights the destination byte. Addresses must lie within the loaded preview;
the command does not read beyond its bounds. Invalid input retains the draft and
position. Enter applies, Esc cancels, and Tab/Shift+Tab traverse field/buttons.

## Editor crash recovery and save outcomes

DN checkpoints dirty editors, including untitled and inactive documents, in a
private `editor-recovery` directory beside `config.conf`. It attempts checkpoints
at roughly five-second intervals while the event loop runs; slow I/O and queued
work may delay completion. A checkpoint is also requested before shell handoff.
Recovery restores the **last completed checkpoint**, so edits since that point
may be lost after a crash. It does not automatically save over your source files.

On startup, abandoned sessions offer **Recover / Inspect / Discard / Later**:

- Recover opens a separate, dirty untitled document, retaining the text, UTF-8 BOM
  preference and checkpoint cursor/selection. Save As uses the existing
  no-overwrite safeguards. Undo history is not restored.
- Inspect opens a bounded, read-only preview using owned Help, then returns to
  the same recovery dialog and focused button.
- Discard removes that checkpoint. Later/Esc leaves it on disk; use
  **File → Recover unsaved documents…** to return to deferred entries.
- Corrupt or unsupported checkpoints cannot be recovered or inspected. They stay
  on disk until explicitly discarded. Partial staging files are not offered.

If **File → Recover unsaved documents…** finds no waiting checkpoints, it shows
one **No files to recover** dialog. Acknowledging it closes the message. Empty
startup scans do not show this dialog automatically.

Session leases exclude live DN instances and prevent two instances from claiming
one abandoned session concurrently. Checkpoint files are versioned, checksummed,
bounded, synchronized and published by sibling-file replacement. On macOS/Unix,
recovery directories require owner-only access and records use mode 0600;
symlinks, hard-linked records and unsafe directories are refused. Native source
paths are encoded losslessly and are descriptive metadata, never restore targets.

Recovered data remains in its original checkpoint until a replacement checkpoint
is completed, or the document is successfully saved or explicitly discarded.
Dirty buffers and their checkpoints survive cancelled quit reviews and settings
save failures. Final cleanup happens after the existing quit and settings checks;
if cleanup fails, DN retains the remaining data and asks you to acknowledge the
notice before finishing exit. No recovery files are removed merely because the
process terminates or a checkpoint update fails.

Saving now distinguishes three outcomes:

- **Not published:** the attempt did not replace the destination; the buffer stays
  dirty and current file/conflict safeguards still apply.
- **Saved:** file synchronization, publication and directory synchronization
  completed; the editor may mark the revision saved and retire its checkpoint.
- **Written, durability unconfirmed:** publication succeeded but directory
  synchronization failed. DN updates the file identity, retains dirty/recovery
  state and cancels pending close/quit. Save retries confirmation without replacing
  an unchanged published revision. If you edit again, the normal checked save
  path applies. External changes are still checked before retrying.

Atomic replacement and conflict checks do not constitute an atomic compare-and-
replace against an uncooperative external writer. Keep the file size/encoding
limits and external-change safeguards in mind when editing concurrently elsewhere.

## Help window controls

Help windows use the shared desktop **Expand/Restore** title button. The button,
Alt+Z and title double-click use the same window geometry, preserving the current
Help topic, scroll position and back history. Expanded Help stays within the
workspace when the terminal shrinks and restores its remembered floating bounds.
Welcome uses the same Expand/Restore and resize controls, preserving its selected
action and returning to its remembered bounds.

## Temp Drive

`Utilities → Temp Drive` opens DN's persistent storage directory in the active
panel. These are real files, not references to originals. Use F5 from the other
panel, or `File → Copy to Temp Drive…` to open the standard Copy dialog for marked
items (the current item when nothing is marked). Existing destinations are refused
by default; the usual queue, Skip, cancellation and copy options apply.

The directory is `temp-drive` beside the actual configuration file: normally
`~/.config/dn/temp-drive` on macOS/Linux (honouring `XDG_CONFIG_HOME`), or
`%APPDATA%\Directory Navigator\temp-drive` on Windows. It is created when first
opened or used for copying. Contents survive restarts even with Save on exit off;
they are never automatically cleared. No system temporary directory is used.

`Utilities → Clear Temp Drive…` asks before clearing **all** contents, including
hidden and filtered files and folders. Cancel is selected initially. The command
uses the ordinary Trash/permanent-delete preference and never deletes the Temp
Drive directory itself. It is unavailable while file operations are pending.
Completed deletions remain completed if cancelled or interrupted; remaining items
are retained. Root or source replacement after confirmation is rejected.

Inside Temp Drive all ordinary panel operations apply. F8 acts on real stored
files, and F6 can move them elsewhere. Parent navigation leaves the directory
normally. A file or link substituted for the Temp Drive root is rejected.
