# Directory Navigator — beta

## Architecture
- Native Rust `dn`: Ratatui/Crossterm/Chrono, Unicode segmentation, Unix libc;
  macOS native Trash via `trash`, Windows native shell APIs; minimal ZIP/TAR/gzip
  codec features via zip/tar/flate2.
- Modular app/input/panels, operations, dialogs, configuration, editor, viewer
  and utilities. Shared focus policy and explicit viewport preparation.
- Cancellable panel scanners; bounded short-task pool; dedicated file-operation
  and traversal workers. Native filesystem boundary and owned sibling staging.
- Shared iterative traversal sends bounded Find/size updates. Dedicated discovery
  state owns session dialogs/options/results; quick view owns debounced reads.

## Implemented
- Release preparation: legacy Pascal files/resources moved intact to ignored
  original-dn/; Rust src/, tests/ and root Cargo commands remain unchanged.
  MIT license credits Dmitry Yarygin; original notices are confined to the local,
  ignored original-dn/ reference folder. README links to docs/USER_GUIDE.md.
  Build/binary/OS artifacts are ignored; Cargo excludes legacy/release artifacts.

- Dual Brief/Details panels, DN palette/borders/menus/clock/function bars, compact
  dialogs/title close controls, classic scrollbars, keyboard and mouse controls.
- Selection/wildcards, filters, sort modes, bookmarks, histories, file info,
  comparison and list export. Quiet periodic refresh, slash-free folder names,
  preservation of surviving marks/cursor and removed-folder recovery.
- Safe copy/move/mkdir/delete: exclusive destinations, native same-volume moves,
  staging, metadata preservation where supported, no recursive symlink following.
  Confirmed Trash is the default; File Manager Setup changes deletion mode and
  Save/Load desktop persists it. Trash failure never causes permanent deletion.
- Enter/double-click browse supported regular archives; other files use OS default
  applications (Ctrl+Enter/File menu always offers external file opening). Directories navigate internally,
  macOS .app bundles launch. F3/F4 retain the internal viewer/editor.
- Bounded text/hex viewer with search/wrapping; UTF-8/grapheme editor with original
  menus/keys, guarded saves/open/close, incremental indexing, undo, find/replace,
  indentation and syntax colors. Internal clipboard and saved preferences.
- Calculator, ASCII/CP437, native system/memory information, Tetris and Star Sky.
- Ctrl+F7/File Find: case-sensitive Unicode patterns beneath the active directory,
  recursive/hidden options, files/folders/links, incremental results capped at
  10,000, Stop, skipped count/first error, Go To and regular-file F3/F4 actions.
  Alt+F7 remains quick search. Go To clears the destination filter and reveals
  hidden results. Native paths remain intact; directory links are not traversed.
- Panel → Calculate directory sizes: visible marked/highlighted directories;
  apparent bytes and file/dir/link/skipped counts in a snapshot dialog. Hard
  links count separately; links contribute their own lengths without following
  targets. Stop retains partial results; Recalculate starts a fresh snapshot.
- Ctrl+Q/Manager Quick view: inactive-panel overlay, 150 ms debounce, stale-read
  cancellation, text/hex preview, wheel scrolling, directory/link information.
  Close/Ctrl+Q/Tab restores the underlying panel; F3 opens the ordinary viewer.
  Preview reads reject final symlinks and do not enter viewer history.
  New tools are session-only; this batch added no dependencies or config keys.
- Ctrl+T / Manager → Directory tree: DN-style fold/location browser,
  native volume root and canonical initial location, one-level background reads,
  stable highlight position during loading, cached collapse/re-expansion,
  branch Refresh, local hidden toggle, arrows/pages/buttons/mouse/scrollbar.
  Enter opens in the originating panel; Cancel preserves both panels. Directory
  links are leaves; failed navigation retains the panel. No dependencies/config
  keys added. Reference: original TREE.PAS/English resources and RDN dirtree.rs.

- F2 / Utilities User menu: original >level nesting, ~hotkeys~, separators,
  local/ancestor/global definitions, F2 switch, F4 edit, reload after editing,
  Options definition entries. Shift+F2 now cycles sorting; document keys retain
  precedence. Starter definitions are created only on explicit Edit.
- Shared foreground command runner: reviewed native argv/context, whole-argument
  placeholders/visible marks, explicit shells only, interactive terminal handoff,
  Unix Ctrl+C handling, stop-on-failure and panel refresh. Bounded parsing and
  expansion; no new dependencies or desktop configuration keys.

