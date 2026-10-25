# Turbo Vision migration status

The default runtime draws all application surfaces through native Turbo Vision
cells and routes positional mouse hits through a TV Group. This includes the
file panes, dropdown menus, shared dialogs, quick view, editor/viewer, utilities,
welcome/settings, auxiliary tools and all Tetris pages. The native runtime does
not call Ratatui widget renderers or compose these screens in a Ratatui buffer.

The migration is **not yet complete**: Ratatui still supplies shared text/style
and geometry data, the outer Terminal/Frame adapter, and an explicit reference
renderer. Existing application focus/command controllers also remain; converting
rendering does not imply that every control now uses stock TV widget semantics.
Native Intel Mac, Windows and Linux execution remains unvalidated.

## Native rendering and input

- Upstream TV 4.1.0 remains pinned to
  `0f2b396a1844c59e31c4714d68ffb6815662918e`, enabled by default. OS clipboard
  and screenshot features remain disabled.
- `painting::Frame` and the shared block/styled-text/clear/list primitives have
  independent native and reference implementations. Screen code shares layout
  and command descriptions; the default `ui::draw_native` uses `NativeFrame` to
  build a TV scene directly. `ui::draw` explicitly selects the reference path.
- The native painter preserves inherited styles, dimmed menu shadows, centered
  titles, wrapped/scrolled Unicode text, editor highlighting/selection, search
  matches, focused controls and cursor requests. Clearing a menu over a wide
  glyph repairs its retired continuation cell. The old overlay-marker bridge
  has been removed.
- Native desktop/window identity, selection and z-order remain authoritative for
  documents. Foreground modal scopes gate delivery. Stable application WindowIds
  map to opaque native ViewIds without narrowing to u16.
- Persistent native pane controls handle Tab focus and focused navigation,
  paging, marking and guarded open/parent commands. Welcome and operation/path
  prompts retain native Dialog/Group focus and Button dispatch, with the custom
  Unicode field used instead of stock InputLine semantics.
- A cached native Group of hit views performs front-to-back positional dispatch
  for each current target layout. Overlapping modal surfaces consume hits before
  background views. The original mouse payload reaches domain controllers for
  wheel, drag, double-click and modifier handling. The default path no longer
  selects targets by reverse-searching the application's rectangle vector.
- Existing editor buffers, undo/search/storage, viewer navigation, file-operation
  guards, asynchronous workers and command controllers are reused. A native
  button or hit view cannot bypass save-before-close or filesystem safety checks.

## Transport and compatibility

One Session still owns raw mode, paste, mouse capture, delayed release draining,
exit/panic restoration and shell suspend/resume. There is one terminal input
reader. The native differential renderer transports complete graphemes, wide
cells, ZWJ sequences and literal private-use characters through the bounded codec.
ANSI/xterm indexed colors, RGB/reset colors and supported styles are preserved.
Cursor requests are committed after painting.

`--no-default-features` retains the original renderer for comparison/rollback.
Its widget rendering is isolated in `painting::Widget::reference`; native methods
write scenes directly. Formats, configuration and filesystem policy are unchanged.
The native reflow adaptation's MIT notice is included in THIRD_PARTY_NOTICES.md,
which is already packaged by release workflows. No remote workflows were run.

## Validation

Local macOS ARM64 checks pass:

- 399 Rust tests; one existing opt-in native Trash test remains ignored.
- Formatting, all-target Clippy and release build.
- 87 original-backend appearance/dialog/editor/utility/command/game/terminal checks.
- 1,008 file-manager frame pairs, 420 Tetris frame pairs and 448 requested-surface
  comparisons: text and foreground/background colors are decoded independently
  from terminal output, with cursor and mouse target checks.
- The 56 requested-surface scenes cover both palettes/languages and two sizes:
  menus/submenus, prompts/settings, quick-view text/wrapping, viewer text/hex/wrap/
  search, editor highlighting/selection/scrolling/search/replace/dirty guards,
  calculator results/errors/history/help, ASCII/CP437, loaded system/memory
  information, auxiliary dialogs and screensaver. Native frames must leave the
  reference canvas empty.
- Dedicated reflow comparisons cover indentation, whitespace, long words,
  explicit blank lines, NBSP/zero-width spaces and multi-scalar glyphs at narrow
  widths. Native mouse tests cover overlapping targets, modal surfaces, replaced
  layouts, outside hits and coordinate bounds. Existing mouse/controller tests
  execute through native positional routing in the default build.
- Interactive terminal checks cover viewer search/wrap, quick view, calculator
  history/help, Unicode editor paste, unsaved-close cancel, Save As to a disposable
  file, mouse close actions, resizing and terminal restoration with subsequent
  shell input. Main-pane and Tetris terminal workflows also remain covered.

These checks establish the local native painting/dispatch increment. They do not
establish native cross-platform execution or equivalence of every stock TV control.

## Remaining transition work

1. Remove Ratatui/Crossterm presentation types from shared model/controller
   interfaces and replace the outer Ratatui Terminal/Frame adapter. Keep the
   reference implementation explicitly isolated until its comparisons can retire.
2. Finish toolkit ownership of per-control keyboard focus and nested modal
   lifetimes where application controllers still own it. Retain existing input
   semantics and guards; stock select-all/clipboard/overwrite behavior must not be
   introduced accidentally. Native text controls must preserve full graphemes.
3. Add the intended movable/resizable/cascaded multiwindow behavior; the present
   document views intentionally retain the established full-screen layouts.
4. Run native validation on Intel Mac, Windows and Linux, including Unicode,
   mouse/paste, configuration, idle workers, shell handoff and terminal cleanup,
   before making release claims for those platforms.
