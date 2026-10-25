# Directory Navigator — beta

## Architecture
- Native Rust `dn`: Ratatui/Crossterm/Chrono, Unicode segmentation, Unix libc;
  macOS native Trash via `trash`, Windows native shell APIs; minimal ZIP/TAR/gzip
  codec features via zip/tar/flate2; TOML parser for grouped preferences.
- Session Desktop owns editor/viewer/game content and stable WindowId values;
  default Turbo Vision build reads selection from the native Desktop/Window
  registry. Native foreground modal scope captures input before controller
  dispatch. File panels remain the permanent base; the comparison build retains
  the old active-screen model.
- Modular app/input/panels, operations, dialogs, configuration, editor, viewer
  and utilities. Shared focus policy and explicit viewport preparation.
- Cancellable panel scanners; bounded short-task pool; dedicated file-operation
  and traversal workers. Native filesystem boundary and owned sibling staging.
- Shared iterative traversal sends bounded Find/size updates. Dedicated discovery
  state owns session dialogs/options/results; quick view owns debounced reads.

## Implemented
- Native main file-manager drawing: both panes (brief/details and all columns),
  headers, rows/type/tag/selection colors, scrollbars, filters/quick search,
  details/totals/space, menu bar/clock, command line/cursor and function bar now
  use a shared native scene. Existing workers, commands and mouse targets remain.
  Menus/dialogs/quick view still use compatibility drawing above native panes;
  style-only composition preserves inherited colors and menu shadows without
  copying native text into the widget canvas. Footer presentation is toolkit-
  neutral. Persistent native pane controls handle Tab focus and focused navigation,
  paging, marking and guarded open/parent commands. Command-line/shortcut/modal
  precedence remains. Mouse dispatch and other control ownership still need work.
- Native Tetris page drawing: Setup, Help, Top Ten, Game Over and name entry now
  join the play window on the native drawing path. Focused controls, score-save
  errors/retry buttons, Unicode field scrolling and cursor position are preserved.
  Existing game input/controllers and mouse dispatch remain. Native-vs-original
  comparisons cover 420 frame pairs across palettes, languages and sizes.
- Native Tetris play-window drawing: the default runtime paints the game frame,
  well/pieces, Info/Next/Best panels, pause banner, buttons and footer directly
  through Turbo Vision. Existing rules, keyboard/mouse actions and score storage
  remain. The subsequent native page increment also replaces the game pages.
  Native glyph references are independent of the retained canvas; differential
  comparisons cover text/colors/targets, both palettes/languages, transitions and
  resizing. This is the first game renderer slice, not a full game control port.
- Native Turbo Vision desktop/control increment: stable document identities map
  to native views; TV owns selection/z-order, with no second active-screen cache.
  Full-screen native documents paint the retained skin and a native foreground
  modal scope gates input. Native close cannot bypass application save/exit
  guards. Welcome and path/operation prompts use native Dialog/Group focus and
  Button commands plus a Unicode custom field. Existing mouse targets and
  Ratatui visual composition remain; other controls/menus still require porting.
  Full migration is NOT complete. No workflow runs, UI redesign, configuration
  changes or filesystem-policy changes belong to this increment.
  macOS ARM64: fmt, all-target Clippy, 391 passing Rust tests (one existing
  opt-in native Trash test ignored), release build and 36 decoded visual scene
  comparisons pass. Original-backend desktop/welcome/dialog/terminal/workflow
  checks: 36 pass. Native PTYs pass Welcome/settings/help return, Unicode mkdir
  cancel/submit, resize, editor/window/unsaved guards, calculator, Tetris and
  shell handoff/restoration. Windows-target Clippy is blocked by Cargo's
  windows-numerics path-source/cache error; native Intel/Windows/Linux untested.
- Turbo Vision migration IN PROGRESS on feature/turbo-vision: pinned 4.1.0 core,
  real TV custom View/diff buffer and event transport enabled by default. The
  existing Ratatui canvas/control/focus layers remain; this is NOT a full native
  widget/window migration. See docs/TURBO_VISION_MIGRATION.md for required work.
  Transport preserves indexed palette semantics, complete graphemes, wide-cell
  continuations and cursor order. One Session keeps paste and exit/suspend guards.
  Original backend builds with --no-default-features. Dependency notice included
  in release archives; no workflow triggers or external runs changed.
  Transport: 36 independently decoded screen/palette/size comparisons pass;
  glyph/color/cursor/resize and bounded-token tests pass. Mac ARM64 native PTY
  editor/window-list, calculator, Tetris and shell suspend/resume workflows pass.
  Fallback terminal/welcome/workflow checks (19) and packaging helpers (11) pass.
  Native Intel/Windows/Linux are not validated.
  Final transport increment: fmt/all-target Clippy/386 Rust tests/release pass
  on macOS ARM64 (one existing opt-in native Trash test ignored). Full native
  remaining control/renderer migration is outstanding; do not call it complete.
