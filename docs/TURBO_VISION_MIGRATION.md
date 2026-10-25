# Turbo Vision migration status

The migration is **not complete**. Turbo Vision now owns the native desktop,
modeless-window selection, z-order and a foreground modal input scope. Welcome
and operation prompts use native Dialog/Group focus and Button command dispatch,
with a custom Unicode field control. All Tetris screens and the main file-manager screen draw directly through TV;
Ratatui still composes the other visual surfaces;
other menus/dialogs and the application event controllers remain. This is a
native ownership/control increment, not completion of the full widget port.

## Implemented

- Pin upstream 4.1.0 to commit `0f2b396a1844c59e31c4714d68ffb6815662918e`.
- Default `turbo-vision` feature drives TV's custom View, cell buffer, differential
  rendering and terminal event polling. Native OS clipboard/screenshot features
  are disabled; the existing internal clipboard and shortcut behavior remain.
- One Session retains raw mode, bracketed paste, delayed mouse-release draining,
  shell suspend/resume and panic/exit restoration. There is one input reader.
- A bounded live-cell grapheme codec retains combining marks, ZWJ sequences and
  literal private-use characters despite upstream's single-char Cell. Retired
  tokens are reclaimed only after the native diff buffer commits its frame.
- Color transport preserves ANSI/xterm indexed colors, RGB and reset colors;
  Terminal-profile colors continue to follow the terminal theme. Cursor requests
  go through TV after painting, so clock ticks cannot move the prompt cursor.
- Stable DN WindowIds map to opaque native ViewIds without u16 truncation.
  The model reads active identity from the native desktop rather than a second
  cached selection. Models still own buffers and workers; native windows cannot
  bypass unsaved-document or filesystem guards by closing themselves.
- The selected native document paints the existing full-screen skin (with native
  Tetris content replacing that part of the canvas). A native
  modal foreground view captures delivery ahead of background documents; closing
  or replacing a document updates its modal owner immediately. Resize and shell
  resume invalidate native raster state; inactive views retain no stale codec
  tokens. Window movement/cascade are not enabled by this increment.
- Welcome and file-operation/path prompts use native Dialog/Group focus and
  Button command dispatch. Immediate button commands cannot be queued into a
  later dialog. Help/mnemonics preserve previous focus. DN's field is a custom
  TV View using existing UTF-8 byte-cursor/edit limits; stock InputLine rejects
  wide and combining input. The current canvas remains their visual skin, and
  their mouse targets remain in the existing dispatcher.
- `--no-default-features` retains the original backend as a comparison/rollback
  build. CLI help/version remain noninteractive. Formats/configuration unchanged.

## Native Tetris renderer

The default runtime now draws every Tetris screen directly from the game model
through the native document's `Terminal::write_cell` path. This includes its frame,
well, settled/falling pieces, preview, score/level/best panels, pause/game-over
banner, shadowed buttons and function bar, plus Setup, Help, Top Ten, Game Over
and name-entry pages. The game is absent from the Ratatui canvas on this path; the surrounding file-manager shell also uses native cells.
Native cells have separate codec references so removing or replacing the game
cannot invalidate Unicode tokens still used by the retained background.

Game rules, keyboard actions, mouse dispatch and score persistence are unchanged.
All game pages use native cells, including focused radio/check/button drawing,
score-save errors and retry buttons, and horizontally scrolled Unicode name input.
Name-entry cursor placement is submitted to TV after painting. Input still uses
the existing game controllers and mouse targets; native control/event ownership
remains a later increment.
The original renderer remains available with `--no-default-features` and as a
headless comparison oracle (`ui::draw`); the default executable uses
`ui::draw_native`.

A differential-frame comparison checks text, foreground/background colors and
mouse targets across both palettes, English/Russian, three terminal sizes,
piece movement/rotation, pause, preview, all game pages, close/reopen, resize,
undersized terminals, game-over banner and screensaver return. It also checks
that native game content is absent from the legacy canvas. A separate check
covers a terminal shrinking between scene preparation and native painting.

The comparison sequence includes 35 frames for each palette/language/size
combination (420 frame pairs): setup focus changes, selected scores,
pending/failed saves, empty scores, long Unicode names, cursor positions,
long path errors, multiple spaces and multiline messages. It checks that game
page titles never enter the legacy canvas.

Current local validation: 394 Rust tests pass (one existing opt-in Trash test
ignored). All-target Clippy, formatting, release build and 30 fallback
desktop/Tetris/terminal checks pass. An interactive macOS ARM64 terminal
check covers play/setup/help/scores navigation, keyboard and mouse return,
resizing through an undersized terminal, shell input and terminal restoration.
Native Windows, Linux and Intel Mac execution remains unvalidated.

## Native file-manager renderer

The default runtime draws both panes and the main browsing shell from native
cells: active/inactive frames and path titles, brief/detail rows, all column
presets, type colors and selection/tag colors, scrollbar/track/thumb, filter and
quick-search lines, entry details, selection totals and volume space. The menu
bar/mnemonics, clock, command line/cursor and function-key bar use the same native
scene. The legacy pane/table/paragraph renderers are not invoked for these
components by the default runtime. They remain as a comparison/fallback path.

