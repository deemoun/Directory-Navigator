# Turbo Vision migration status

The migration is **not complete**. The branch currently replaces the terminal
output/input transport with a real Turbo Vision terminal and custom View; the
Ratatui canvas and existing DN controllers remain as a deliberate compatibility
boundary. It must not be described as a native-window/native-control conversion.

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
- `--no-default-features` retains the original backend as a comparison/rollback
  build. CLI help/version remain noninteractive. Formats/configuration unchanged.

## Remaining required work for a full transition

1. Extract toolkit-neutral geometry, text measurement and command/event boundaries
   from shared models; stop exposing Ratatui/Crossterm types to domain controllers.
2. Adapt stable DN WindowIds to native TV Desktop/Window ownership. Make TV the
   authority for focus, modality and z-order, preserving current unsaved guards.
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
widgets. The transport codec addresses custom DN canvas views; native text-taking
controls still need equivalent handling before replacing DN's editor/inputs.
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