- Tetris presentation follows the DN game/setup/Top Ten references: framed
  Info/Next/Best sidebar, New/Setup/Top 10/Pause shadowed buttons and title close;
  Name/Start/End/Score dotted leaders, centered OK and conditional Retry save;
  ten original named level choices and preview checkbox with keyboard/mouse
  focus, OK/Cancel and title cancellation. Classic Tetris only; no Pentix
  placeholder. English/Russian resources. Board mechanics, scoring, high-score
  persistence/import and name entry safeguards remain unchanged.
  macOS ARM64 fmt/all-target Clippy/381 tests/release pass (one existing opt-in
  native Trash test ignored). Both palettes/languages, resizing, full u64 score
  columns, setup cancellation/apply and mouse targets covered. Native PTY workflow
  checks game/setup/Top 10, level/preview apply, title cancel/close, shell input
  and terminal restoration. Native Intel/Windows/Linux remain untested locally.
- Calculator-only compact 72×18 DN layout: framed black expression field with
  session-history dropdown, radio choices beside a framed right-aligned result
  box, and shadowed Evaluate/Copy/Close/Help buttons. Adds EXP scientific copy
  (15 significant digits) while preserving exact DEC/HEX/BIN/OCT evaluation.
  History recall does not execute/evaluate, help retains expression state, and
  truncated display values copy in full to the existing internal clipboard.
  English/Russian resources; other utility layouts and filesystem behavior unchanged.
  macOS ARM64: fmt, all-target Clippy, 379 tests passed (one existing opt-in
  native Trash test ignored), release build; calculator PTY workflow covers
  evaluation/EXP, copy, history recall, help/return, exit and terminal restoration.
  Both palettes/languages, 80×25/larger/resized screens, clipboard preservation
  and long-value copying covered. Other platforms not run locally.
- Classic DN panel scrollbars use a white strip with black stippling, square
  position marker and arrow buttons on both active/inactive panels. Disabled
  controls remain subdued; Terminal colors and scroll geometry/input are unchanged.
  Verified both layouts/panels/profiles at 80×25 and 132×40, including resize,
  arrows, wheel and drag. macOS ARM64 fmt/Clippy/full tests/release pass.
- Session window list (Manager → Windows…, editor File → Windows…, Alt+0;
  Mac-friendly Ctrl+O then U) switches between the permanent file manager and
  one retained editor/viewer. Stable identities reject stale mouse targets;
  closing active content returns to the most recently used remaining screen.
  F4 resumes retained editor; Open/exit retain unsaved guards, including hidden
  editors. Exit after editor approval still cancels/waits for operation/export
  workers. English/Russian dialog uses existing chrome and palettes, supports
  keyboard/mouse, resizing and modal precedence. No dependencies/config changes;
  multiple documents, cascade/tile and games in the list remain deferred.
  Verified on macOS ARM64: fmt, all-target Clippy, 376 tests passed (one existing
  opt-in native Trash test ignored), release build, and the 40-scene pre-Desktop
  UI baseline (cells/styles/mouse targets). Native PTY smoke checks window
  switching, retained drafts, hidden-editor exit/cancel/discard, successful status,
  shell input and terminal restoration with isolated settings. Intel/Windows/Linux
  execution remains unverified locally; no GitHub workflow was dispatched.
- Internal Desktop foundation replaces independent App editor/viewer/game fields.
  Lifecycle creation/removal maintains unique session IDs and the previous screen
  precedence; focus and rendering share its active identity. Editor/viewer modals
  and cancellable workers stay owned by their content; app-wide overlays unchanged.
  This foundation originally changed no UI; the session window list builds on it
  without adding further content ownership layers. Still one screen per kind.
