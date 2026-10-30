use crossterm::event::{Event, MouseEventKind};

use dn::{app::App, input, mouse, terminal::Session};
use std::io;

fn run() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if let Some(arg) = args.next() {
        if arg == "--version" || arg == "-V" {
            println!("{} {}", dn::APPLICATION_NAME, dn::DISPLAY_VERSION);
            return Ok(());
        }
        if arg == "--help" || arg == "-h" {
            println!(
                "{} {}\n\nUsage: dn [--help | --version]\n  -h, --help     Show help\n  -V, --version  Show version\nRun from the directory to browse. F1 shows keyboard help.",
                dn::APPLICATION_NAME,
                dn::DISPLAY_VERSION
            );
            return Ok(());
        }
        return Err(io::Error::other(
            "Unknown argument. Usage: dn [--help | --version]",
        ));
    }
    let mut app = App::new(std::env::current_dir()?)?;
    app.load_initial_config();
    match app
        .config_store
        .path()
        .map(|p| p.with_file_name("editor-recovery"))
    {
        Ok(root) => dn::editor::recovery::start(&mut app, root),
        Err(e) => app.error(format!("Editor recovery unavailable: {e}")),
    }
    let (session, mut terminal) = Session::start()?;
    terminal.bind_desktop(app.desktop.native());
    let mut dirty = true;
    let mut showing_output = false;
    let mut exit_settings_saved = false;
    loop {
        dirty |= app.poll();
        if app.output_visible != showing_output {
            if app.output_visible {
                session.reveal_output()?;
            } else {
                session.resume(&mut terminal)?;
                dirty = true;
            }
            showing_output = app.output_visible;
        }
        app.desktop
            .native()
            .set_scope(dn::turbo::desktop::Scope::for_app(&app));
        if dirty && !showing_output {
            session.cursor_style(dn::focus::current(&app) == dn::focus::Focus::Manager)?;
            terminal.draw_app(&mut app)?;

            dirty = false;
        }
        if !app.quit {
            exit_settings_saved = false;
        }
        if app.quit {
            if !exit_settings_saved {
                exit_settings_saved = app.save_settings_on_exit();
            }
            if exit_settings_saved && dn::editor::recovery::finish(&mut app) {
                break;
            }
            dirty = true;
            continue;
        }
        let pending = if showing_output {
            None
        } else {
            app.user_menu
                .pending
                .take()
                .or_else(|| app.associations.pending.take())
                .or_else(|| app.command_line.pending.take())
        };
        if let Some(plan) = pending {
            dn::editor::recovery::checkpoint_now(&mut app);
            session.suspend();
            let result = dn::user_menu::runner::run(&plan);
            match &result {
                Ok(status) => println!("\nCommand finished: {status}"),
                Err(e) => println!("\nCannot run command: {e}"),
            }
            session.prepare_output_input()?;
            println!("Press Enter to return to DN. Esc / Ctrl+O also return.\r");
            app.output_visible = true;
            showing_output = true;
            app.screensaver.activity(std::time::Instant::now());
            app.refresh();
            if let Err(e) = result {
                app.error(e.to_string());
            }
            dirty = true;
            continue;
        }
        let timeout = app.wait_duration(std::time::Instant::now());
        if let Some(event) = session.poll(&mut terminal, timeout)? {
            if showing_output {
                dn::terminal_output::event(&mut app, &event);
                continue;
            }
            if dn::screensaver::event(&mut app, &event, std::time::Instant::now()) {
                dirty = true;
                continue;
            }
            if let Event::Mouse(mouse) = &event {
                session.mouse_event(mouse.kind);
            }
            if app.desktop.native().dispatch().is_none_or(|receipt| {
                receipt.window != app.desktop.active()
                    || receipt.scope != dn::turbo::desktop::Scope::for_app(&app)
            }) {
                continue;
            }
            match event {
                Event::Key(key) => {
                    input::handle_ui(&mut app, key);
                    dirty = true;
                }
                Event::Resize(_, _) => {
                    input::cancel_feedback(&mut app);
                    app.shortcuts.clear();
                    app.mouse.scrollbar_drag = None;
                    dirty = true;
                }
                Event::Paste(text) => {
                    input::paste(&mut app, &text);
                    dirty = true;
                }
                Event::Mouse(event)
                    if matches!(
                        event.kind,
                        MouseEventKind::Down(_)
                            | MouseEventKind::Drag(crossterm::event::MouseButton::Left)
                            | MouseEventKind::Up(crossterm::event::MouseButton::Left)
                            | MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                    ) =>
                {
                    mouse::handle_ui(&mut app, event);
                    dirty = true;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("dn: {error}");
        std::process::exit(1);
    }
}
