# Built-in tools

The first plugin iteration contains only Calculator and Tetris. They are compiled
into DN; there is no dynamic loader, binary ABI, installation format, external SDK,
new dependency or separate plugin configuration format. This is an internal Rust interface which
may evolve with the application.

## Enable and disable

Options → Configuration → Plugins lists every registered built-in tool, including
disabled tools. Checkboxes edit a draft: OK applies it, Cancel discards it. All
plugins are enabled by default. Disabling closes the current presentation and
hides its menu entry and blocks shortcut launches. Enabling restores the menu
entry without opening the tool automatically. Calculator
session history and pending Tetris score records survive disabling.

The preference uses the existing configuration file and save behavior (Save desktop
or save on exit). Its optional TOML v2 section is:

```toml
[plugins]
disabled = ["tetris"]
```

Older configurations enable all tools. Unknown plugin IDs are retained on save.

## Contract

`plugins::ToolPlugin` supplies a `Definition`, opening availability, open/event/close
callbacks, an optional wake deadline, controls and an immutable view. A definition
has a stable `PluginId`, localized title/menu label, existing shortcut label, menu
optional named root category and either Dialog or Document presentation. Existing launch shortcuts
remain in DN's shortcut maps and dispatch the generic `menu::Action::Plugin(id)`
or host opening operation. The manager hierarchy, built-in plugin captions,
shortcut labels and placement live in `src/menu/definition.rs`. Built-ins derive
menu metadata from that hierarchy; removing their entry leaves them registered
and accessible through existing launch shortcuts. Other registered tools may
request an optional named root category; entries not already placed are appended
to that category, independently of its display position.

Events cover keys, atomic paste, local `ActionId` values, mouse input, focus,
activity transitions and timer ticks. The host runs ordinary Tab/Shift+Tab,
Enter/Esc and button Space through the plugin's actual shared native controls.
A plugin can retain specialized key behavior, as Tetris does for Space.

Callbacks receive a small `Context`: presentation owner, viewport, language,
palette, activity, injected time and the existing configuration storage path.
They never receive `App` or own the terminal/event loop. Effects are limited to
closing their own presentation, copying text to DN's internal clipboard and
opening owned Help. Interactive Copy/Help effects require the foreground binding;
background activity cannot change another tool's focus or clipboard. Filesystem policy and score validation remain in the existing
score store; no generic filesystem or background-job API is introduced.

A view is either a shared dialog or a custom native scene plus shared controls.
Preparation delivers activity changes before capturing the frame; painting reads
model state and uses the last injected time. The host retains clipping, cursor,
mouse publication, native focus and terminal restoration.

## Lifetime and ownership

The registry lives for the entire application session in `Desktop.plugins`.
Document content stores a plugin ID rather than a game model. Each presentation
binds the plugin to a fresh existing `ModalId` or `WindowId`; callbacks and effects
are checked against that live binding. Published mouse frames retain the existing
owner/layout revision checks. Blocking dialogs and terminal-output mode prevent
input from reaching underlying tools. Closing a document retires its binding and
owned children through the existing document/modal lifecycle.

There is one presentation per built-in tool. Calculator retains its expression,
result, base and history across closes; opening resets focus and its history page.
Tetris releases its current game on close, while the score store and pending
unsaved records remain in the module for later reopening/retry. The existing
`tetris.conf` path, format, DOS import, merging and write safeguards are unchanged.
Tetris appears in the window list; inactivity, blocking dialogs, output/screensaver
and viewports below 80×25 suspend its clock without catch-up.

Compatibility launch/action helpers in `utilities` and `tetris`, and typed state
accessors used by tests, forward to the registry. They are not a separate plugin
controller or a stable external API. Other utilities and the editor/viewer remain
on their existing integrations.

## Editing menus and command bars

Edit `src/menu/definition.rs`: `ROOTS` sets the category order, caption and ordered
entries; `submenu_items` describes all nested manager menus. `EDITOR_ROOTS`
defines editor categories, their enabled state, entries and separators.
`EDITOR_BAR` and `VIEWER_BAR` define the bottom command bars, including captions,
explicit shortcut labels, actions, gaps and widths. The viewer retains its command
bar without gaining a dropdown menu. Moving an entry preserves its existing key;
blank editor function-key slots stay explicit entries. `item(caption, shortcut,
action)` adds a command or submenu link, and `line()` places a separator. Captions
use the existing `~mnemonic~` syntax and remain localization keys; add translations
to the language catalogs when introducing new captions. Shortcut text describes
existing input bindings; changing a displayed label does not rebind a key.

Named `RootId` values keep F9, sort shortcuts and plugin registration independent
of category positions. `EditorRootId` similarly keeps F10 opening File regardless
of the editor category order; Tab/Shift+Tab derive the enabled categories from
the same definition. Reordering an existing category or moving a command among
root menus requires no changes to input or rendering code. Command execution stays
in `menu::execute`; adding new behavior still requires its implementation.

## Adding another built-in tool

1. Add a module implementing `ToolPlugin`, with its own state and local action IDs.
2. Register its constructor in `Runtime::default`; use the existing dialog and
   scene primitives from its view implementation.
3. Declare its menu entry and presentation. No new central Focus, WindowKind,
   mouse-target or command variant is needed: Plugin is the shared route.

The test-only counter tool verifies generic registration, duplicate rejection,
menu activation, keyboard/paste/mouse routing, nested Help, immutable translated
painting, document reuse and retirement. Calculator/Tetris tests retain their
original behavior checks, including Unicode, history, page focus, physics, score
failure recovery and cancellation.

## Validation

macOS ARM64: 525 native Rust tests pass; the existing opt-in native Trash test is
ignored. Formatting, all-target Clippy, release and no-default-features checks pass.
All 128 pre/post cell/style captures match across both palettes/languages and two
terminal sizes. Release PTYs cover module/window switching, inactive gravity,
calculator state, owned Help/paste isolation, native controls, score-name
cancellation, resize and terminal restoration. Plugins settings additionally cover
draft cancellation, registry-driven lists, long-list scrolling/stale targets,
disable/re-enable, owned Help retirement and configuration safeguards. A release
PTY verifies toggles, blocked shortcuts, live game retirement, explicit saving,
resize and terminal restoration. Windows/Linux validation is skipped.