- Windows settings use `%APPDATA%/Directory Navigator/config.conf`; no lookup,
  migration or writes in the former directory. Companion configuration files
  derive their location from the same settings path. macOS/Linux paths unchanged.
  Added isolated-process Windows regression for Unicode paths, first-run defaults,
  save/reload and untouched former settings. Mac fmt/Clippy/tests/release pass
  (364 tests, one opt-in ignored). Windows cross-check requires missing dependencies;
  download was declined, so native Windows validation awaits manual CI.
- Intel CI mouse-exit leakage reproduced on ARM64 with a 250 ms delayed release:
  the old 100 ms cleanup window returned the report to the shell. Shutdown now
  retains capture/raw input until release (one-second lost-release bound), then
  disables reports, flushes incomplete input and restores the terminal. UI unchanged.
  PTY regression covers fast/delayed/partial releases and keyboard exit; all
  restoration, clean shell input and process-status assertions remain enabled.
  All four Mac Cargo checks pass (364 tests, one opt-in ignored), plus 20
  consecutive ARM64 four-scenario PTY runs. Native Intel confirmation awaits
  manual workflow; no GitHub run was started and no CI checks/retries changed.
- config.conf uses typed TOML v2 sections only; no flat-format reader, conversion
  or migration backups. Invalid contents move to exclusive `_old_config.conf`
  (numbered on collision), then defaults load and normal saves work. Missing files
  use defaults; read-only/symlink/inaccessible/locked recovery failures remain errors.
  Native bookmarks and unknown TOML data survive saves; no .txt fallback.
  Existing lock/conflict/size/read-only/symlink/staging protections remain.
- Options → Import DOS settings reads bounded DN.CFG blocks in a cancellable worker,
  previews supported preferences/skips and offers Apply/Cancel with modal mouse/key
  controls. No source writes or automatic imports; paths/commands/desktop objects,
  palettes, Trash and confirmation policy are excluded. Mapping is independently
  implemented from documented source layouts; real DOS files remain untested.
- Manual Build unified draft release validates one SemVer, checks/builds native
  macOS ARM64/Intel, Windows x86_64 and Linux ARM64/x86_64 from an identical tree.
  All five must pass before the write-only final job tags sources and creates an
  unpublished draft. Downloads contain only five versioned binary archives;
  checksums stay internal. Branches stay unchanged; existing tags/releases refused.
  No signing, automatic dispatch or automatic publication. Separate Linux/Windows
  manual build workflows remain available without release permissions.
  Shared Python packager supports TAR.GZ and Windows ZIP; 11 helper tests,
  5 workflow-policy tests, YAML/shell checks and all four Mac Cargo checks pass.
  Local Windows cross-Clippy was blocked by Cargo's windows-implement cache error;
  native five-platform validation awaits manual dispatch. No GitHub run started.
- Windows-target all-target Clippy now passes with denied warnings on the Mac
  cross-check: narrowly documented Windows-only read-only staging cleanup lint
  exception; Unix-only test imports/mutability are correctly gated. Added a
  Windows regression test for abandoned read-only staging and untouched targets.
  Native Windows execution remains pending; no CI checks disabled or runs started.
- Live-handle ownership checks use Windows volume/file IDs through native API
  calls; path opens do not follow reparse points and failed queries reject ownership.
  Staging publish/cleanup, editor staging, config-lock cleanup and copied directory
  checks use this guard, avoiding creation-time collisions and replacement deletion.
  Regression tests retain moved/replaced stages and symlinks; a Windows-specific
  test forces identical creation times. Windows all-target Clippy cross-check
  passes; Mac fmt/Clippy, 343 tests and release build pass. Native Windows
  execution still requires the manual workflow.
- Workflow policy assertions normalize CRLF before locating the draft job;
  regression exercises LF/CRLF/mixed checkout endings without weakening rules.
  Policy regressions, Windows-target all-target Clippy and all four Mac Cargo
  checks pass (344 tests, one opt-in ignored); no CI run started.
- New-installation welcome uses Directory Navigator/Cargo version, profile-aware
  controls, deletion mode, Start/Keyboard settings/Help, and ≡ → Welcome reopening.
  Dedicated modal focus blocks background input/screensavers and survives resizing.
  Only successful initial loads without config.conf show it;
  dismissal uses normal settings persistence, with no onboarding key or dependency.
- Keyboard profiles Auto/Classic DN/Mac-friendly persist via config.conf/desktop.
  Classic DN is the default for new/missing/unknown settings on every platform;
  explicit Auto/Mac choices remain intact. Auto still follows the OS. Ctrl+O supplies
  function/letter/navigation alternatives with a context guide; text modals retain
  normal input, Esc/click cancels, menus/bars/help reflect the profile. Keyboard
  settings have draft Apply/Cancel and mouse controls; legacy aliases/guards remain.
