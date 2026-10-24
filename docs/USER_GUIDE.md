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

Directory Navigator shows a compact welcome when `config.conf` does not exist. It lists the active profile’s controls, opening behavior and
current deletion mode. **Start**, Esc or the title close control dismisses it;
**Keyboard settings** and **Help** return to the welcome when closed. **≡ → Welcome…**
opens it again without changing panel state.

Dismissing the welcome writes no files. Normal settings saving on exit prevents
it appearing on later launches. If saving is disabled and no configuration exists,
it appears next time. Existing, invalid or inaccessible `.conf` settings do not
trigger it; settings errors are reported separately. Load desktop never triggers it.
There is no startup delay or splash timer. CLI help/version bypass the interface.

## Keyboard profiles

**Options → Configuration → Keyboard shortcuts** selects **Automatic**, **Classic
DN** or **Mac-friendly**. Apply changes the profile immediately; Cancel preserves
it. Save/Load desktop and normal settings persistence include `[application]` with `shortcuts = "auto"`,
`shortcuts = "classic_dn"` or `shortcuts = "mac"` in `config.conf`. Missing values
use Classic DN on every platform, including macOS. Explicitly saved choices are
preserved. Automatic remains available and selects Mac-friendly on macOS, Classic
DN elsewhere. Select Mac-friendly explicitly when preferred, including over SSH.

The Mac profile uses **Ctrl+O, release, then another key**. `^O` in menus and
function bars means this prefix. A context guide appears while waiting; Esc,
Ctrl+O again, or a mouse click dismisses it without editing text. Text prompts
retain their normal input behavior. Existing DN and modern Ctrl aliases remain
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

File → Find searches filename patterns beneath the active directory, with
subdirectory and hidden-entry options. Alt+F7 remains cached quick search.
Results arrive in the background; Stop retains partial results and Close cancels.
Enter/double-click jumps to the containing folder and highlights the result;
F3/F4 view/edit regular files. Jumping clears that panel's filename filter and
reveals hidden results when necessary. Patterns are case-sensitive Unicode `*`/`?`.
Results stop at 10,000; skipped paths and the first error are reported. Traversal
does not follow directory links and reports nesting deeper than 512 as skipped.

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
name; multiple items require an existing destination directory. Existing files,
folders and broken links are refused; nothing is silently overwritten or merged.

Operations run in a background worker with item/byte progress. Cancellation and
errors retain completed work. Moves prefer native no-overwrite rename, with a
copy fallback when needed. Symlinks are copied as links and are not recursively
traversed. Copy metadata support varies by platform.

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

Classic DN is the default fixed 256-color palette, with gray panels and red/gray
function bars. Options → Colors previews Classic DN or the legacy terminal palette;
Cancel restores the previous choice. Interface → Show clock controls the local
upper-right clock. Save desktop persists these choices. No drive selector is added.

Screen savers: Options → Configuration → Screen savers offers DN’s Star Sky,
Preview, and Never / 1 / 2 / 5 / 10 / 30 / 60 minute idle delays (default: 10 minutes).
Save desktop persists the delay. The ≡ menu’s Screensaver starts it immediately.
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
`{other_dir}`, `{selected}`, `{shell}`. `{selected}` expands to visible marked
entries in listing order, falling back to the highlighted entry; `..` is excluded.
Filtered-out marks alone produce an error. Native filename bytes remain intact.
Expansion is bounded to 2,048 arguments/64 KiB per command and 256 KiB per action.

Every action shows its working directory, panel context, executable and arguments
before Run. Commands run sequentially in the active directory, stopping on failure.
DN restores the normal terminal for interactive programs, then waits for Enter
before returning and refreshing the panels. On macOS/Linux, Ctrl+C interrupts the
command and returns to that prompt. Windows command interruption is unverified.

There is no implicit shell or DOS batch expansion. For pipelines/built-ins/scripts,
invoke a shell explicitly, e.g. `/bin/sh -c 'printf "%s\n" "$DN_FILE"'`.
`DN_FILE`, `DN_DIR`, `DN_OTHER_FILE` and `DN_OTHER_DIR` are native environment
values; quote their expansions. Windows has a small `cmd` starter menu. Original
DOS `!`, `$`, `%1`…`%9`, parameter prompts and batch flags are not translated.


Archive browsing is read-only. Enter/double-click a ZIP, TAR, TAR.GZ or TGZ,
or choose File → Browse archive. Enter opens folders or previews regular members;
Backspace/Left returns to the parent and Esc closes the archive. F3 opens the
bounded text/hex viewer, with its existing wrap/search controls; closing that
viewer returns to the archive. F9 shows member metadata, Ctrl+R refreshes,
Stop cancels background indexing/preview work, and the mouse supports rows,
double-click, wheel, scrollbar arrows/track, buttons and the title close control.
The underlying panel paths, cursor, marks, filters and layouts remain intact.

Members are previewed in memory, never unpacked to disk or added to file history.
Links/hard links and special entries are listed but never followed or executed.
Encrypted entries can be listed but cannot be previewed. Corruption, unavailable
sources and source changes are reported; partial TAR listings are labelled.
Archive sources must be regular files; file symlinks retain OS-default opening.

