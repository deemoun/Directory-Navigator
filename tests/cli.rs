use std::{
    fs, io,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn version_flags_and_help_are_noninteractive_and_do_not_touch_settings() -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = Temp(std::env::temp_dir().join(format!(
        "dn-cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&temp.0)?;
    let blocked = temp.0.join("not-a-config-directory");
    fs::write(&blocked, "unchanged")?;
    assert_eq!(dn::DISPLAY_VERSION, env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V", "--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dn"))
            .arg(flag)
            .current_dir(&temp.0)
            .env("XDG_CONFIG_HOME", &blocked)
            .env("APPDATA", &blocked)
            .env("HOME", &temp.0)
            .stdin(Stdio::null())
            .output()?;
        assert!(output.status.success(), "{flag}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\u{1b}'));
        let identity = format!("Directory Navigator {}\n", env!("CARGO_PKG_VERSION"));
        if matches!(flag, "--version" | "-V") {
            assert_eq!(text, identity);
        } else {
            assert!(text.starts_with(&identity));
            assert!(text.contains("-V, --version"));
        }
        assert_eq!(fs::read_to_string(&blocked)?, "unchanged");
        assert_eq!(fs::read_dir(&temp.0)?.count(), 1);
    }
    Ok(())
}