- Global associations.conf beside config.conf: case-insensitive final extensions,
  extensionless names, internal viewer/hex/editor/default targets and explicit argv
  commands with native whole-argument %f/%d/%n expansion. Enter/double-click override
  archives; Ctrl+Enter/F3/F4 retain explicit behavior. Options Edit/Reload, automatic
  reload after editor close/startup/Load desktop, bounded background reads, inactive
  starter template, validation errors and stale-launch suppression. No new deps.
- Read-only dn.tet/DN.TET import decodes original 740/760-byte DOS tables, merges
  only Tetris top ten into tetris.conf without duplicate reimports, and preserves
  the source/Pentix data. CP437/CP866 selected on first import and remembered;
  unknown line counts display “—”. Modern Unicode scores and original scoring/
  qualification/tie rules remain; v1 score files read, v2 holds import metadata.
- Tetris final-score → qualifying Unicode name → highlighted top-ten flow.
  Named records include lines and start/end levels; Enter accepts, Esc records
  Anonymous. Immediate separate tetris.conf beside settings, independent of
  preference auto-save. Bounded UTF-8 validation, locked merging/atomic writes,
  malformed/read-only/link refusal and retryable session-retained failures.
- Classic three-row footer: cyan entry details, centered yellow selection with
  white figures, gray available/total volume space with white figures. Visible
  marks only; folders counted separately without recursion; unknown sizes marked.
  Width-aware exact bytes/binary units, English/Russian, shared list/mouse geometry.
  Native statvfs/GetDiskFreeSpaceExW snapshots in cancellable tasks; path-bound
  replies, five-second polling and explicit unavailable states. No new deps/keys.
- Alt+F8 / Utilities Commands History: session-only searchable list over the
  existing 100-entry MRU history. Unicode substring filter, navigation/mouse,
  Use recalls without execution; Cancel preserves prompt/cursor.
- File Copy names / paths: listing-order visible marks/highlighted entry to the
  internal clipboard, lossless conversion, CR/LF/2 MiB limits and atomic clipboard
  replacement. No dependencies/configuration keys added.
- English/Russian UTF-8 UI catalogs with English fallback; translated menu
  mnemonics retain English aliases. Configurable independent date order/separators,
  12/24-hour time/separator, preview and Apply/Cancel. Persisted in config.conf;
  legacy defaults unchanged. Clock reserves 13 cells in 12-hour mode. Panel,
  metadata and archive dates use regional formatting; machine timestamps and
  native paths/content stay unchanged. Some detailed diagnostics remain English.
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
  Queued-entry/ancestor checks stop detected replacements; duplicate/overlapping
  batches are refused. Copy reads are bounded by initial size; Unix change times
  detect restored-mtime rewrites. Fallback moves preflight all copied items before
  source cleanup, recheck each removal and sync source parents. UI unchanged.
  Cancellation is rechecked immediately before entry changes/Trash submission.
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

- Versioned UTF-8 settings store reads only config.conf and saves preferences on normal
  exit (System Setup opt-out). Save/Load desktop share its baseline. Unchanged
  files are skipped; external changes, lock conflicts, bad files and write errors
  are reported without silently overwriting them. Failed saves cancel exit.
  Atomic private staging, bounded reads and native bookmarks remain. TOML v2
  adds the toml dependency; other user-menu/association/score formats stay separate.

- Screen rest defaults to 10 minutes; setup includes 30/60-minute delays.
  Existing saved delays remain unchanged.

- Settings use config.conf only; config.txt is ignored with no fallback, migration,
  conflict checks or writes against that filename. Missing .conf uses defaults/
  first-run welcome. Existing locking/atomic saves/unknown-key handling stay intact.

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
- Desktop refactor: pre/post captures match all cells, styles and mouse targets in
  40 scenes (80×25/132×40, both palettes), using fixed volume snapshots on the
  original HEAD and refactored code to avoid native-I/O timing noise. Identity,
  guard, replacement, file-switch failure and late-worker tests pass. All four Mac
  Cargo checks pass: 369 tests, one opt-in ignored; terminal restoration included.
  Native Linux/Windows execution was not performed; no CI was started.