Limits: 512 MiB archive source; 10,000 members; 20,000 virtual nodes; 16 MiB tree
metadata; 64 path levels/4 KiB paths. TAR traversal/decompression is limited to
256 MiB and each worker read job to ten seconds between I/O checks. ZIP indexing
uses bounded central-directory validation and a 64 MiB cumulative read budget.
Preview limits remain 2 MiB/100,000 lines. Small complete ZIP previews check CRC;
truncated previews do not verify the unseen remainder. ZIP supports Stored and
Deflate; ZIP64, multi-volume/self-extracting layouts, duplicate ZIP names,
passwords, extraction, archive modification and nested archive browsing are
outside this batch. ZIP timestamps are shown as stored; TAR timestamps use UTC.
The native codecs use `zip`, `tar` and `flate2` with minimal features.


Menu scope: ≡, File, Utilities, Panel, Manager and Options. Directory tree lives
under Manager; Tab/Shift+Tab switch panels. Historical aliases such as Panel Setup and
Options → File Manager → Setup remain. The editor keeps its original headings;
Block (Alt+B) offers selection, cut, copy and paste. Unassigned editor function
keys have no action labels or mouse targets. Paragraph alone remains
intentionally dimmed, with no dropdown. Editor preferences remain under Options.
Confirmation policy explains safeguards; it does not offer switches to disable
them. Save/Load desktop continues to persist the selected palette and settings.

Unsupported commands are not menu placeholders. Disk administration/recovery,
DOS communications/country/printer setup and built-in printing are outside this
app's scope. Save all and desktop window-management commands await a matching
multi-document/window model. Future work is separate: safe archive extraction,
disk/free-space information and permission editing; later,
output recall, archive creation and palette editing.
Split/combine and line drawing remain low priority. No settings format changes.

The bottom row is a command prompt with a blinking cursor, not a status line.
Type a single-line command and press Enter to run it in the active directory,
using `$SHELL -c` (or `%COMSPEC% /D /S /C` on Windows). Commands run with your
normal permissions in the real terminal, without file-operation safeguards.
DN waits for Enter after completion, restores the panels and refreshes listings.
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
No drive selector or DOS drive-letter controls are added.

## Tetris high scores

The game uses a classic DN-style board with Info, Next and Best panels and
New / Setup / Top 10 / Pause buttons. The title close control and Esc return to
files. Top 10 shows Name, Start, End and Score with dotted leaders; the highlighted
entry's line count remains below the table. Pending scores keep a Retry save
button, and OK or the title close control returns to the game.

Setup provides ten named levels (1–9 and 0 for level 10) and a Piece preview
checkbox. Arrows select levels; Tab/Shift+Tab move between levels, preview, OK
and Cancel. Space toggles preview, Enter applies (or cancels on Cancel), and Esc
cancels. Applying starts a new game; cancellation preserves the current board,
score, level, preview and paused state. Classic Tetris is the only implemented
style; Pentix is not offered.

Alt+F9 opens Tetris. A finished game shows the final score, line count and
starting/ending levels. Enter/Esc continues: positive scores qualifying for the
top ten ask for a player name; other scores show the leaderboard directly.
Type or paste a Unicode name (up to 30 graphemes / 240 UTF-8 bytes). Enter/OK
saves it; Esc/Skip name records Anonymous. Blank names also use Anonymous.
The new entry is highlighted. F4 shows the top ten during play. Closing an
unfinished game does not add a score.

Names, scores, lines, start/end levels and the last player name are stored in
`tetris.conf` beside `config.conf`, not in the program’s directory. This is a
separate UTF-8 `key=value` file. Existing version 1 files remain readable;
version 2 supports imported records with unknown line counts (`-`) and a remembered
`dn_tet_encoding=cp437` or `cp866`. Both contain `last_name=...` and up to ten
`score=points,lines,start,end,name` records. Names may contain commas; control
characters are rejected.

Accepted scores save immediately, even if preference auto-save is disabled.
Writes use a private staging file and lock, merging the latest disk scores before
keeping the top ten; equal scores retain existing order. Read-only, malformed,
unsupported and symlinked files are not silently replaced. A failure leaves the
score in memory and shows an error with Retry save. Unsaved scores remain available
when closing/reopening the game during the session; retry after fixing the error.
An explicitly abandoned name prompt or exiting the app before acceptance does not
commit that result.

To import original scores, copy `dn.tet` or `DN.TET` beside `config.conf`, then
open/reopen Tetris. The original countdown-XOR binary format is read directly:
20 short-string/level/signed-score records, of which only the ten Tetris slots
are imported; the Pentix table is left alone. Packed 740-byte and word-aligned
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

## Switching open windows

**Manager → Windows…** or **Alt+0** opens the window list from the file panels,
editor or viewer. The editor also has **File → Windows…**. In the Mac-friendly
profile, press **Ctrl+O**, release, then **U**.

Choose a row with the arrows, Home/End or PageUp/PageDown, then Enter or Switch.
A double-click also switches; Esc, Cancel or the title close control leaves the
current window active. A `*` marks an editor with unsaved changes.

Switching keeps documents open, including their cursor, scroll position and undo
history. Panel paths, marks, filters and the command draft remain in place. F4
from the panels resumes the existing editor. Close still uses the normal
unsaved-change guard; exiting the application also checks a background editor.

This first step supports one editor and one viewer alongside the file manager.
Games and modal dialogs are not switchable. Windows remain session-only.
