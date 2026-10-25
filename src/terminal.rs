use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        MouseEventKind,
    },
    execute,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use std::{
    cell::Cell,
    io::{self, IsTerminal, Stdout},
    time::{Duration, Instant},
};

pub type Screen = crate::turbo::TurboBackend<Stdout>;

pub struct Session {
    mouse_down: Cell<bool>,
}

pub fn restore() {
    restore_input(false);
}
fn restore_input(mouse_down: bool) {
    if mouse_down {
        // Keep capture and raw input active until the pending button is released.
        // Disabling reports first can suppress that release, while a short deadline
        // lets delayed/fragmented reports spill into the parent shell under load.
        // Bound the wait in case the terminal loses the release entirely.
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            match event::poll(deadline.saturating_duration_since(Instant::now())) {
                Ok(true) => match event::read() {
                    Ok(event::Event::Mouse(event))
                        if matches!(event.kind, MouseEventKind::Up(_)) =>
                    {
                        break
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                _ => break,
            }
        }
    }
    // Stop new reports before restoring canonical input and shell echo.
    let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    if mouse_down {
        // Also discard a fragmented in-flight report which the event parser
        // cannot yet decode. Only flush for a mouse-initiated handoff.
        #[cfg(unix)]
        unsafe {
            libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
        }
    }
    let _ = execute!(
        io::stdout(),
        SetAttribute(Attribute::Reset),
        ResetColor,
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape,
        Show
    );
    let _ = disable_raw_mode();
}

impl Session {
    pub fn poll(&self, screen: &mut Screen, timeout: Duration) -> io::Result<Option<event::Event>> {
        screen.poll_event(timeout)
    }

    pub fn mouse_event(&self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::Down(_) => self.mouse_down.set(true),
            MouseEventKind::Up(_) => self.mouse_down.set(false),
            _ => {}
        }
    }
    pub fn cursor_style(&self, command: bool) -> io::Result<()> {
        execute!(
            io::stdout(),
            if command {
                SetCursorStyle::BlinkingBar
            } else {
                SetCursorStyle::DefaultUserShape
            }
        )
    }
    pub fn suspend(&self) {
        restore_input(self.mouse_down.replace(false));
    }
    pub fn resume(&self, screen: &mut Screen) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        screen.clear()?;
        Ok(())
    }

    pub fn start() -> io::Result<(Self, Screen)> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other("dn requires an interactive terminal"));
        }
        let previous = std::panic::take_hook();
        let terminal_thread = std::thread::current().id();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().id() == terminal_thread {
                restore();
            }
            previous(info);
        }));
        let guard = Self {
            mouse_down: Cell::new(false),
        };
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        let screen = crate::turbo::TurboBackend::new(io::stdout())?;

        Ok((guard, screen))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        restore_input(self.mouse_down.replace(false));
    }
}