- Grouped config/import: focused tests cover types/defaults/unknown values,
  recovery of invalid/old/empty/unversioned files without overwriting backups, native paths,
  TLV mappings/bounds/termination/duplicates, readonly source/cancellation/stale
  replies, unrelated live sorts, Apply/Cancel and resized mouse geometry.
  Config recovery adds collision, oversized, lock, read-only, symlink and
  save/reload regressions. All four Mac Cargo checks pass (364 tests, one
  opt-in ignored); Windows-target all-target Clippy passes.
  Native Linux/Windows execution remains untested.
  Release PTY reached preview Cancel/Apply and resizing, but full import workflow
  completion was blocked by test-shell cleanup in the sandbox; outside-sandbox run
  was declined. No GitHub workflow was triggered.
- Associations: 13 regressions cover parsing/limits/native argv, literal command
  execution/spawn errors, internal tools, directory/Ctrl+Enter precedence, stale
  replies, invalid-config recovery, safe definition editing/reload, archive
  override and English/Russian menu/mouse geometry at 80×25 and 120×40.
  macOS release PTY passed editor/viewer associations, literal external command
  handoff, definition Edit/Save/Reload, 80×25→120×40 resize and terminal restoration.
- Ten file-operation regressions cover queued/ancestor replacements, restored-mtime
  rewrites before and after move preflight, growing/shrinking inputs, batch retention,
  cancellation boundaries, overlapping selections and hard-linked native/fallback moves.
  Trash cancellation at entry notification never reaches the backend. macOS release
  PTY passed copy/collision refusal, move, permanent-delete Cancel/confirm, Unicode
  mkdir, 80×25→120×40 resize and return to the shell with terminal modes restored.
- Additional deterministic unit regressions: wildcard matcher versus an independent
  DP oracle, Unicode/history editing and bounds, panel/scroll geometry across small
  and resized viewports, score parser/ranking invariants, and owned staging cleanup,
  replacement races/no-overwrite publication. No new test dependencies.
- Tetris: final-score/name/leaderboard flow, Unicode editing/limits, positive-score
  qualification/ties, immediate saves independent of preference auto-save,
  corruption/read-only/link/lock failures, retry and concurrent-instance merging.
  macOS release PTY verified natural game over, Unicode name, top-ten display,
  immediate save/restart persistence, resizing, mouse Back and terminal restoration.
- Footer: selected bytes/counts, folders without recursion, filtered marks,
  unknown lengths, color roles, exact/compact capacity, unavailable states,
  native capacity errors, stale replies/throttling, 80×25/resize/mouse geometry.
  macOS release PTY verified no marks/two files/folder, free/total capacity,
  search handoff, directory changes, mouse selection and terminal restoration.
- Command tools: history bounds/dedup/filter/cancel, non-executing recall, mouse
  selection, modal precedence, Unicode/native clipboard failures and size limits.
  Release macOS PTY passed history menu/Alt+F8, recall without execution, copying
  names/paths, mouse Use/Cancel, resizing and terminal restoration.
- Localization: catalog parity/validation, Cyrillic mnemonics, regional settings
  round trips/defaults/Cancel, midnight/noon/offsets, native content preservation
  and manager/editor/viewer geometry at 80×25 and 120×40. macOS release PTY
  verified language preview/apply/cancel, clock ticks, prompts, editor replacement,
  resize, restart persistence and terminal restoration. Linux/Windows untested.
- Simplified-menu release PTY check passes remaining Alt menus, Refresh, immediate
  Screensaver, Manager tree, keyboard exit and terminal restoration at 80×25.
- PTY regression verifies fragmented/delayed SGR mouse releases cannot leak into
  the parent shell on click-to-quit; keyboard exit and subsequent shell input pass.
- Command tests cover Unicode editing/paste limits, shell plans/cwd, cd failures,
  history, panel/modal precedence and cursor visibility after resizing. Release
  macOS PTY passed execution/return, history, directory changes, Unicode resize,
  listing refresh and terminal/cursor restoration.
- Settings tests cover complete/native TOML round trips, unchanged exits, unknown
  keys, invalid/versioned/oversized files, symlinks/read-only errors, locks,
  concurrent changes, Apply/Cancel and exit recovery. Release macOS PTY passed
  two-run persistence, clock/sort reload, conflict recovery, mouse opt-out,
  80×25→120×40 resizing and terminal restoration.