- Read-only ZIP/TAR/TAR.GZ/TGZ browser: asynchronous bounded indexing, virtual
  folders, native member identity, mouse/keyboard navigation, parent/close,
  refresh/Stop, metadata and in-memory text/hex previews via the ordinary viewer.
  Panels remain untouched; no member extraction or virtual viewer history.
  Links/special entries are leaves; encrypted previews are refused. Unsafe paths,
  corrupt/truncated data, duplicates and changed sources are handled explicitly.

- Clean menus retain working DN actions, editor headings and supported aliases.
  Unsupported dropdown commands and empty Archives configuration are removed;
  Confirmation policy is explicitly read-only. Block exposes existing selection
  and clipboard actions; Paragraph is the sole intentionally dimmed heading.
  Unassigned editor function keys no longer advertise unsupported actions.
  No dependency/configuration/filesystem behavior changes.

- UI branding is Directory Navigator; CLI, About and system information derive their exact version from Cargo.
  About credits Dmitry Yarygin as author.
  Public documentation describes DOS Navigator inspiration; the app uses MIT licensing.

- Versioned UTF-8 settings store loads legacy files and saves preferences on normal
  exit (System Setup opt-out). Save/Load desktop share its baseline. Unchanged
  files are skipped; external changes, lock conflicts, bad files and write errors
  are reported without silently overwriting them. Failed saves cancel exit.
  Atomic private staging, bounded reads and native bookmarks remain; no new deps.

- Screen rest defaults to 10 minutes; setup includes 30/60-minute delays.
  Existing saved delays remain unchanged.

- Settings use config.conf; legacy config.txt loads when no .conf exists and
  migrates on save, retaining the old file. Migration preserves extension keys
  and detects conflicting changes to either file.

- Bottom row is a Unicode command prompt with blinking cursor; native terminal
  execution reuses the existing runner. Pure cd navigates the panel; shell commands
  use the active directory. Session history, editing and single-line paste preserve
  modal/editor precedence; status notices no longer share the prompt row.

- Terminal teardown disables mouse/paste reporting before restoring input echo;
  mouse-initiated exits/handoffs drain a pending release and reset styles/cursor.

- --version/-V report Cargo’s package version without opening the UI or settings;
  version changes are made only in Cargo.toml. CLI help lists both flags.

- Main menus omit redundant Disk/Window headings; Directory tree stays under
  Manager, with Ctrl+T and Tab panel switching retained. ≡ labels are Refresh
  and Screensaver. Manager retains Swap panels without redundant Activate left/right
  entries. Keyboard/mouse navigation uses the reduced menu set.

## Validation
- Simplified-menu release PTY check passes remaining Alt menus, Refresh, immediate
  Screensaver, Manager tree, keyboard exit and terminal restoration at 80×25.
- PTY regression verifies fragmented/delayed SGR mouse releases cannot leak into
  the parent shell on click-to-quit; keyboard exit and subsequent shell input pass.
- Command tests cover Unicode editing/paste limits, shell plans/cwd, cd failures,
  history, panel/modal precedence and cursor visibility after resizing. Release
  macOS PTY passed execution/return, history, directory changes, Unicode resize,
  listing refresh and terminal/cursor restoration.
- Settings tests cover complete/legacy/native round trips, unchanged exits, unknown
  keys, invalid/versioned/oversized files, symlinks/read-only errors, locks,
  concurrent changes, Apply/Cancel and exit recovery. Release macOS PTY passed
  two-run persistence, clock/sort reload, conflict recovery, mouse opt-out,
  80×25→120×40 resizing and terminal restoration.
- macOS ARM64: fmt check, Clippy with denied warnings, 241 passing tests
  (one opt-in real-Trash success test ignored), release build.
- Menu tests enforce working entries, unique mnemonics, normalized separators,
  unchanged headings, Block selection/clipboard/undo, Paragraph styling and clock
  reservation; existing cascade/mouse/resize and confirmation/config tests pass.
- Release macOS PTY menu workflow passes all main headings, configuration
  cascades/read-only policy, editor Block keyboard/mouse actions, dimmed Paragraph,
  80×25→120×40 resizing and terminal restoration.
