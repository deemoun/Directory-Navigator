use crossterm::event::{Event, MouseEventKind};
#[cfg(not(feature = "turbo-vision"))]
use dn::ui;
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
    let (session, mut terminal) = Session::start()?;
    #[cfg(feature = "turbo-vision")]
    terminal.bind_desktop(app.desktop.native());
    let mut dirty = true;
    loop {
        dirty |= app.poll();
        #[cfg(feature = "turbo-vision")]
        app.desktop
            .native()
            .set_scope(dn::turbo::desktop::Scope::for_app(&app));
        if dirty {
            session.cursor_style(dn::focus::current(&app) == dn::focus::Focus::Manager)?;
            #[cfg(feature = "turbo-vision")]
            terminal.draw_app(&mut app)?;
            #[cfg(not(feature = "turbo-vision"))]
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            dirty = false;
        }
        if app.quit {
            if app.save_settings_on_exit() {
                break;
            }
            dirty = true;
            continue;
        }
        if let Some(plan) = app
            .user_menu
            .pending
            .take()
            .or_else(|| app.associations.pending.take())
            .or_else(|| app.command_line.pending.take())
        {
            session.suspend();
            let result = dn::user_menu::runner::run(&plan);
            match &result {
                Ok(status) => println!("\nCommand finished: {status}"),
                Err(e) => println!("\nCannot run command: {e}"),
            }
            println!("Press Enter to return to DN.");
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line);
            session.resume(&mut terminal)?;
            app.refresh();
            if let Err(e) = result {
                app.error(e.to_string());
            }
            dirty = true;
            continue;
        }
        let timeout = app.wait_duration(std::time::Instant::now());
        if let Some(event) = session.poll(&mut terminal, timeout)? {
            if dn::screensaver::event(&mut app, &event, std::time::Instant::now()) {
                dirty = true;
                continue;
            }
            if let Event::Mouse(mouse) = &event {
                session.mouse_event(mouse.kind);
            }
            #[cfg(feature = "turbo-vision")]
            if app.desktop.native().dispatch().is_none() {
                continue;
            }
            match event {
                Event::Key(key) => {
                    input::handle(&mut app, key);
                    dirty = true;
                }
                Event::Resize(_, _) => {
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
                    mouse::handle(&mut app, event);
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
