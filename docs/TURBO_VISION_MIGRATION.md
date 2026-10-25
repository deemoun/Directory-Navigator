# Turbo Vision migration status

The migration is **not complete**. Turbo Vision now owns the native desktop,
modeless-window selection, z-order and a foreground modal input scope. Welcome
and operation prompts use native Dialog/Group focus and Button command dispatch,
with a custom Unicode field control. Ratatui still composes the visual skin;
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
- The selected native document paints the existing full-screen skin. A native
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

## Remaining required work for a full transition

1. Extract toolkit-neutral geometry, text measurement and command/event boundaries
   from shared models; stop exposing Ratatui/Crossterm types to domain controllers.
2. Extend native document/modal ownership into individual controls and nested
   modal scopes. Currently a foreground full-screen scope captures input before
   existing controller dispatch; it does not replace the whole focus router.
3. Port shared dialogs/menus/buttons/inputs, then custom panel/tree/archive/find
   views, editor/viewer, utilities, welcome, screensaver and game windows. Keep
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
