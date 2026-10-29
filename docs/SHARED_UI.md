# Shared UI definitions

This refactor preserves DN's existing appearance, key precedence, modal ownership
and filesystem safeguards. It adds no command bus, public plugin SDK, configuration
version, dependency or new preference.

## Commands and shortcuts

`src/commands.rs` describes manager command keys and editor/viewer key bindings,
including ordered required/forbidden modifiers. Binding metadata also supplies Mac
shortcut labels; editor/viewer prefix proposals reuse these bindings where the
existing prefix names them. Function-key labels remain explicit when menu or
status entries move. Manager Mac recipes retain their original behavior: for
example, active-panel refresh and the menu's workspace refresh are distinct.

`shortcuts` retains prefix state, owner/eligibility checks and navigation adapters;
`input`, editor/viewer controllers and plugin hosts retain input scope, text entry,
validation and guarded execution. Menus and viewer controls share existing
availability rules. Help prose lives in `commands::help`, with compatibility
reexports in `shortcuts`. The text and localization keys are preserved verbatim;
prose is maintained alongside the bindings rather than regenerated with new
wording or layout.

## Dialog style and settings

`dialog_theme` names existing DOS attributes for text, headings, disabled text,
frames and close markers. It provides the common centering and standard button-row
metrics. Content-specific dimensions and native Turbo Vision control palettes stay
explicit; no restyling or global palette replacement occurs.

Compact prompt, Copy/Move, archive and game button rows reserve the translated
caption width plus both DN focus markers. The same bounds drive native painting
and mouse targets; compact rows redistribute padding rather than clipping a
caption or overwriting its first character with the left arrow. Caption widths
use Unicode screen cells and exclude mnemonic markers.

`settings::definition` describes ordinary boolean fields once: localized caption,
existing TOML group/key and typed Config accessors. Settings drafts and TOML read/
write paths use the same descriptions. Form metadata supplies titles, dimensions,
field order and the existing two-column layout. Defaults still come from Config.
Regional/profile choices use explicit typed `ChoiceSpec` variants, captions and
Config accessors. Cycling and dropdown contents share those variants, including
current-first ordering and legacy unknown-separator fallbacks. Their controllers,
draft transactions and child-dialog ownership remain; the plugin list still comes
from its registry. TOML v2, validation,
unknown-field preservation and atomic publication are unchanged.

## Dialog proposals

`Proposal::deliver` checks the live modal/document identity before delivering focus
and action together. `dialog_controls::apply` writes controller-owned focus through
a common adapter, then calls the existing semantic controller. Prompts, advanced
Copy/Move, settings, editor, viewer and tools share it. Plugins use the same delivery
gate while receiving their own Focus/Action events. Keyboard and mouse share the
Colors/Columns semantic action bridge; native file-dialog release behavior and
specialized scrolling/dragging stay in their existing paths.

## Lists, histories and one-shot tasks

`list_state::ListViewport` shares cursor/offset clamping with explicit policies:
archive parent rows, last-item versus last-page offset limits and window-list
blank-space behavior are preserved. Controllers retain key mapping, selectable
rows and stable identities. `ClickTracker` takes time and identity explicitly;
strict 400 ms, inclusive 400 ms and inclusive 500 ms policies remain distinct.
List wheel direction shares the existing three-row step.

`history::remember` supplies only deduplicating MRU insertion and truncation.
Command/file histories retain capacity 100 and Copy destinations 16; lexical
identity, path resolution and calculator history semantics are unchanged.

`task::take_reply` polls one-shot task slots, keeps pending slots intact and retires
completed/disconnected slots once. It delegates stale owner/generation rejection
to Task::try_recv. Viewer, archive, information, import and preview consumers keep
their existing error strings and result application. Streams, dedicated file jobs,
Stop/cancellation behavior, worker bounds and in-flight save rules are unchanged.

## Dialog registration

`dialog_adapter` registers keyboard, mouse, immutable spec builders and controller
focus setters for ten ordinary dialog scopes. Input, overlay painting, viewport
signatures and proposal application consume the same registration. Global input
precedence stays in the host, including window-list handling before global keys.
Native Help/FileDialog, editor pages, Welcome and plugin hosts retain their special
paths; the registry adds no new command bus or owner lifecycle.

## Test support and visual contract

`tests/support` owns private temporary fixtures, bounded background polling, native
render capture and production keyboard/mouse helpers. Editor, menu, plugin settings, command tools, dialog navigation, default buttons,
native controls, remaining forms and scrollbar tests share these helpers; document-specific assertions remain local.

`ui_refactor_parity` compares 320 frozen pre-refactor native captures across both
languages, palettes and shortcut profiles at 80x25 and 132x40. Captures hash all
cell symbols/styles, cursor coordinates and ordered mouse regions. They include
settings, calculator, viewer search, editor/menu/search/help, window/command/file
histories, bookmarks, Colors and nested choice lists. Repeated rendering
uses the same prepared snapshot. Clock and filesystem-dependent backgrounds are excluded; only digits in the
regional clock preview are masked. The two dropdown cases containing Automatic's
OS-dependent name use macOS goldens; other platforms omit those cases and retain
their host-aware choice behavior tests. Existing dedicated checks remain.
The fixture is never automatically regenerated by the test.

The complete macOS suite, formatting, all-target Clippy, release and no-default-
features checks apply. Release PTYs cover editor/viewer controls, prompt/settings
navigation, Unicode, nested Help/choices, Copy safeguards/history/verification,
plugin disable/re-enable, resizing and terminal restoration. File writes stay in
private temporary fixtures. Windows/Linux validation remains deferred.
