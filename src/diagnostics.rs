//! Opt-in terminal-test diagnostics. No output or file access when disabled.
use crossterm::event::{Event, KeyCode};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Seek, SeekFrom, Write},
    time::Instant,
};

pub struct Recorder {
    file: Option<File>,
    events: VecDeque<String>,
    start: Instant,
}
impl Recorder {
    fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<File> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    }
    pub fn from_env() -> Self {
        let file = std::env::var_os("DN_TEST_DIAGNOSTICS").and_then(|path| Self::open(path).ok());
        Self {
            file,
            events: VecDeque::new(),
            start: Instant::now(),
        }
    }
    pub fn event(&mut self, event: &Event) {
        if self.file.is_none() {
            return;
        }
        let description = match event {
            Event::Key(k) => match k.code {
                KeyCode::Char(_) => format!("character {:?} {:?}", k.modifiers, k.kind),
                code => format!("key {code:?} {:?} {:?}", k.modifiers, k.kind),
            },
            Event::Paste(text) => format!("paste bytes={}", text.len()),
            other => format!("{other:?}"),
        };
        if self.events.len() == 24 {
            self.events.pop_front();
        }
        self.events.push_back(format!(
            "{}ms {description}",
            self.start.elapsed().as_millis()
        ));
    }
    pub fn snapshot(&mut self, app: &dn::app::App) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let state = format!(
            "{}\nrecent events:\n{}\n",
            dn::testing::diagnostic_state(app),
            self.events.iter().cloned().collect::<Vec<_>>().join("\n")
        );
        let result = (|| -> std::io::Result<()> {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(state.as_bytes())?;
            file.set_len(state.len() as u64)
        })();
        if result.is_err() {
            self.file = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_bounded_redacted_and_never_replace_an_existing_file() -> std::io::Result<()>
    {
        let root = std::env::temp_dir().join(format!(
            "dn-diagnostics-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root)?;
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temp(root);
        let path = temp.0.join("diagnostics");
        {
            let mut recorder = Recorder {
                file: Some(Recorder::open(&path)?),
                events: VecDeque::new(),
                start: Instant::now(),
            };
            let mut app = dn::app::App::new(temp.0.clone())?;
            dn::editor::open(&mut app, true);
            dn::editor::paste(&mut app, "PRIVATE_BUFFER");
            for _ in 0..30 {
                recorder.event(&Event::Paste("PRIVATE_PASTE".into()));
            }
            recorder.event(&Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char('界'),
                crossterm::event::KeyModifiers::NONE,
            )));
            assert_eq!(recorder.events.len(), 24);
            recorder.snapshot(&app);
            let text = std::fs::read_to_string(&path)?;
            assert!(text.contains("paste bytes=13") && text.contains("character"));
            for private in [
                "PRIVATE_BUFFER",
                "PRIVATE_PASTE",
                "界",
                temp.0.to_str().unwrap(),
            ] {
                assert!(!text.contains(private));
            }
            assert!(Recorder::open(&path).is_err());
            assert_eq!(std::fs::read_to_string(&path)?, text);
            recorder.events.clear();
            recorder.snapshot(&app);
            assert!(std::fs::metadata(&path)?.len() < text.len() as u64);
        }
        Ok(())
    }
}
