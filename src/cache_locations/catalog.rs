//! A small OS catalog. No application cleanup recipes and no mutation.
use super::{Location, Status};
use crate::{platform_fs::entry_identity, task::Cancellation};
use std::{fs, path::PathBuf};
#[derive(Clone, Copy)]
#[allow(dead_code)] // All platforms are exercised by portable resolver tests.
pub(super) enum Platform {
    Mac,
    Linux,
    Windows,
}
#[derive(Default)]
pub(super) struct Inputs {
    pub home: Option<PathBuf>,
    pub temp: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub local: Option<PathBuf>,
    pub windows: Option<PathBuf>,
    pub developer: DeveloperHomes,
}
#[derive(Default)]
pub(super) struct DeveloperHomes {
    pub cargo: Option<PathBuf>,
    pub rustup: Option<PathBuf>,
    pub gradle: Option<PathBuf>,
    pub npm: Option<PathBuf>,
    pub android: Option<PathBuf>,
    pub avd: Option<PathBuf>,
}
fn absolute(path: Option<PathBuf>) -> Option<PathBuf> {
    path.filter(|p| p.is_absolute() && p.parent().is_some())
}
pub(super) fn candidates(platform: Platform, input: Inputs) -> Vec<Location> {
    let home = absolute(input.home);
    let mut rows = Vec::new();
    let mut add = |name, purpose, path| rows.push(Location::new(name, purpose, absolute(path)));
    match platform {
        Platform::Mac => {
            add(
                "User caches",
                "Re-creatable application caches; applications may be using them.",
                home.as_ref().map(|p| p.join("Library/Caches")),
            );
            add(
                "User logs",
                "Diagnostic logs; they can be useful for troubleshooting.",
                home.as_ref().map(|p| p.join("Library/Logs")),
            );
            add(
                "System caches",
                "Shared system caches; inspection may require additional access.",
                Some("/Library/Caches".into()),
            );
            add(
                "System logs",
                "Diagnostic logs; they can be useful for troubleshooting.",
                Some("/Library/Logs".into()),
            );
        }
        Platform::Linux => {
            add(
                "User caches",
                "Re-creatable application caches; applications may be using them.",
                absolute(input.cache).or_else(|| home.as_ref().map(|p| p.join(".cache"))),
            );
            add(
                "System caches",
                "Shared system caches; inspection may require additional access.",
                Some("/var/cache".into()),
            );
        }
        Platform::Windows => {
            add(
                "User temporary files",
                "Temporary working files; running applications may need them.",
                absolute(input.local).map(|p| p.join("Temp")),
            );
            add(
                "Windows temporary files",
                "Temporary working files; running applications may need them.",
                absolute(input.windows.clone()).map(|p| p.join("Temp")),
            );
            add(
                "System logs",
                "Diagnostic logs; they can be useful for troubleshooting.",
                absolute(input.windows).map(|p| p.join("Logs")),
            );
        }
    }
    add(
        "Current temporary directory",
        "Temporary working files; running applications may need them.",
        input.temp,
    );
    if !matches!(platform, Platform::Windows) {
        add(
            "System temporary files",
            "Temporary working files; running applications may need them.",
            Some("/tmp".into()),
        );
        add(
            "Persistent temporary files",
            "Temporary files that may remain across restarts.",
            Some("/var/tmp".into()),
        );
        add(
            "System diagnostic logs",
            "Diagnostic logs; they can be useful for troubleshooting.",
            Some("/var/log".into()),
        );
    }
    if matches!(platform, Platform::Mac) {
        // Only named developer locations: never walk HOME to discover projects.
        let defaults = [
            ("cargo", ".cargo"),
            ("rustup", ".rustup"),
            ("gradle", ".gradle"),
            ("npm", ".npm"),
            ("android", "Library/Android/sdk"),
            ("avd", ".android/avd"),
        ];
        let overrides = [
            input.developer.cargo,
            input.developer.rustup,
            input.developer.gradle,
            input.developer.npm,
            input.developer.android,
            input.developer.avd,
        ];
        let roots: Vec<_> = defaults
            .into_iter()
            .zip(overrides)
            .map(|((name, suffix), value)| {
                (
                    name,
                    absolute(value).or_else(|| home.as_ref().map(|p| p.join(suffix))),
                )
            })
            .collect();
        add(
            "Xcode build output",
            "Rebuildable Xcode build products and indexes; active builds may need them.",
            home.as_ref()
                .map(|p| p.join("Library/Developer/Xcode/DerivedData")),
        );
        add(
            "Xcode archives (valuable)",
            "Release archives and debug symbols; valuable developer data, not disposable cache.",
            home.as_ref()
                .map(|p| p.join("Library/Developer/Xcode/Archives")),
        );
        add(
            "iOS device support",
            "Installed device debugging support and symbols; debugging may require these files.",
            home.as_ref()
                .map(|p| p.join("Library/Developer/Xcode/iOS DeviceSupport")),
        );
        add(
            "Simulator devices (data)",
            "Simulator devices, applications and user data; may contain valuable unsaved work.",
            home.as_ref()
                .map(|p| p.join("Library/Developer/CoreSimulator/Devices")),
        );
        add(
            "Simulator caches",
            "Simulator caches; running simulators may be using them.",
            home.as_ref()
                .map(|p| p.join("Library/Developer/CoreSimulator/Caches")),
        );
        add(
            "Shared simulator assets",
            "Installed simulator runtimes and assets; other mounted filesystems are excluded.",
            Some(PathBuf::from("/Library/Developer/CoreSimulator")),
        );
        add(
            "Cargo registry cache",
            "Downloaded Rust dependencies and registry metadata; offline builds may need them.",
            roots
                .iter()
                .find(|(name, _)| *name == "cargo")
                .and_then(|(_, path)| path.clone())
                .map(|p| p.join("registry")),
        );
        add(
            "Cargo Git cache",
            "Downloaded Git dependencies; offline builds may need these checkouts.",
            roots
                .iter()
                .find(|(name, _)| *name == "cargo")
                .and_then(|(_, path)| path.clone())
                .map(|p| p.join("git")),
        );
        add(
            "Rust toolchains (installed)",
            "Installed Rust compilers and tools; development depends on these installations.",
            roots
                .iter()
                .find(|(name, _)| *name == "rustup")
                .and_then(|(_, path)| path.clone())
                .map(|p| p.join("toolchains")),
        );
        add(
            "Gradle caches",
            "Gradle dependency and build caches; offline builds may require them.",
            roots
                .iter()
                .find(|(name, _)| *name == "gradle")
                .and_then(|(_, path)| path.clone())
                .map(|p| p.join("caches")),
        );
        add(
            "Gradle distributions",
            "Downloaded Gradle Wrapper versions; projects may require these installations.",
            roots
                .iter()
                .find(|(name, _)| *name == "gradle")
                .and_then(|(_, path)| path.clone())
                .map(|p| p.join("wrapper/dists")),
        );
        add("npm cache", "Downloaded npm packages; configured locations outside the environment are not inspected.", roots.iter().find(|(name, _)| *name == "npm").and_then(|(_, path)| path.clone()));
        add("Maven repository (default)", "Default local Maven repository; may include locally published artifacts unavailable elsewhere.", home.as_ref().map(|p| p.join(".m2/repository")));
        add(
            "Android SDK (installed)",
            "Installed Android SDK tools and platforms; development depends on these files.",
            roots
                .iter()
                .find(|(name, _)| *name == "android")
                .and_then(|(_, path)| path.clone()),
        );
        add(
            "Android virtual devices (data)",
            "Virtual devices and their application/user data; valuable data, not disposable cache.",
            roots
                .iter()
                .find(|(name, _)| *name == "avd")
                .and_then(|(_, path)| path.clone()),
        );
        add(
            "pip cache (default)",
            "Default pip download and wheel cache; custom configuration is not inspected.",
            home.as_ref().map(|p| p.join("Library/Caches/pip")),
        );
        add(
            "uv cache (default)",
            "Default uv package cache; custom configuration is not inspected.",
            home.as_ref().map(|p| p.join("Library/Caches/uv")),
        );
        add(
            "Applications (installed)",
            "Installed applications, not disposable cache; inspect only, without uninstalling.",
            Some(PathBuf::from("/Applications")),
        );
        add(
            "User applications (installed)",
            "Installed applications in your home directory, not disposable cache.",
            home.as_ref().map(|p| p.join("Applications")),
        );
        add("Shared application data", "Shared application resources and potentially valuable data; applications may require them.", Some(PathBuf::from("/Library/Application Support")));
        add(
            "User application data",
            "Application resources and potentially valuable user data; not disposable cache.",
            home.as_ref().map(|p| p.join("Library/Application Support")),
        );
        add("App containers (user data)", "Sandboxed application settings and potentially valuable user data; not disposable cache.", home.as_ref().map(|p| p.join("Library/Containers")));
        add(
            "Group containers (user data)",
            "Shared sandbox settings and potentially valuable user data; not disposable cache.",
            home.as_ref().map(|p| p.join("Library/Group Containers")),
        );
        add("Device backups (valuable)", "Local iPhone/iPad backups; valuable device data, also included under User application data.", home.as_ref().map(|p| p.join("Library/Application Support/MobileSync/Backup")));
    }
    if matches!(platform, Platform::Linux) {
        rows[0].cleanup = true;
    }
    rows
}
pub(super) fn resolve(mut rows: Vec<Location>, cancel: &Cancellation) -> Vec<Location> {
    let mut seen = Vec::new();
    rows.retain_mut(|row| {
        if cancel.cancelled() {
            return false;
        }
        let Some(path) = &row.path else {
            return true;
        };
        let result = (|| {
            let canonical = fs::canonicalize(path)?;
            if canonical.parent().is_none() {
                return Err(std::io::Error::other(
                    "A filesystem root is not a cache location",
                ));
            }
            let metadata = fs::symlink_metadata(&canonical)?;
            if !metadata.is_dir() || crate::platform_fs::directory_link(&metadata) {
                return Err(std::io::Error::other("Location is not a real directory"));
            }
            let identity = entry_identity(&canonical)?;
            fs::read_dir(&canonical)?;
            if entry_identity(&canonical)? != identity {
                return Err(std::io::Error::other("Directory changed during discovery"));
            }
            Ok((canonical, identity))
        })();
        match result {
            Ok((canonical, identity)) => {
                if seen.contains(&identity) {
                    return false;
                }
                seen.push(identity);
                row.path = Some(canonical);
                row.identity = Some(identity);
                row.status = Status::NotAnalyzed;
            }
            Err(error) => {
                row.status = failure_status(&error);
                row.cause = error.to_string().chars().take(2048).collect();
            }
        }
        true
    });
    rows
}
pub(super) fn native(cancel: &Cancellation) -> Vec<Location> {
    #[cfg(windows)]
    let (platform, inputs) = {
        use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Windows};
        (
            Platform::Windows,
            Inputs {
                local: known(&FOLDERID_LocalAppData),
                windows: known(&FOLDERID_Windows),
                temp: Some(std::env::temp_dir()),
                ..Default::default()
            },
        )
    };
    #[cfg(not(windows))]
    let (platform, inputs) = (
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else {
            Platform::Linux
        },
        Inputs {
            home: std::env::var_os("HOME").map(PathBuf::from),
            cache: std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            temp: Some(std::env::temp_dir()),
            developer: if cfg!(target_os = "macos") {
                let read = |key| std::env::var_os(key).map(PathBuf::from);
                DeveloperHomes {
                    cargo: read("CARGO_HOME"),
                    rustup: read("RUSTUP_HOME"),
                    gradle: read("GRADLE_USER_HOME"),
                    npm: read("npm_config_cache").or_else(|| read("NPM_CONFIG_CACHE")),
                    android: read("ANDROID_HOME"),
                    avd: read("ANDROID_AVD_HOME"),
                }
            } else {
                DeveloperHomes::default()
            },
            ..Default::default()
        },
    );
    resolve(candidates(platform, inputs), cancel)
}
#[cfg(windows)]
fn known(id: &windows::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DONT_VERIFY},
    };
    // The Shell allocates this UTF-16 path. Never request creation or elevation.
    unsafe {
        let path = SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None).ok()?;
        let value = std::ffi::OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        absolute(Some(PathBuf::from(value)))
    }
}

fn failure_status(error: &std::io::Error) -> Status {
    match error.kind() {
        std::io::ErrorKind::NotFound => Status::Missing,
        std::io::ErrorKind::PermissionDenied => Status::Inaccessible,
        _ => Status::Unavailable,
    }
}
#[cfg(test)]
#[test]
fn access_failures_are_typed_not_guessed_from_causes() {
    assert_eq!(
        failure_status(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        Status::Inaccessible
    );
    assert_eq!(
        failure_status(&std::io::Error::from(std::io::ErrorKind::NotFound)),
        Status::Missing
    );
    assert_eq!(
        failure_status(&std::io::Error::other(
            "Permission denied in an external cause"
        )),
        Status::Unavailable
    );
}
