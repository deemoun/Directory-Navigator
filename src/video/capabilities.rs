//! Session-only, identity-checked tool discovery and real encoder probes.
use super::*;
#[derive(Clone, Debug)]
pub(crate) struct Encoder {
    pub name: String,
    pub reason: String,
}
impl Encoder {
    pub fn available(&self) -> bool {
        self.reason.is_empty()
    }
}
#[derive(Clone)]
pub(in crate::video) struct Capabilities {
    pub tools: (PathBuf, PathBuf),
    stamps: Vec<(
        crate::platform_fs::EntryIdentity,
        u64,
        Option<std::time::SystemTime>,
    )>,
    pub encoders: Vec<Encoder>,
}
impl Capabilities {
    pub fn unchanged(&self) -> bool {
        [&self.tools.0, &self.tools.1]
            .into_iter()
            .zip(&self.stamps)
            .all(|(p, (identity, len, modified))| {
                fs::metadata(p).ok().is_some_and(|m| {
                    m.len() == *len
                        && m.modified().ok() == *modified
                        && crate::platform_fs::entry_identity(p).ok() == Some(*identity)
                })
            })
    }
}
pub(in crate::video) fn encoder_name(name: &str) -> &'static str {
    match name {
        "libx264" => "Software H.264",
        "libx265" => "Software H.265",
        "h264_videotoolbox" => "Apple hardware H.264",
        "hevc_videotoolbox" => "Apple hardware H.265",
        "h264_nvenc" => "NVIDIA hardware H.264",
        "hevc_nvenc" => "NVIDIA hardware H.265",
        "h264_qsv" => "Intel hardware H.264",
        "hevc_qsv" => "Intel hardware H.265",
        _ => "Unknown encoder",
    }
}
pub(in crate::video) fn hardware_probe(
    tool: &Path,
    encoder: &str,
    width: u32,
    height: u32,
    fps: &str,
    work: &Work,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let mut a = args(&[
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        &format!("color=c=gray:s={width}x{height}:r={fps}"),
        "-frames:v",
        "3",
        "-pix_fmt",
        "yuv420p",
        "-c:v",
        encoder,
    ]);
    if encoder.contains("videotoolbox") {
        a.extend(args(&["-allow_sw", "0", "-require_sw", "0"]));
    }
    a.extend(args(&["-f", "null", "-"]));
    run_bounded(
        tool,
        &a,
        work,
        cancel,
        true,
        Some(Duration::from_secs(10)),
        &mut |_| {},
    )
    .map(|_| ())
}
pub(in crate::video) fn discover(
    prefs: &Preferences,
    cached: Option<&Capabilities>,
    cancel: &AtomicBool,
) -> io::Result<Capabilities> {
    let tools = (
        tool(&prefs.ffmpeg, "ffmpeg")?,
        tool(&prefs.ffprobe, "ffprobe")?,
    );
    let stamps = [&tools.0, &tools.1]
        .into_iter()
        .map(|p| {
            let m = fs::metadata(p)?;
            Ok((
                crate::platform_fs::entry_identity(p)?,
                m.len(),
                m.modified().ok(),
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    if let Some(c) = cached.filter(|c| c.tools == tools && c.stamps == stamps) {
        return Ok(c.clone());
    }
    let work = Work::new(&std::env::temp_dir())?;
    for (t, expected) in [(&tools.0, "ffmpeg version"), (&tools.1, "ffprobe version")] {
        if !run(t, &args(&["-version"]), &work, cancel, true, &mut |_| {})?.starts_with(expected) {
            return Err(error("Selected executable is not FFmpeg/FFprobe"));
        }
    }
    let listing = run(
        &tools.0,
        &args(&["-hide_banner", "-encoders"]),
        &work,
        cancel,
        true,
        &mut |_| {},
    )?;
    let mut encoders = vec![];
    for name in [
        "libx264",
        "libx265",
        "h264_videotoolbox",
        "hevc_videotoolbox",
        "h264_nvenc",
        "hevc_nvenc",
        "h264_qsv",
        "hevc_qsv",
    ] {
        check(cancel)?;
        // Other platform backends are irrelevant here, but absent local backends
        // remain visible with a reason instead of disappearing from the selector.
        if (name.contains("videotoolbox") && !cfg!(target_os = "macos"))
            || ((name.contains("nvenc") || name.contains("qsv")) && cfg!(target_os = "macos"))
        {
            continue;
        }
        let reason = if !listing.split_whitespace().any(|w| w == name) {
            "Not included in this FFmpeg build".into()
        } else if name.starts_with("lib") {
            String::new()
        } else {
            match hardware_probe(&tools.0, name, 256, 144, "30", &work, cancel) {
                Ok(()) => String::new(),
                Err(e) => {
                    check(cancel)?;
                    e.to_string().chars().take(1024).collect()
                }
            }
        };
        encoders.push(Encoder {
            name: name.into(),
            reason,
        });
    }
    Ok(Capabilities {
        tools,
        stamps,
        encoders,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_is_bound_to_both_native_tool_identities_and_explicit_paths() -> io::Result<()> {
        let requested = std::env::current_exe()?;
        // Production discovery stores canonical native paths (including the
        // Windows verbatim prefix). Seed the cache in that same representation.
        let path = fs::canonicalize(&requested)?;
        let metadata = fs::metadata(&path)?;
        let stamp = (
            crate::platform_fs::entry_identity(&path)?,
            metadata.len(),
            metadata.modified().ok(),
        );
        let cached = Capabilities {
            tools: (path.clone(), path.clone()),
            stamps: vec![stamp, stamp],
            encoders: vec![Encoder {
                name: "libx264".into(),
                reason: String::new(),
            }],
        };
        let p = Preferences {
            ffmpeg: requested.to_string_lossy().into(),
            ffprobe: requested.to_string_lossy().into(),
        };
        let result = discover(&p, Some(&cached), &AtomicBool::new(false))?;
        // Running this test executable with -version would fail. A cache hit
        // performs no subprocess work and retains the verified result.
        assert_eq!(result.encoders[0].name, "libx264");
        assert_eq!(result.tools, (path.clone(), path.clone()));
        assert!(result.unchanged());
        // Both requested and canonical explicit spellings must hit the cache.
        let normalized = Preferences {
            ffmpeg: path.to_string_lossy().into(),
            ffprobe: path.to_string_lossy().into(),
        };
        assert_eq!(
            discover(&normalized, Some(&cached), &AtomicBool::new(false))?.tools,
            result.tools
        );
        for index in 0..2 {
            let mut changed = cached.clone();
            changed.stamps[index].1 += 1;
            assert!(!changed.unchanged(), "Both tool stamps must be checked");
        }
        assert!(!Encoder {
            name: "h264_videotoolbox".into(),
            reason: "Access denied".into()
        }
        .available());
        Ok(())
    }
}
