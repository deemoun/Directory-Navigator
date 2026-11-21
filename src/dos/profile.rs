//! Typed, shared startup overrides. Only allowlisted configuration is generated.
use super::{Kind, Verified};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuSpeed {
    #[default]
    Emulator,
    Fixed,
    Maximum,
}
impl CpuSpeed {
    pub fn key(self) -> &'static str {
        match self {
            Self::Emulator => "emulator",
            Self::Fixed => "fixed",
            Self::Maximum => "maximum",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Emulator => "Use emulator settings",
            Self::Fixed => "Fixed",
            Self::Maximum => "Maximum",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Emulator, Self::Fixed, Self::Maximum]
            .into_iter()
            .find(|v| v.key() == s)
    }
    pub fn next(self) -> Self {
        match self {
            Self::Emulator => Self::Fixed,
            Self::Fixed => Self::Maximum,
            Self::Maximum => Self::Emulator,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayMode {
    #[default]
    Emulator,
    Windowed,
    Fullscreen,
}
impl DisplayMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Emulator => "emulator",
            Self::Windowed => "windowed",
            Self::Fullscreen => "fullscreen",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Emulator => "Use emulator settings",
            Self::Windowed => "Windowed",
            Self::Fullscreen => "Fullscreen",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Emulator, Self::Windowed, Self::Fullscreen]
            .into_iter()
            .find(|v| v.key() == s)
    }
    pub fn next(self) -> Self {
        match self {
            Self::Emulator => Self::Windowed,
            Self::Windowed => Self::Fullscreen,
            Self::Fullscreen => Self::Emulator,
        }
    }
}
/// Requested window bounds; Automatic inherits the emulator configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Resolution {
    #[default]
    Automatic,
    R640x480,
    R800x600,
    R1024x768,
    R1280x960,
    R1600x1200,
    R1920x1080,
}
impl Resolution {
    pub const ALL: [Self; 7] = [
        Self::Automatic,
        Self::R640x480,
        Self::R800x600,
        Self::R1024x768,
        Self::R1280x960,
        Self::R1600x1200,
        Self::R1920x1080,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::R640x480 => "640x480",
            Self::R800x600 => "800x600",
            Self::R1024x768 => "1024x768",
            Self::R1280x960 => "1280x960",
            Self::R1600x1200 => "1600x1200",
            Self::R1920x1080 => "1920x1080",
        }
    }
    pub fn label(self) -> &'static str {
        if self == Self::Automatic {
            "Automatic"
        } else {
            self.key()
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == s)
    }
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}
pub const MEMORY: [u8; 8] = [0, 1, 2, 4, 8, 16, 32, 63];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub cpu: CpuSpeed,
    pub cycles: u32,
    pub memory_mb: u8,
    pub display: DisplayMode,
    pub resolution: Resolution,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            cpu: CpuSpeed::Emulator,
            cycles: 3000,
            memory_mb: 0,
            display: DisplayMode::Emulator,
            resolution: Resolution::Automatic,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> io::Result<()> {
        if !(50..=2_000_000).contains(&self.cycles) || !MEMORY.contains(&self.memory_mb) {
            return Err(io::Error::other("Invalid DOS settings: cycles must be 50–2,000,000 and memory must use a listed value."));
        }
        Ok(())
    }
    pub fn inherited(&self) -> bool {
        self.cpu == CpuSpeed::Emulator
            && self.memory_mb == 0
            && self.display == DisplayMode::Emulator
            && self.resolution == Resolution::Automatic
    }
    pub fn summary(&self, language: crate::localization::Language) -> String {
        let cpu = if self.cpu == CpuSpeed::Fixed {
            format!("{} {}", language.text("Fixed"), self.cycles)
        } else {
            language.text(self.cpu.label()).into()
        };
        let memory = if self.memory_mb == 0 {
            language.text("Use emulator settings").into()
        } else {
            format!("{} MiB", self.memory_mb)
        };
        format!(
            "{}: {} · {}: {} · {}: {} · {}: {}",
            language.text("CPU speed"),
            cpu,
            language.text("Memory"),
            memory,
            language.text("Display"),
            language.text(self.display.label()),
            language.text("Window resolution"),
            language.text(self.resolution.label())
        )
    }
    pub(super) fn render(&self, verified: &Verified) -> io::Result<String> {
        self.validate()?;
        let mut output = String::new();
        if self.cpu != CpuSpeed::Emulator {
            let value = if self.cpu == CpuSpeed::Fixed {
                self.cycles.to_string()
            } else {
                "max".into()
            };
            output.push_str("[cpu]\n");
            if verified.kind == Kind::Staging {
                let version=version_pair(&verified.version).ok_or_else(||io::Error::other("Cannot verify CPU overrides for this DOSBox Staging version. Check DOSBox or choose Use emulator settings."))?;
                if version >= (0, 82) {
                    output.push_str(&format!(
                        "cpu_cycles={value}\ncpu_cycles_protected={}\n",
                        if self.cpu == CpuSpeed::Fixed {
                            "auto"
                        } else {
                            "max"
                        }
                    ));
                } else {
                    output.push_str(&format!("cycles={value}\n"));
                }
            } else {
                output.push_str(&format!("cycles={value}\n"));
            }
        }
        if self.memory_mb != 0 {
            output.push_str(&format!("[dosbox]\nmemsize={}\n", self.memory_mb));
        }
        if self.display != DisplayMode::Emulator || self.resolution != Resolution::Automatic {
            output.push_str("[sdl]\n");
            if self.display != DisplayMode::Emulator {
                output.push_str(&format!(
                    "fullscreen={}\n",
                    self.display == DisplayMode::Fullscreen
                ));
            }
            if self.resolution != Resolution::Automatic {
                let key = if verified.kind == Kind::Staging {
                    let version = version_pair(&verified.version).ok_or_else(|| io::Error::other(
                        "Cannot verify window resolution for this DOSBox Staging version. Check DOSBox or choose Automatic."))?;
                    if version >= (0, 83) {
                        "window_size"
                    } else {
                        "windowresolution"
                    }
                } else {
                    "windowresolution"
                };
                output.push_str(&format!("{key}={}\n", self.resolution.key()));
            }
        }
        Ok(output)
    }
}
fn version_pair(s: &str) -> Option<(u32, u32)> {
    s.split(|c: char| !c.is_ascii_digit() && c != '.')
        .find_map(|word| {
            let mut parts = word.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            Some((major, minor))
        })
}
pub(super) struct Fragment {
    workspace: crate::network::workspace::Workspace,
    pub path: PathBuf,
}
impl Fragment {
    pub fn create(contents: &str) -> io::Result<Self> {
        let workspace = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let path = workspace.root.join("dn-dos.conf");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        Ok(Self { workspace, path })
    }
    pub fn preserve(self) {
        self.workspace.preserve();
    }
}
pub(super) fn arguments(verified: &Verified, path: &Path) -> Vec<std::ffi::OsString> {
    let mut args: Vec<_> = super::flags(verified).into_iter().map(Into::into).collect();
    args.push(
        if verified.kind == Kind::Staging && verified.flag.starts_with("--") {
            "--conf"
        } else {
            "-conf"
        }
        .into(),
    );
    args.push(path.as_os_str().to_owned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    fn verified(kind: Kind, version: &str) -> Verified {
        Verified {
            kind,
            version: version.into(),
            flag: if kind == Kind::Staging {
                "--nolocalconf"
            } else {
                ""
            },
        }
    }
    #[test]
    fn inherited_profiles_generate_nothing_and_explicit_settings_use_the_right_dialect() {
        for kind in [Kind::Classic, Kind::X, Kind::Staging] {
            let v = verified(kind, "unknown");
            assert_eq!(Profile::default().render(&v).unwrap(), "");
        }
        let p = Profile {
            cpu: CpuSpeed::Fixed,
            cycles: 25000,
            memory_mb: 32,
            display: DisplayMode::Windowed,
            resolution: Resolution::Automatic,
        };
        for (kind, version, modern) in [
            (Kind::Classic, "DOSBox 0.74-3", false),
            (Kind::X, "DOSBox-X 2026.01.02", false),
            (Kind::Staging, "DOSBox Staging 0.81.2", false),
            (Kind::Staging, "DOSBox Staging 0.82.0", true),
            (Kind::Staging, "DOSBox Staging 0.83.1", true),
        ] {
            let text = p.render(&verified(kind, version)).unwrap();
            assert!(text.contains("memsize=32\n"));
            assert!(text.contains("fullscreen=false\n"));
            if modern {
                assert!(text.contains("cpu_cycles=25000\ncpu_cycles_protected=auto\n"));
            } else {
                assert!(text.contains("\ncycles=25000\n"));
            }
            assert!(!text.contains("autoexec"));
            assert!(!text.contains("mount"));
        }
        assert!(p
            .render(&verified(Kind::Staging, "Version unavailable"))
            .is_err());
        for cycles in [0, 49, 2_000_001, u32::MAX] {
            assert!(Profile {
                cycles,
                ..p.clone()
            }
            .validate()
            .is_err());
        }
        assert!(Profile {
            memory_mb: 64,
            ..p.clone()
        }
        .validate()
        .is_err());
        let text = Profile {
            cpu: CpuSpeed::Maximum,
            ..p
        }
        .render(&verified(Kind::Staging, "0.83.0"))
        .unwrap();
        assert!(text.contains("cpu_cycles=max\ncpu_cycles_protected=max"));
    }
    #[test]
    fn window_resolution_is_inherited_or_rendered_for_each_emulator_version() {
        assert_eq!(Profile::default().resolution, Resolution::Automatic);
        for resolution in Resolution::ALL {
            assert_eq!(Resolution::parse(resolution.key()), Some(resolution));
            for (kind, version, key) in [
                (Kind::Classic, "DOSBox 0.74-3", "windowresolution"),
                (Kind::X, "DOSBox-X 2026.10.01", "windowresolution"),
                (Kind::Staging, "DOSBox Staging 0.81.2", "windowresolution"),
                (Kind::Staging, "DOSBox Staging 0.82.2", "windowresolution"),
                (Kind::Staging, "DOSBox Staging 0.83.0", "window_size"),
            ] {
                let profile = Profile {
                    resolution,
                    ..Default::default()
                };
                let text = profile.render(&verified(kind, version)).unwrap();
                if resolution == Resolution::Automatic {
                    assert!(profile.inherited());
                    assert!(text.is_empty());
                } else {
                    assert!(!profile.inherited());
                    assert_eq!(text, format!("[sdl]\n{key}={}\n", resolution.key()));
                }
                assert!(!text.contains("fullresolution"));
                assert!(!text.contains("output="));
            }
        }
        assert_eq!(Resolution::R1920x1080.next(), Resolution::Automatic);
        assert!(Resolution::parse("123x456").is_none());
        let profile = Profile {
            resolution: Resolution::R800x600,
            ..Default::default()
        };
        assert!(profile.render(&verified(Kind::Staging, "unknown")).is_err());
    }

    #[test]
    fn override_arguments_keep_order_and_native_paths_and_cleanup_is_owned() {
        let fragment = Fragment::create("[sdl]\nfullscreen=false\n").unwrap();
        let path = fragment.path.clone();
        for kind in [Kind::Classic, Kind::X, Kind::Staging] {
            let v = verified(kind, "0.83.0");
            let args = arguments(&v, &path);
            assert_eq!(args.last().unwrap(), path.as_os_str());
            let prefix: Vec<std::ffi::OsString> = super::super::flags(&v)
                .into_iter()
                .map(Into::into)
                .collect();
            assert_eq!(&args[..prefix.len()], &prefix);
            let program = Path::new("DOS app ; $(literal) 世界.COM");
            let command = super::super::process::command(Path::new("dosbox"), program, &args);
            assert_eq!(command.get_args().last().unwrap(), program.as_os_str());
        }
        drop(fragment);
        assert!(!path.exists());
    }
}