- Archive tests cover codecs, implicit folders, Unicode, links, unsafe paths,
  malformed/duplicate/encrypted/CRC data, limits/cancellation/deadlines, changed
  sources, truncated TAR, modal/mouse controls, resizing and panel restoration.
- User-menu tests cover format/limits/quoting, native argument bytes and shell
  characters, context/marks/filter rules, nesting/review/cancel/mouse/resize,
  edit/reload/invalid-file repair, F2 routing, stale tasks and failed commands.
- Directory-tree tests cover lazy loading, hidden/current ancestry, bounded scans,
  stable scroll anchoring, link cycles, failed/denied reads, retry/refresh,
  cancellation/stale tasks, menus, panel-state preservation, mouse and resizing.
- Existing tests cover Unicode/hidden patterns, limits, partial Stop/Close delivery,
  symlink cycles/hard links, permissions and missing/disappearing entries,
  result navigation, modal/mouse controls, debounce, restoration and resizing.
- Release archive PTY workflow passes ZIP/TAR/TAR.GZ browsing, mouse/parent
  navigation, text/hex previews and wrapping, link info, refresh, malformed-file
  errors, resize/title close and terminal restoration; no members written to disk.
- Release user-menu PTY workflow passes F2/nested hotkeys, review/Cancel/Run,
  native cwd/arguments, interactive input, Ctrl+C interruption, failed launch,
  F4 edit/reload, local/global switching, resize/title close and terminal restore.
- Release tree PTY workflow passes Ctrl+T, tree menus, branch folding,
  mouse selection/title close, Enter navigation, Refresh, hidden toggle,
  80×25→120×40 resizing, cancellation and terminal restoration.
- Earlier release PTY workflow passes recursive Find/result jump/F3, size recalculation,
  quick preview/wheel/toggle/Tab, retained Alt+F7, resize and terminal restoration.
  Earlier editor/file-operation, palette, clock, scrollbar and saver workflows pass.
- Real default-app opening verified on macOS. Successful real macOS Trash transfer
  remains unverified: sandbox failure; outside-sandbox test permission declined.
  Isolated Trash tests and settings/confirmation PTY workflow pass.
- GitHub push/PR/manual checks plus manual Build all platforms workflow configured,
  not executed remotely. Build matrix: macOS ARM64/Intel, Linux x86_64/ARM64,
  Windows x86_64. YAML/matrix and exact Unix packaging step validated locally
  (clean archive, executable permissions and SHA-256). Windows packaging untested.
  Locked tests/builds, version smoke check and 30-day archives/
  checksums; no release publishing or signing.
  Linux/Windows remain untested locally.

## Limitations and next steps
- Path-based recursion retains intermediate-directory substitution races; full
  descriptor-relative traversal remains future hardening. Nesting beyond 512 is
  reported as skipped. Kernel I/O cannot be force-cancelled.
- No restart-resume journal or atomic directory-tree publication. Crashes can
  leave identifiable staging files or completed portions of operations.
- User menus retain DN's structure, not DOS batch syntax; parameter prompts,
  legacy macros, persistent command history and output windows remain deferred. External
  programs run with the user's permissions. Windows interruption is unverified.
- Archive browsing is bounded (512 MiB sources/10k entries/20k virtual nodes,
  16 MiB metadata/256 MiB TAR stream/10-second read jobs). ZIP64, multi-volume,
  SFX, passwords and nested browsing are deferred; ZIP Stored/Deflate only.
  Truncated previews do not verify unseen data. Kernel I/O cannot be interrupted.
- Viewer is a bounded preview; editor is UTF-8 only, 2 MiB/100k lines, with linear
  suffix shifts. Size totals are snapshots; no content search or filesystem watcher.
- Linux Trash supports the home volume only. macOS Trash refuses undecodable
  names; Finder Put Back is not guaranteed. Restore through the desktop manager.
  Linux external opening needs xdg-utils and a desktop session.
- Feature priorities: safe archive extraction, history dialog/output recall, then disk
  information/permission editing. Platform validation, traversal/recovery hardening
  and streaming viewing remain important.
  Tree mutations/persistent cache, archive extraction/modification, persistent command history, OS clipboard,
  macros and custom palette editing remain deferred.

Build: cargo build --release; run ./target/release/dn.
Host: RUSTUP_HOME=/private/tmp/dn-rustup, CARGO_HOME=/private/tmp/dn-cargo;
add its bin directory to PATH. The release binary runs independently of Rust.
