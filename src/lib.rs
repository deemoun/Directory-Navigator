pub const APPLICATION_NAME: &str = "Directory Navigator";
pub const DISPLAY_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod app;
pub mod associations;
pub mod columns;
pub mod commands;
pub mod config;
pub mod config_import;
pub mod desktop;
pub mod dialog;
pub(crate) mod dialog_adapter;
pub(crate) mod dialog_controls;
pub(crate) mod dialog_theme;
pub mod editor;
pub mod file_info;
pub mod file_tools;
pub mod filesystem;
pub mod formatting;
pub mod input;
pub mod menu;
pub mod mouse;
pub mod operations;
pub mod panel;
pub mod panel_layout;
pub mod pattern;
#[doc(hidden)]
pub mod plugins;
pub mod quick_search;
pub mod settings;
pub mod shortcuts;
pub mod terminal;
pub mod tetris;
pub mod ui;
pub mod viewer;
pub mod welcome;

pub mod utilities;

pub mod bookmarks;
pub mod navigation_tools;

pub mod clock_view;
pub mod palette;
pub mod screensaver;

pub mod focus;

pub(crate) mod history;
pub mod layout;
pub(crate) mod list_state;

pub mod task;

pub mod discovery;
pub mod launcher;
pub mod platform_fs;
pub mod publication;
pub mod quick_view;
pub mod trash_support;
pub mod traversal;

pub mod directory_tree;

pub mod user_menu;

pub mod archive;

pub mod command_line;

pub mod localization;
pub mod regional;

pub mod volume_space;

pub mod windowing;

pub mod turbo;

pub mod panel_status;

pub mod panel_command;

pub mod painting;

pub mod presentation;
#[doc(hidden)]
pub mod testing;

pub mod control_focus;

pub mod requests;

pub mod modals;

pub mod copy_dialog;

pub mod terminal_output;

pub(crate) mod eligibility;
pub(crate) mod retirement;
