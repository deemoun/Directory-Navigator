pub const APPLICATION_NAME: &str = "Directory Navigator";
pub const DISPLAY_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod app;
pub mod associations;
pub mod columns;
pub mod config;
pub mod dialog;
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

pub mod layout;

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