Selection/space presentation is now toolkit-neutral and shared with the old
renderer. Command-line viewport preparation is shared without invoking a widget.
A persistent native TV Group owns the two pane controls and Tab/Shift-Tab focus.
The focused native pane dispatches arrows, column/page movement, Home/End,
Enter/parent and marking as toolkit-neutral pane commands. Application models
still own listings and selection; opening files/directories retains the existing
filesystem and document guards. Command-line editing, shortcuts, quick search and
modal scopes keep precedence ahead of pane navigation. Mouse targets remain in
the existing dispatcher and synchronize through the same native focus boundary.
This does not claim completion of all native control/event ownership.

Shared native composition supports retained menus/dialogs above the panes.
Style-only markers carry background styles through the overlay canvas while
native text stays outside it. The transport resolves those markers on every
frame, including when native text changes without an overlay diff, and never
writes them to the terminal. This preserves dimmed menu shadows, wide/combining
text and dialog inheritance. Quick view, dropdown menus, dialogs and other
unported surfaces remain on the compatibility renderer; panes underneath remain
native throughout.

The file-manager comparison covers 42 frames across two palettes, two languages,
three initial sizes and both layouts (1,008 frame pairs). It checks colors/text,
hit targets and cursor position; layout/column changes, marking, scrolling,
loading/empty lists, unavailable metadata/space, filters, quick search, long
Unicode commands, modal pages, quick view, nested menus, native text changes
under a shadow, resize, screensaver and Tetris transitions are included. Native
panes also remain active behind Tetris, including with quick view enabled.

File-manager increment validation on macOS ARM64: 396 Rust tests pass (one existing
opt-in Trash test ignored), together with 58 original-backend pane/appearance/
footer/navigation/scrollbar/command/dialog/terminal/game checks. Formatting,
all-target Clippy and release build pass. Interactive terminal checks pass pane
switching, directory entry/return, marking, menu/prompt return, mouse focus/wheel,
resize, game/setup/help/scores, terminal restoration and subsequent shell input.
No native Windows, Linux or Intel Mac execution is implied by these local checks.

## Remaining required work for a full transition

1. Extract toolkit-neutral geometry, text measurement and command/event boundaries
   from shared models; stop exposing Ratatui/Crossterm types to domain controllers.
2. Extend native document/modal ownership into individual controls and nested
   modal scopes. Currently a foreground full-screen scope captures input before
   existing controller dispatch; it does not replace the whole focus router.
3. Finish native mouse/control ownership for the migrated panes and native game
   input controls; port
   remaining dialogs/menus/buttons/inputs, quick view, tree/archive/find views,
   editor/viewer, utilities, welcome and screensaver surfaces. Keep
   existing editor buffer/search/undo/storage and all filesystem workers.
4. Port headless UI/input tests to native controls and retire the legacy canvas,
   focus router, hit-region dispatcher and remaining Ratatui dependency only after
   parity. This intermediate transport is not the end architecture.
5. Re-run all safety, Unicode, mouse/paste, config, CLI, idle-worker and PTY
   restoration contracts, with native validation on Mac ARM/Intel, Windows and
   Linux before making cross-platform release claims. Workflows stay manual.

The present upstream cell model does not retain multi-scalar graphemes in native
widgets, and stock InputLine accepts only one-column typed characters. DN's custom
field preserves existing editing behavior; the transport codec preserves display.
Native text drawing still needs equivalent handling before retiring the canvas.
The new native forms deliberately do not introduce stock input semantics (select
all on focus, automatic clipboard use, or new overwrite/selection shortcuts).
No new filesystem behavior or configuration migration belongs in this work.

## Validation of the transport increment

- Independent ANSI screen decoding compares 36 manager, dialog, utility, editor,
  viewer, game/setup and window-list scenes at 80×25/132×40 in both profiles.
- Transport tests cover combining marks, wide/ZWJ glyphs, literal private-use
  characters, live-token reclamation, changing text, resize, ANSI/RGB/reset colors
  and cursor placement. They do not establish native-widget equivalence.
- Mac ARM64 PTY workflows exercise editor/window switching and unsaved guards,
  calculator paste/history/help, Tetris setup/scores, and command execution with
  suspend/resume. Shell input and terminal restoration pass.
- Original-backend terminal-exit/welcome/workflow checks and release packaging
  helper tests pass. Native Intel, Windows and Linux execution is not validated.

## Native desktop/control increment

- Native selection, document replacement/removal, modal receipts, repeated scope
  replacement and stale identities have dedicated tests. Existing desktop tests
  continue to cover dirty buffers, failed loads, worker cancellation and quit
  guards. Native forms test Unicode input, immediate commands and focus cycling.
- All 36 independently decoded scene comparisons bind the real application
  desktop, including editor/viewer/game documents and foreground modal scopes.
- Native painting still uses the established canvas as its skin. Completion
  requires the remaining controls/menus and custom view renderers, followed by
  retiring Ratatui and the legacy focus/hit-target layers. No cross-platform
  native validation is implied by the local macOS ARM64 checks.

Validation on macOS ARM64: formatting, all-target Clippy, 391 Rust tests and the
release build pass (one existing opt-in native Trash test ignored). The original
backend passes 36 desktop/welcome/dialog/terminal/workflow checks. Native PTY
workflows cover Welcome/settings/help return, Unicode mkdir cancel/submit,
resizing, retained documents/unsaved guards, calculator, Tetris and command shell
handoff; terminal settings and subsequent shell input are restored. Windows
cross-Clippy could not resolve windows-numerics from Cargo's path source/cache;
native Intel/Windows/Linux execution remains untested. No GitHub runs dispatched.