- Draft-release workflow YAML/five-platform matrix and all shell steps validated
  locally. Ten stdlib Python tests cover versions/output metadata, collisions/API
  failures, root-only lockfile edits, binary-version checks, archive contents/modes,
  checksums and incomplete/tampered bundles. Rust policy tests enforce manual-only
  triggers and isolated draft-job writes. Isolated macOS ARM64 source override to
  1.2.3-rc.1 built with --locked, packaged and extracted; binary version/checksum
  verified without changing the development version. Unified release and
  separate build workflows pass YAML/shell/policy checks; all four local Cargo
  checks and eleven helper tests pass. No GitHub run/tag/release initiated here.
  User-reported prior run passed both Mac builds; Linux failed Clippy (fixed in
  terminal_exit). Updated remote workflows and Linux builds remain untested.
- Terminal-exit PTY waits tolerate transient empty reads while the shell lives,
  drain buffered output after exit, and fail with stage/status/escaped output.
  Shell handoff checks DN's exit code and exact terminal-mode restoration before
  verifying unchanged input; fragmented mouse-release checks remain mandatory.
  Five focused wait tests and 20 consecutive ARM64 mouse/keyboard PTY runs pass.
  No UI changes or CI bypass/retry; updated Intel run still required.
- Conf-only tests verify ignored/unchanged config.txt files (including broken
  links), .conf defaults/welcome, save/reload/unknown-key preservation and conflict
  safeguards. DOS score fixtures verify both record layouts, XOR/endian fields,
  CP437/CP866, stable duplicate/tie merging, unknown lines/name whitespace, modern
  round trips, retries, malformed/readonly/link inputs and unchanged original files.
  Release macOS PTY passes ignored txt + normal conf persistence and DOS fixture
  import/display/reopen without duplicates, with terminal restoration. A real
  user-supplied dn.tet has not been tested; other platforms remain untested.
- Welcome tests cover missing/existing/ignored-txt/invalid/inaccessible settings, CLI
  bypass, session-only dismissal, save-on-exit/restart behavior, modal isolation,
  locale/profile updates, mouse controls, screensaver suppression and small sizes.
  Release macOS PTY passes first/subsequent launches, Help/settings return,
  persistence, title close, manual reopening, resizing and terminal restoration.
- Classic DN defaults on all platforms and saved Mac/Auto choices round-trip.
  Release macOS PTY verifies Classic first start, switching to Mac in Settings,
  saved-profile restart, welcome/Help/mouse/resize and terminal restoration.
- Keyboard-profile tests cover missing/invalid configuration, Apply/Cancel and
  desktop persistence, original confirmations and unsaved guards, prefix input
  isolation, paste/mouse/focus changes, editor word movement and user-menu review.
  English/Russian labels and targets fit 80×25/120×40. Release macOS PTY passes
  Auto→Classic→Mac, saved/reloaded preferences, editor save/close, viewer wrap/search,
  copy cancellation, resize and terminal restoration.
- macOS ARM64: fmt check, Clippy (including all test targets) with denied warnings, 369 passing tests
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
- GitHub Rust checks and Build all platforms are workflow_dispatch-only; no push,
  pull-request or scheduled triggers. A local regression test guards every workflow.
  No workflows have been executed remotely here. Build matrix: macOS ARM64/Intel, Linux x86_64/ARM64,
  Windows x86_64. YAML/matrix and exact Unix packaging step validated locally
  (clean archive, executable permissions and SHA-256). Windows packaging untested.
  Locked tests/builds, version smoke check and 30-day archives/
  checksums; these two existing workflows do not create releases or sign builds.
  Linux/Windows remain untested locally.

## Limitations and next steps
- Desktop is an ownership foundation only: multiple documents, window switching,
  geometry, Cascade/Tile and desktop-window persistence remain future stages.
  Next: multiple full-screen documents and a window list before overlapping layouts.
- Ancestor/entry snapshots detect replacements between operation stages, but
  path-based recursion still has check-to-use races; full descriptor-relative
  traversal remains future hardening. Nesting beyond 512 in discovery is
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
- Feature priorities: safe archive extraction, output recall, then disk
  information/permission editing. Platform validation, traversal/recovery hardening
  and streaming viewing remain important.
  Tree mutations/persistent cache, archive extraction/modification, persistent command history, OS clipboard,
  macros and custom palette editing remain deferred.

Build: cargo build --release; run ./target/release/dn.
Host: RUSTUP_HOME=/private/tmp/dn-rustup, CARGO_HOME=/private/tmp/dn-cargo;
add its bin directory to PATH. The release binary runs independently of Rust.
