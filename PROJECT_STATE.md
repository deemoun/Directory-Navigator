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
- Manual Build draft release accepts a validated SemVer, builds/checks two native
  macOS Apple Silicon/Intel targets, verifies embedded versions and packages versioned assets.
  Final write-only job verifies complete checksums and tags the identical source
  tree (release-only Cargo version commit if needed), then creates an unpublished
  draft; branches stay unchanged. Existing tags/releases are refused; no Windows
  assets, signing, automatic dispatch or automatic publication.
- Separate manual Build Linux checks/builds native ARM64/x86_64 with a version
  input, verified archives/checksums and read-only permissions; no tags/releases.
  Linux failures do not block Mac drafts. Fixed Linux openpty test Clippy warning.
- Separate manual Build Windows checks/builds x86_64 MSVC, verifies the requested
  version, and uploads dn.exe/docs/license in a versioned ZIP with SHA-256 files.
  Read-only permissions; no tags/releases or dependency on Mac/Linux workflows.
  Workflow YAML/policy checks, ten helper tests and all four local Cargo checks
  pass; Windows/Linux native builds still require manual GitHub validation.
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
  Atomic private staging, bounded reads and native bookmarks remain; no new deps.

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
- Settings tests cover complete/legacy/native round trips, unchanged exits, unknown
  keys, invalid/versioned/oversized files, symlinks/read-only errors, locks,
  concurrent changes, Apply/Cancel and exit recovery. Release macOS PTY passed
  two-run persistence, clock/sort reload, conflict recovery, mouse opt-out,
  80×25→120×40 resizing and terminal restoration.
- Draft-release workflow YAML/Mac-only matrix and all shell steps validated
  locally. Ten stdlib Python tests cover versions/output metadata, collisions/API
  failures, root-only lockfile edits, binary-version checks, archive contents/modes,
  checksums and incomplete/tampered bundles. Rust policy tests enforce manual-only
  triggers and isolated draft-job writes. Isolated macOS ARM64 source override to
  1.2.3-rc.1 built with --locked, packaged and extracted; binary version/checksum
  verified without changing the development version. Updated Mac-only release
  and separate Linux workflows pass YAML/shell/policy checks; all four local Cargo
  checks and ten helper tests pass. No GitHub run/tag/release initiated here.
  User-reported prior run passed both Mac builds; Linux failed Clippy (fixed in
  terminal_exit). Updated remote workflows and Linux builds remain untested.
- Terminal-exit PTY waits tolerate transient empty reads while the shell lives,
  drain buffered output after exit, and fail with stage/status/escaped output.
  Shell handoff checks DN's exit code and exact terminal-mode restoration before
  verifying unchanged input; fragmented mouse-release checks remain mandatory.
  Five focused wait tests and 20 consecutive ARM64 mouse/keyboard PTY runs pass.
  No application/UI changes or CI bypass/retry; updated Intel run still required.
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
- Keyboard-profile tests cover legacy/unknown configuration, Apply/Cancel and
  desktop persistence, original confirmations and unsaved guards, prefix input
  isolation, paste/mouse/focus changes, editor word movement and user-menu review.
  English/Russian labels and targets fit 80×25/120×40. Release macOS PTY passes
  Auto→Classic→Mac, saved/reloaded preferences, editor save/close, viewer wrap/search,
  copy cancellation, resize and terminal restoration.
- macOS ARM64: fmt check, Clippy (including all test targets) with denied warnings, 341 passing tests
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
