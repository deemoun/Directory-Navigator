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

Enter or double-click browses ZIP/TAR/TAR.GZ/TGZ archives inside DN; other files
open in the operating system's default application. Ctrl+Enter or File → Open
with default app opens an archive externally.
F3 explicitly opens DN's viewer; F4 explicitly opens DN's editor. Ordinary
directories stay inside DN. Links to files open externally, links to directories
navigate normally, and broken links report an error. On macOS, `.app` bundles
launch instead of being browsed as directories.

Opening uses macOS `/usr/bin/open`, Linux `xdg-open` (requires xdg-utils and a
desktop session), or Windows ShellExecute. Native paths are passed directly,
without shell command substitution. Launching is asynchronous; opener errors
are reported in DN without falling back to the viewer/editor. File associations
are managed by the OS. User menus have a separate explicit command runner;
there is no association editor.

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
or APPDATA/DOSNavigator/config.conf on Windows. The directory stays unchanged. If `config.conf` is absent, legacy
`config.txt` is loaded and migrated on the next save; the old file is kept as a
backup. An existing `config.conf` always takes precedence.
The UTF-8 `key=value` file has a version header, comments and lossless native
bookmark encoding. Older files remain readable; unknown extension keys are kept.
Unreadable, malformed or newer-version files report errors rather than being
replaced. Recognized legacy values retain their existing fallback defaults.
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
output recall, archive creation, custom associations and palette editing.
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

Alt+F9 opens Tetris. A finished game shows the final score, line count and
starting/ending levels. Enter/Esc continues: positive scores qualifying for the
top ten ask for a player name; other scores show the leaderboard directly.
Type or paste a Unicode name (up to 30 graphemes / 240 UTF-8 bytes). Enter/OK
saves it; Esc/Skip name records Anonymous. Blank names also use Anonymous.
The new entry is highlighted. F4 shows the top ten during play. Closing an
unfinished game does not add a score.

Names, scores, lines, start/end levels and the last player name are stored in
`tetris.conf` beside `config.conf`, not in the program’s directory. This is a
separate UTF-8 `key=value` file: `tetris_version=1`, `last_name=...`, then up to ten
`score=points,lines,start,end,name` records. Names may contain commas; control
characters are rejected. Old installations start with an empty table.

Accepted scores save immediately, even if preference auto-save is disabled.
Writes use a private staging file and lock, merging the latest disk scores before
keeping the top ten; equal scores retain existing order. Read-only, malformed,
unsupported and symlinked files are not silently replaced. A failure leaves the
score in memory and shows an error with Retry save. Unsaved scores remain available
when closing/reopening the game during the session; retry after fixing the error.
An explicitly abandoned name prompt or exiting the app before acceptance does not
commit that result. The old DOS binary `dn.tet` format is not imported.
