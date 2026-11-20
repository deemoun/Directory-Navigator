//! Cancellable external-media pipeline. No shell, unbounded capture or final-path writes.
use super::*;
use crate::{filesystem::open_regular, publication::StagedFile};
use serde_json::Value;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::AtomicU64,
    time::{Duration, Instant},
};
#[path = "capabilities.rs"]
pub(super) mod capabilities;
pub(super) use capabilities::{discover, encoder_name, Capabilities};
const MAX_FILES: usize = 10_000;
const MAX_DEPTH: usize = 512;
const CAPTURE_LIMIT: u64 = 8 * 1024 * 1024;
pub(super) fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation cancelled",
        ))
    } else {
        Ok(())
    }
}
fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
pub(super) struct Work {
    path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
}
impl Work {
    pub fn new(parent: &Path) -> io::Result<Self> {
        let parent = fs::canonicalize(parent)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = parent.join(format!(
                ".dn-video-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let b = fs::DirBuilder::new();
            #[cfg(unix)]
            let mut b = b;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            match b.create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        identity: crate::platform_fs::entry_identity(&path)?,
                        path,
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e),
            }
        }
        Err(error("Cannot create private video workspace"))
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        if crate::platform_fs::entry_identity(&self.path).ok() == Some(self.identity) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
#[derive(Clone)]
pub(super) struct Clip {
    pub path: PathBuf,
    metadata: Metadata,
    identity: crate::platform_fs::EntryIdentity,
    hash: blake3::Hash,
    pub duration: f64,
    stream: Value,
    audio: Option<Value>,
    pub width: u64,
    pub height: u64,
    pub fps: String,
    pub hdr: bool,
}
impl Clip {
    fn verify(&self) -> io::Result<()> {
        let file = open_regular(&self.path)?;
        let m = file.metadata()?;
        if crate::platform_fs::entry_identity(&self.path)? != self.identity
            || !crate::platform_fs::same_identity(&self.metadata, &m)
            || m.len() != self.metadata.len()
            || m.modified().ok() != self.metadata.modified().ok()
        {
            return Err(error(format!(
                "Source changed; retry analysis: {}",
                super::display(&self.path)
            )));
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Plan {
    pub request: Request,
    pub clips: Vec<Clip>,
    pub copy: bool,
    pub duplicates: usize,
    pub encoders: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: String,
    pub encoder: String,
    tools: (PathBuf, PathBuf),
    pub capabilities: Option<Capabilities>,
    pub warning: String,
}
pub(super) fn candidate(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        ["mp4", "mov", "mkv", "webm", "mts", "m2ts", "ts"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}
fn natural(a: &Path, b: &Path) -> std::cmp::Ordering {
    let a = a.to_string_lossy().to_lowercase();
    let b = b.to_string_lossy().to_lowercase();
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let aa = &a[start_i..i];
            let bb = &b[start_j..j];
            let aa = aa
                .iter()
                .position(|c| *c != b'0')
                .map_or(&aa[aa.len()..], |n| &aa[n..]);
            let bb = bb
                .iter()
                .position(|c| *c != b'0')
                .map_or(&bb[bb.len()..], |n| &bb[n..]);
            let c = aa.len().cmp(&bb.len()).then_with(|| aa.cmp(bb));
            if !c.is_eq() {
                return c;
            }
        } else {
            let c = a[i].cmp(&b[j]);
            if !c.is_eq() {
                return c;
            }
            i += 1;
            j += 1;
        }
    }
    a.len().cmp(&b.len())
}
fn collect(request: &Request, cancel: &AtomicBool) -> io::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    let mut examined = 0usize;
    let output = crate::requests::destination_key(&request.output)?;
    let csv = crate::requests::destination_key(&request.output.with_extension("csv"))?;
    let output_id = crate::platform_fs::entry_identity(&request.output).ok();
    let csv_id = request
        .csv
        .then(|| crate::platform_fs::entry_identity(&request.output.with_extension("csv")).ok())
        .flatten();
    let excluded = |path: &Path| -> io::Result<bool> {
        let key = crate::requests::destination_key(path)?;
        let identity = crate::platform_fs::entry_identity(path).ok();
        Ok(key == output
            || (request.csv && key == csv)
            || identity.is_some_and(|id| Some(id) == output_id || Some(id) == csv_id))
    };
    for source in &request.sources {
        check(cancel)?;
        let m = fs::symlink_metadata(source)?;
        if m.is_symlink() || crate::platform_fs::directory_link(&m) {
            return Err(error("Video sources cannot be links"));
        }
        if m.is_file() {
            if !candidate(source) {
                return Err(error("Select a supported video file or real folder"));
            }
            paths.push(source.clone());
        } else if m.is_dir() {
            let mut found = vec![];
            let mut stack = vec![(source.clone(), 0)];
            while let Some((directory, depth)) = stack.pop() {
                check(cancel)?;
                let dm = fs::symlink_metadata(&directory)?;
                if !dm.is_dir() || crate::platform_fs::directory_link(&dm) {
                    return Err(error("Source directory changed or became a link"));
                }
                for entry in fs::read_dir(directory)? {
                    check(cancel)?;
                    examined += 1;
                    if examined > 100_000 {
                        return Err(error("Video scan exceeds 100,000 entries"));
                    }
                    let entry = entry?;
                    let p = entry.path();
                    let m = fs::symlink_metadata(&p)?;
                    if m.is_symlink() || crate::platform_fs::directory_link(&m) {
                        continue;
                    }
                    if m.is_dir()
                        && request.recursive
                        && !entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".dn-video-")
                    {
                        if depth >= MAX_DEPTH {
                            return Err(error("Video scan exceeds 512 directory levels"));
                        }
                        stack.push((p, depth + 1));
                    } else if m.is_file() && candidate(&p) && !excluded(&p)? {
                        found.push(p);
                    }
                    if found.len() + paths.len() > MAX_FILES {
                        return Err(error("Video merge exceeds 10,000 clips"));
                    }
                }
            }
            found.sort_by(|a, b| {
                natural(
                    a.strip_prefix(source).unwrap_or(a),
                    b.strip_prefix(source).unwrap_or(b),
                )
                .then_with(|| a.cmp(b))
            });
            paths.extend(found);
        } else {
            return Err(error("Video source is not a regular file or folder"));
        }
        if paths.len() > MAX_FILES {
            return Err(error("Video merge exceeds 10,000 clips"));
        }
    }
    for p in &paths {
        if excluded(p)? {
            return Err(error(
                "Output collides with a selected source; choose another filename",
            ));
        }
    }
    if paths.is_empty() {
        return Err(error("No supported video files found"));
    }
    Ok(paths)
}
fn hash(
    path: &Path,
    cancel: &AtomicBool,
) -> io::Result<(blake3::Hash, Metadata, crate::platform_fs::EntryIdentity)> {
    let mut f = open_regular(path)?;
    let before = f.metadata()?;
    let identity = crate::platform_fs::entry_identity(path)?;
    let mut h = blake3::Hasher::new();
    let mut bytes = [0; 128 * 1024];
    loop {
        check(cancel)?;
        let n = f.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        h.update(&bytes[..n]);
    }
    let after = f.metadata()?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || !crate::platform_fs::owns_file(path, &f)
    {
        return Err(error("Source changed while hashing"));
    }
    Ok((h.finalize(), after, identity))
}
fn tool(value: &str, name: &str) -> io::Result<PathBuf> {
    let path = if !value.trim().is_empty() {
        PathBuf::from(value)
    } else {
        let filename = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        };
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|p| p.join(&filename)).find(|p| p.is_file()).ok_or_else(|| error(format!("{name} not found. Install FFmpeg: macOS: brew install ffmpeg; Ubuntu/Debian: sudo apt install ffmpeg; Fedora: install an FFmpeg build with H.264/AAC support; Windows: install FFmpeg and add its bin directory to PATH.")))?
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
    {
        return Err(error("Select a native executable, not a shell wrapper"));
    }
    let path = if cfg!(windows) && path.extension().is_none() {
        path.with_extension("exe")
    } else {
        path
    };
    if path.components().count() == 1 && !path.is_absolute() {
        if let Some(found) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(&path))
            .find(|p| p.is_file())
        {
            return fs::canonicalize(found);
        }
    }
    fs::canonicalize(&path).map_err(|cause| {
        io::Error::new(
            cause.kind(),
            format!(
                "{name}: {}: {cause}",
                crate::filesystem::display_name(&path.to_string_lossy())
            ),
        )
    })
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
fn run(
    tool: &Path,
    args: &[OsString],
    work: &Work,
    cancel: &AtomicBool,
    short: bool,
    progress: &mut (impl FnMut(Progress) + ?Sized),
) -> io::Result<String> {
    run_bounded(tool, args, work, cancel, short, None, progress)
}
fn run_bounded(
    tool: &Path,
    args: &[OsString],
    work: &Work,
    cancel: &AtomicBool,
    short: bool,
    timeout: Option<Duration>,
    progress: &mut (impl FnMut(Progress) + ?Sized),
) -> io::Result<String> {
    check(cancel)?;
    let mut stdout = StagedFile::create(&work.path.join("stdout"))?;
    let mut stderr = StagedFile::create(&work.path.join("stderr"))?;
    let mut progress_reader = File::open(stdout.path())?;
    let mut process = Process(
        Command::new(tool)
            .current_dir(&work.path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(stdout.file.try_clone()?)
            .stderr(stderr.file.try_clone()?)
            .spawn()?,
    );
    let child = &mut process.0;
    let start = Instant::now();
    let mut reported = Instant::now();
    let mut records = super::timing::Records::default();
    let result = (|| loop {
        if let Err(e) = check(cancel) {
            break Err(e);
        }
        if timeout.is_some_and(|limit| start.elapsed() > limit) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Speed test or hardware probe reached its time limit",
            ));
        }
        if (short && start.elapsed() > Duration::from_secs(30))
            || start.elapsed() > Duration::from_secs(24 * 3600)
        {
            break Err(error("FFmpeg/FFprobe timed out"));
        }
        if stdout.file.metadata()?.len() > CAPTURE_LIMIT
            || stderr.file.metadata()?.len() > CAPTURE_LIMIT
        {
            break Err(error("FFmpeg diagnostic output exceeds its safety limit"));
        }
        if !short && reported.elapsed() >= Duration::from_millis(200) {
            reported = Instant::now();
            let mut bytes = vec![0; 8192];
            // This file handle has its own cursor; consume bounded new bytes.
            let n = progress_reader.read(&mut bytes)?;
            for seconds in records.feed(&bytes[..n]) {
                let mut p = Progress::new(
                    "Processing",
                    super::timing::time(Duration::from_secs_f64(seconds.min(365.0 * 86400.0))),
                );
                p.processed = Some(seconds);
                progress(p);
            }
        }
        if let Some(status) = child.try_wait()? {
            if stdout.file.metadata()?.len() > CAPTURE_LIMIT
                || stderr.file.metadata()?.len() > CAPTURE_LIMIT
            {
                break Err(error("FFmpeg diagnostic output exceeds its safety limit"));
            }
            if status.success() {
                stdout.file.seek(SeekFrom::Start(0))?;
                let mut out = String::new();
                (&mut stdout.file)
                    .take(CAPTURE_LIMIT)
                    .read_to_string(&mut out)?;
                break Ok(out);
            }
            let len = stderr.file.metadata()?.len();
            stderr
                .file
                .seek(SeekFrom::Start(len.saturating_sub(16 * 1024)))?;
            let mut bytes = vec![];
            stderr.file.read_to_end(&mut bytes)?;
            break Err(error(format!(
                "FFmpeg/FFprobe failed ({status}): {}",
                String::from_utf8_lossy(&bytes)
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    })();
    // Always reap, including cancellation and parse failures. Capture files cannot
    // wait forever for a descendant holding a pipe open.
    result
}
fn args(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}
fn probe(path: &Path, probe: &Path, work: &Work, cancel: &AtomicBool) -> io::Result<Value> {
    let mut a = args(&[
        "-v",
        "error",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        "mov,matroska,webm,mpegts",
        "-show_streams",
        "-show_format",
        "-show_data",
        "-of",
        "json",
    ]);
    a.push(path.as_os_str().into());
    serde_json::from_str(&run(probe, &a, work, cancel, true, &mut |_| {})?)
        .map_err(|e| error(format!("Invalid FFprobe response: {e}")))
}
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").into()
}
fn number(v: &Value, key: &str) -> Option<f64> {
    v[key]
        .as_str()
        .and_then(|v| v.parse().ok())
        .or_else(|| v[key].as_f64())
        .filter(|n| n.is_finite() && *n > 0.0)
}
fn video(v: &Value) -> io::Result<&Value> {
    v["streams"]
        .as_array()
        .and_then(|s| {
            s.iter()
                .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        })
        .ok_or_else(|| error("File has no usable video stream"))
}
fn rotation(stream: &Value) -> i64 {
    stream["side_data_list"]
        .as_array()
        .and_then(|a| a.iter().find_map(|v| v["rotation"].as_i64()))
        .unwrap_or(0)
}
fn frame_rate(stream: &Value) -> String {
    ["avg_frame_rate", "r_frame_rate"]
        .into_iter()
        .map(|key| text(stream, key))
        .find(|rate| valid_rate(rate))
        .unwrap_or_default()
}
fn display_size(stream: &Value) -> (u64, u64) {
    let mut width = stream["width"].as_u64().unwrap_or(0);
    let mut height = stream["height"].as_u64().unwrap_or(0);
    if stream["sample_aspect_ratio"]
        .as_str()
        .is_some_and(|s| !matches!(s, "1:1" | "0:1" | "N/A"))
    {
        // No silent distortion: display-size conversion needs an explicit
        // square-pixel target rather than treating coded width as display width.
        let sar = text(stream, "sample_aspect_ratio");
        let mut parts = sar.split(':');
        if let (Some(a), Some(b)) = (
            parts.next().and_then(|s| s.parse::<u64>().ok()),
            parts.next().and_then(|s| s.parse::<u64>().ok()),
        ) {
            if let Some(display_width) = width.saturating_mul(a).checked_div(b) {
                width = display_width;
            }
        }
    }
    if rotation(stream).unsigned_abs() % 180 == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    (width, height)
}
fn hdr(stream: &Value) -> bool {
    matches!(
        stream["color_transfer"].as_str(),
        Some("smpte2084" | "arib-std-b67")
    ) || stream["pix_fmt"]
        .as_str()
        .is_some_and(|s| s.contains("10") || s.contains("12") || s.contains("16"))
        || stream["side_data_list"].as_array().is_some_and(|s| {
            s.iter().any(|v| {
                text(v, "side_data_type").contains("DOVI")
                    || text(v, "side_data_type").contains("Mastering")
            })
        })
}
fn stream_equal(a: &Value, b: &Value, audio: bool) -> bool {
    let keys: &[&str] = if audio {
        &[
            "codec_name",
            "profile",
            "codec_tag_string",
            "sample_fmt",
            "sample_rate",
            "channels",
            "channel_layout",
            "time_base",
            "extradata",
        ]
    } else {
        &[
            "codec_name",
            "profile",
            "level",
            "codec_tag_string",
            "width",
            "height",
            "pix_fmt",
            "sample_aspect_ratio",
            "time_base",
            "r_frame_rate",
            "avg_frame_rate",
            "color_range",
            "color_space",
            "color_transfer",
            "color_primaries",
            "extradata",
            "side_data_list",
        ]
    };
    keys.iter().all(|k| a[*k] == b[*k])
}
#[cfg(test)]
pub(super) fn prepare(
    request: Request,
    prefs: Preferences,
    cancel: &AtomicBool,
    progress: &mut (impl FnMut(Progress) + ?Sized),
) -> io::Result<Plan> {
    prepare_cached(request, prefs, None, cancel, progress)
}
pub(super) fn prepare_cached(
    mut request: Request,
    prefs: Preferences,
    cached: Option<Capabilities>,
    cancel: &AtomicBool,
    progress: &mut (impl FnMut(Progress) + ?Sized),
) -> io::Result<Plan> {
    request.settings.validate()?;
    request.output = crate::requests::destination_key(&request.output)?;
    if request
        .output
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("mp4"))
    {
        return Err(error("Choose an MP4 output filename"));
    }
    progress("Scanning".into());
    let paths = collect(&request, cancel)?;
    let capabilities = discover(&prefs, cached.as_ref(), cancel)?;
    let tools = capabilities.tools.clone();
    let work = Work::new(&std::env::temp_dir())?;
    let encoders = capabilities
        .encoders
        .iter()
        .filter(|e| e.available())
        .map(|e| e.name.clone())
        .collect::<Vec<_>>();
    let mut clips = vec![];
    let mut metadata_bytes = 0usize;
    let mut seen = HashSet::new();
    let mut duplicates = 0;
    for (i, p) in paths.into_iter().enumerate() {
        progress(Progress::new("Checking duplicates", (i + 1).to_string()));
        let (hash, metadata, identity) = hash(&p, cancel)?;
        if !seen.insert(hash) {
            duplicates += 1;
            continue;
        }
        progress(Progress::new("Probing", (i + 1).to_string()));
        let v = probe(&p, &tools.1, &work, cancel)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", super::display(&p))))?;
        let stream = video(&v)?.clone();
        let audio = v["streams"]
            .as_array()
            .and_then(|s| s.iter().find(|v| v["codec_type"] == "audio"))
            .cloned();
        let bytes = serde_json::to_vec(&stream)
            .map_err(|e| error(e.to_string()))?
            .len()
            + serde_json::to_vec(&audio)
                .map_err(|e| error(e.to_string()))?
                .len();
        metadata_bytes = metadata_bytes
            .checked_add(bytes)
            .ok_or_else(|| error("Video metadata exceeds its safety limit"))?;
        if bytes > 64 * 1024 || metadata_bytes > 16 * 1024 * 1024 {
            return Err(error(
                "Video metadata exceeds its safety limit (64 KiB per clip, 16 MiB per job)",
            ));
        }
        let duration = number(&v["format"], "duration")
            .or_else(|| number(&stream, "duration"))
            .ok_or_else(|| error("Clip duration is unavailable"))?;
        let (width, height) = display_size(&stream);
        let fps = frame_rate(&stream);
        if !valid_rate(&fps) || width == 0 || height == 0 {
            return Err(error("Clip has invalid dimensions or frame rate"));
        }
        let c = Clip {
            path: p,
            metadata,
            identity,
            hash,
            duration,
            hdr: hdr(&stream),
            stream,
            audio,
            width,
            height,
            fps,
        };
        c.verify()?;
        clips.push(c);
    }
    let mut plan = assemble(request, clips, duplicates, encoders, tools)?;
    if !plan.copy && plan.request.settings.codec == Codec::Auto && plan.encoder.starts_with("lib") {
        plan.warning = capabilities
            .encoders
            .iter()
            .filter(|e| e.name.starts_with("h264") && !e.available())
            .map(|e| format!("{}: {}", e.name, e.reason))
            .next()
            .unwrap_or_else(|| "No verified hardware encoder".into());
    }
    if !plan.copy && !plan.encoder.starts_with("lib") {
        if let Err(e) = capabilities::hardware_probe(
            &plan.tools.0,
            &plan.encoder,
            plan.width,
            plan.height,
            &plan.fps,
            &work,
            cancel,
        ) {
            check(cancel)?;
            if plan.request.settings.codec == Codec::Auto
                && plan.request.settings.hardware.is_empty()
            {
                plan.warning = e.to_string();
                plan.encoder = "libx264".into();
            } else {
                return Err(error(format!("Selected hardware is unavailable: {e}")));
            }
        }
    }
    plan.capabilities = Some(capabilities);
    Ok(plan)
}
fn assemble(
    request: Request,
    clips: Vec<Clip>,
    duplicates: usize,
    encoders: Vec<String>,
    tools: (PathBuf, PathBuf),
) -> io::Result<Plan> {
    let first = clips.first().ok_or_else(|| error("No unique clips"))?;
    let settings = &request.settings;
    let copy = settings.codec == Codec::Auto
        && settings.width.is_none()
        && settings.fps.is_empty()
        && settings.hardware.is_empty()
        && settings.quality.is_none()
        && settings.preset == "medium"
        && clips.iter().all(|c| {
            matches!(c.stream["codec_name"].as_str(), Some("h264" | "hevc"))
                && stream_equal(&first.stream, &c.stream, false)
                && (!settings.audio
                    || match (&first.audio, &c.audio) {
                        (None, None) => true,
                        (Some(a), Some(b)) => a["codec_name"] == "aac" && stream_equal(a, b, true),
                        _ => false,
                    })
        });
    if first.width > 16384 || first.height > 16384 {
        return Err(error(
            "Source dimensions exceed the supported limit (16384)",
        ));
    }
    let mut width = settings.width.unwrap_or(first.width as u32);
    let mut height = settings.height.unwrap_or(first.height as u32);
    if !copy {
        width = width.next_multiple_of(2);
        height = height.next_multiple_of(2);
    }
    let fps = if settings.fps.is_empty() {
        first.fps.clone()
    } else {
        settings.fps.clone()
    };
    let encoder = if !settings.hardware.is_empty() {
        settings.hardware.clone()
    } else if settings.codec == Codec::Auto && !copy {
        encoders
            .iter()
            .find(|e| !e.starts_with("lib") && e.starts_with("h264"))
            .cloned()
            .unwrap_or_else(|| "libx264".into())
    } else if settings.codec == Codec::H265 {
        "libx265".into()
    } else {
        "libx264".into()
    };
    let mut request = request;
    if request.settings.bitrate_kbps == 0 {
        let rate = rate_value(&fps);
        let kbps = (60000.0 * (f64::from(width) * f64::from(height) / (3840.0 * 2160.0)) * rate
            / 60.0)
            .clamp(8000.0, 200000.0);
        request.settings.bitrate_kbps = if encoder.starts_with("hevc") {
            (kbps * 2.0 / 3.0).round() as u32
        } else {
            kbps.round() as u32
        };
    }
    Ok(Plan {
        request,
        clips,
        copy,
        duplicates,
        encoders,
        width,
        height,
        fps,
        encoder,
        tools,
        capabilities: None,
        warning: String::new(),
    })
}
fn rate_value(rate: &str) -> f64 {
    let mut s = rate.split('/');
    let n = s.next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(30.0);
    let d = s.next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0);
    n / d
}
pub(super) fn valid_rate(rate: &str) -> bool {
    let mut parts = rate.split('/');
    let Some(n) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    let d = parts.next().map_or(Some(1), |s| s.parse::<u32>().ok());
    parts.next().is_none()
        && d.is_some_and(|d| d > 0 && n > 0 && f64::from(n) / f64::from(d) <= 1000.0)
}
fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
#[derive(Debug)]
pub(super) struct Report {
    pub path: PathBuf,
    pub warnings: Vec<(&'static str, String)>,
}
fn copy_color_args(stream: &Value, output: &mut Vec<OsString>) {
    // MOV/MP4 remuxing may drop container-only color declarations. Preserve
    // explicit source values without touching the compressed bitstream.
    for (key, flag) in [
        ("color_range", "-color_range"),
        ("color_space", "-colorspace"),
        ("color_transfer", "-color_trc"),
        ("color_primaries", "-color_primaries"),
    ] {
        if let Some(value) = stream[key]
            .as_str()
            .filter(|v| !v.is_empty() && *v != "unknown")
        {
            output.extend(args(&[flag, value]));
        }
    }
}
fn clip_args(
    plan: &Plan,
    c: &Clip,
    target: &Path,
    work: &Work,
    luts: &mut HashSet<super::color::Transfer>,
    cancel: &AtomicBool,
) -> io::Result<Vec<OsString>> {
    let request = &plan.request;
    let mut a = args(&[
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-y",
        "-xerror",
        "-progress",
        "pipe:1",
        "-stats_period",
        "1",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        "mov,matroska,webm,mpegts",
        "-i",
    ]);
    a.push(c.path.as_os_str().into());
    if !plan.copy && request.settings.audio && c.audio.is_none() {
        a.extend(args(&["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]));
    }
    a.extend(args(&[
        "-map",
        &format!("0:{}", c.stream["index"].as_u64().unwrap_or(0)),
    ]));
    if request.settings.audio {
        if let Some(audio) = &c.audio {
            a.extend(args(&[
                "-map",
                &format!("0:{}", audio["index"].as_u64().unwrap_or(1)),
            ]));
        } else if !plan.copy {
            a.extend(args(&["-map", "1:a:0", "-shortest"]));
        }
    } else {
        a.push("-an".into());
    }
    if plan.copy {
        a.extend(args(&["-c", "copy"]));
        copy_color_args(&c.stream, &mut a);
    } else {
        // Fit display aspect ratio, not coded dimensions; normalize SAR only
        // after scaling so anamorphic sources are not squeezed or letterboxed.
        let dar = "a*if(gt(sar,0),sar,1)";
        let mut filter = format!("scale=w='max(2,trunc(min({w},{h}*({dar}))/2)*2)':h='max(2,trunc(min({h},{w}/({dar}))/2)*2)':out_color_matrix=bt709:out_range=tv,setsar=1,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,fps={fps},format=yuv420p",w=plan.width,h=plan.height,fps=plan.fps);
        if let Some(transfer) =
            super::color::Transfer::from_name(&text(&c.stream, "color_transfer"))
        {
            if luts.insert(transfer) {
                super::color::write(&work.path, transfer, cancel)?;
            }
            filter=format!("fps={fps},scale=w='max(2,trunc(min({w},{h}*({dar}))/2)*2)':h='max(2,trunc(min({h},{w}/({dar}))/2)*2)',scale=in_color_matrix=bt2020:out_range=pc,format=gbrp16le,lut3d=file={}:interp=tetrahedral,scale=out_color_matrix=bt709:out_range=tv,setsar=1,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,format=yuv420p",transfer.filename(),w=plan.width,h=plan.height,fps=plan.fps);
        }
        // Every converted clip uses the same SDR contract. Strip stale HDR
        // mastering/Dolby Vision frame side data after applying orientation.
        filter.push_str(",sidedata=mode=delete,setparams=range=limited:color_primaries=bt709:color_trc=bt709:colorspace=bt709");
        a.extend(args(&[
            "-vf",
            &filter,
            "-c:v",
            &plan.encoder,
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_primaries",
            "bt709",
        ]));
        if plan.encoder.contains("videotoolbox") {
            a.extend(args(&["-allow_sw", "0", "-require_sw", "0"]));
        }
        if plan.encoder.starts_with("lib") {
            let quality = request
                .settings
                .quality
                .unwrap_or(if plan.encoder == "libx265" { 20 } else { 18 })
                .to_string();
            a.extend(args(&[
                "-crf",
                &quality,
                "-preset",
                &request.settings.preset,
            ]));
        } else {
            a.extend(args(&[
                "-b:v",
                &format!("{}k", request.settings.bitrate_kbps),
            ]));
        }
        if request.settings.audio {
            a.extend(args(&[
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-af",
                "aresample=async=1:first_pts=0",
            ]));
        }
    }
    if plan.copy && c.stream["codec_name"] == "hevc" {
        let tag = text(&c.stream, "codec_tag_string");
        let tag = if ["hvc1", "hev1", "dvh1", "dvhe"].contains(&tag.as_str()) {
            tag.as_str()
        } else {
            "hvc1"
        };
        a.extend(args(&["-tag:v", tag]));
    } else if !plan.copy && (plan.encoder.contains("265") || plan.encoder.contains("hevc")) {
        a.extend(args(&["-tag:v", "hvc1"]));
    }
    a.extend(args(&[
        "-map_metadata",
        "-1",
        "-movflags",
        "+write_colr",
        "-f",
        "mp4",
    ]));
    a.push(target.as_os_str().into());
    Ok(a)
}
pub(super) fn execute(
    plan: &Plan,
    cancel: &AtomicBool,
    mailbox: &crate::operations::conflict::Mailbox,
    progress: &mut (impl FnMut(Progress) + ?Sized),
) -> io::Result<Report> {
    let request = &plan.request;
    if plan.capabilities.as_ref().is_some_and(|c| !c.unchanged()) {
        return Err(error("FFmpeg/FFprobe changed; Analyze again"));
    }
    if !plan.copy && !plan.encoders.contains(&plan.encoder) {
        return Err(error(format!("Required encoder is unavailable: {}. Choose a supported encoder or install a suitable FFmpeg build.",plan.encoder)));
    }

    let output =
        crate::operations::generated::Output::new(request.output.clone(), cancel, mailbox)?;
    let csv_output = if request.csv {
        Some(crate::operations::generated::Output::new(
            request.output.with_extension("csv"),
            cancel,
            mailbox,
        )?)
    } else {
        None
    };
    let work = Work::new(
        request
            .output
            .parent()
            .ok_or_else(|| error("Missing output directory"))?,
    )?;
    let mut luts = HashSet::new();
    let mut normalized = vec![];
    let mut durations = vec![];
    for (i, c) in plan.clips.iter().enumerate() {
        c.verify()?;
        if hash(&c.path, cancel)?.0 != c.hash {
            return Err(error("Source content changed; retry analysis"));
        }
        let target = work.path.join(format!("clip-{i:05}.mp4"));
        let a = clip_args(plan, c, &target, &work, &mut luts, cancel)?;
        let phase = if plan.copy { "Joining" } else { "Converting" };
        let completed = plan.clips[..i].iter().map(|c| c.duration).sum::<f64>();
        let emit = |seconds: f64| Progress {
            stage: phase,
            detail: format!("{}/{}", i + 1, plan.clips.len()),
            processed: Some(completed + seconds.clamp(0.0, c.duration)),
            clip: i + 1,
            clips: plan.clips.len(),
        };
        progress(emit(0.0));
        run(&plan.tools.0, &a, &work, cancel, false, &mut |t| {
            progress(emit(t.processed.unwrap_or(0.0)))
        })?;
        progress(emit(c.duration));
        c.verify()?;
        // Controlled relative names avoid FFconcat quoting, native-path and
        // protocol injection; no user filename is inserted into this document.
        let probe = probe(&target, &plan.tools.1, &work, cancel)?;
        durations.push(
            number(&probe["format"], "duration")
                .ok_or_else(|| error("Normalized clip duration is unavailable"))?,
        );
        normalized.push(target);
    }
    let list = work.path.join("concat.txt");
    let mut f = File::create(&list)?;
    for i in 0..normalized.len() {
        writeln!(f, "file 'clip-{i:05}.mp4'")?;
    }
    drop(f);
    let stage = output.stage()?;
    let mut a = args(&[
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-y",
        "-progress",
        "pipe:1",
        "-stats_period",
        "1",
        "-protocol_whitelist",
        "file",
        "-f",
        "concat",
        "-safe",
        "1",
        "-i",
    ]);
    a.push(list.as_os_str().into());
    a.extend(args(&[
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-c",
        "copy",
        "-movflags",
        "+faststart+write_colr",
        "-f",
        "mp4",
    ]));
    if plan.copy {
        copy_color_args(&plan.clips[0].stream, &mut a);
    }
    a.push(stage.path().as_os_str().into());
    progress("Joining".into());
    run(&plan.tools.0, &a, &work, cancel, false, progress)?;
    progress("Validating".into());
    let result = probe(stage.path(), &plan.tools.1, &work, cancel)?;
    let output_video = video(&result)?;
    let color_metadata_changed = plan.copy
        && [
            "pix_fmt",
            "color_range",
            "color_space",
            "color_transfer",
            "color_primaries",
            "side_data_list",
        ]
        .iter()
        .any(|key| plan.clips[0].stream[*key] != output_video[*key]);
    if number(&result["format"], "duration").is_none() || stage.file.metadata()?.len() == 0 {
        return Err(error("Merged output failed validation; nothing published"));
    }
    let mut validate = args(&[
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-xerror",
        "-err_detect",
        "explode",
        "-protocol_whitelist",
        "file",
        "-i",
    ]);
    validate.push(stage.path().as_os_str().into());
    validate.extend(args(&[
        "-map", "0:v:0", "-map", "0:a:0?", "-f", "null", "-",
    ]));
    run(&plan.tools.0, &validate, &work, cancel, false, &mut |t| {
        progress(Progress::new("Validating", t.detail))
    })?;
    for c in &plan.clips {
        c.verify()?;
        if hash(&c.path, cancel)?.0 != c.hash {
            return Err(error(
                "Source content changed during processing; nothing published",
            ));
        }
    }
    let warning = output.publish(&stage, cancel)?;
    let mut result = Report {
        path: request.output.clone(),
        warnings: vec![],
    };
    if color_metadata_changed {
        result.warnings.push((
            "Output color metadata differs from the source",
            String::new(),
        ));
    }
    if let Some(w) = warning {
        result
            .warnings
            .push(("Directory synchronization warning", w));
    }
    if let Some(csv_output) = csv_output {
        let csv = (|| -> io::Result<Option<String>> {
            let mut stage = csv_output.stage()?;
            writeln!(
                stage.file,
                "sequence,clip,start_seconds,end_seconds,duration_seconds"
            )?;
            let mut start = 0.0;
            for (i, c) in plan.clips.iter().enumerate() {
                writeln!(
                    stage.file,
                    "{},{},{:.6},{:.6},{:.6}",
                    i + 1,
                    csv_cell(&c.path.file_name().unwrap_or_default().to_string_lossy()),
                    start,
                    start + durations[i],
                    durations[i]
                )?;
                start += durations[i];
            }
            csv_output.publish(&stage, cancel)
        })();
        match csv {
            Ok(Some(w)) => result.warnings.push((
                "Timeline CSV published; directory synchronization failed",
                w,
            )),
            Ok(None) => (),
            Err(e) => result.warnings.push((
                "Video published; timeline CSV was not published",
                e.to_string(),
            )),
        }
    }
    Ok(result)
}

pub(super) fn diagnostics(
    preferences: &Preferences,
    cancel: &AtomicBool,
) -> crate::utilities::information::Fields {
    let mut fields = Vec::new();
    for (label, configured, name) in [
        ("FFmpeg", &preferences.ffmpeg, "ffmpeg"),
        ("FFprobe", &preferences.ffprobe, "ffprobe"),
    ] {
        if check(cancel).is_err() {
            break;
        }
        let result = (|| -> io::Result<(PathBuf, String)> {
            let path = tool(configured, name)?;
            let work = Work::new(&std::env::temp_dir())?;
            let output = run_bounded(
                &path,
                &args(&["-version"]),
                &work,
                cancel,
                true,
                Some(Duration::from_secs(3)),
                &mut |_| {},
            )?;
            let line = output.lines().next().unwrap_or("").to_string();
            if !line
                .to_ascii_lowercase()
                .starts_with(&format!("{name} version"))
            {
                return Err(error(
                    "Executable did not identify as the requested FFmpeg tool",
                ));
            }
            Ok((path, line))
        })();
        match result {
            Ok((path, version)) => {
                fields.push((label.into(), version));
                fields.push((format!("{label} path"), path.to_string_lossy().into_owned()));
            }
            Err(e) => fields.push((label.into(), e.to_string())),
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(work: &Work) -> Request {
        Request {
            sources: vec![],
            output: work.path.join("merged.mp4"),
            recursive: false,
            csv: false,
            settings: Default::default(),
        }
    }
    #[test]
    fn exact_rates_natural_order_stream_safety_and_hdr() {
        for rate in ["30000/1001", "60000/1001", "120", "1/2"] {
            assert!(valid_rate(rate));
        }
        for rate in ["0/0", "30/0", "NaN", "30/1/2", "1001", "-1"] {
            assert!(!valid_rate(rate));
        }
        assert!(natural(Path::new("part2.mp4"), Path::new("part10.mp4")).is_lt());
        assert!(candidate(Path::new("Ёжик.MP4")));
        assert!(!candidate(Path::new("clip.m3u8")));
        let a = serde_json::json!({"codec_name":"h264","time_base":"1/30000","extradata":"abc","color_transfer":"bt709"});
        let mut b = a.clone();
        assert!(stream_equal(&a, &b, false));
        b["extradata"] = "different".into();
        assert!(!stream_equal(&a, &b, false));
        assert!(hdr(&serde_json::json!({"pix_fmt":"yuv420p10le"})));
        assert!(hdr(&serde_json::json!({"color_transfer":"smpte2084"})));
        assert!(!hdr(&a));
        assert_eq!(
            frame_rate(&serde_json::json!({"avg_frame_rate":"0/0","r_frame_rate":"30000/1001"})),
            "30000/1001"
        );
        assert_eq!(
            display_size(
                &serde_json::json!({"width":160,"height":90,"sample_aspect_ratio":"2:1","side_data_list":[{"rotation":90}]})
            ),
            (90, 320)
        );
        assert_eq!(csv_cell("a,\"b"), "\"a,\"\"b\"");
    }
    #[test]
    #[ignore = "subprocess fixture; only launched and reaped by its parent regression"]
    fn progress_child_fixture() {
        println!("out_time_us=1000000\nprogress=continue");
        std::io::stdout().flush().unwrap();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    #[test]
    #[ignore = "subprocess fixture; only launched by its parent regression"]
    fn oversized_child_fixture() {
        let mut out = std::io::stdout().lock();
        for _ in 0..(CAPTURE_LIMIT / 4096 + 2) {
            out.write_all(&[b'x'; 4096]).unwrap();
        }
    }
    #[test]
    fn process_progress_cancellation_capture_limits_and_cleanup() -> io::Result<()> {
        let w = Work::new(&std::env::temp_dir())?;
        let cancel = AtomicBool::new(false);
        let child_args = |name: &str| {
            args(&[
                "--exact",
                &format!("video::engine::tests::{name}"),
                "--ignored",
                "--nocapture",
            ])
        };
        let e = run(
            &std::env::current_exe()?,
            &child_args("progress_child_fixture"),
            &w,
            &cancel,
            false,
            &mut |p| {
                assert_eq!(p.stage, "Processing");
                cancel.store(true, Ordering::Relaxed);
            },
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        assert_eq!(fs::read_dir(&w.path)?.count(), 0);
        cancel.store(false, Ordering::Relaxed);
        let e = run(
            &std::env::current_exe()?,
            &child_args("oversized_child_fixture"),
            &w,
            &cancel,
            true,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(e.to_string().contains("safety limit"));
        assert_eq!(fs::read_dir(&w.path)?.count(), 0);
        Ok(())
    }
    #[test]
    fn collection_hashing_cancellation_and_source_changes() -> io::Result<()> {
        let w = Work::new(&std::env::temp_dir())?;
        let c = AtomicBool::new(false);
        for name in [
            "clip10.mp4",
            "clip2.MP4",
            ".hidden.mov",
            "other.txt",
            "merged.mp4",
        ] {
            fs::write(w.path.join(name), name)?;
        }
        fs::create_dir(w.path.join("nested"))?;
        fs::write(w.path.join("nested/clip1.mp4"), b"nested")?;
        let mut r = request(&w);
        r.sources = vec![w.path.clone()];
        let paths = collect(&r, &c)?;
        assert_eq!(paths.len(), 3);
        assert!(paths[1].ends_with("clip2.MP4"));
        assert!(paths[2].ends_with("clip10.mp4"));
        r.recursive = true;
        assert_eq!(collect(&r, &c)?.len(), 4);
        let p = w.path.join("clip2.MP4");
        let (h, m, id) = hash(&p, &c)?;
        assert_eq!(h, hash(&p, &c)?.0);
        let clip = Clip {
            path: p.clone(),
            metadata: m,
            identity: id,
            hash: h,
            duration: 1.0,
            stream: Value::Null,
            audio: None,
            width: 2,
            height: 2,
            fps: "30".into(),
            hdr: false,
        };
        clip.verify()?;
        fs::write(&p, b"changed content length")?;
        assert!(clip.verify().is_err());
        r.sources = vec![r.output.clone()];
        assert!(collect(&r, &c).is_err());
        c.store(true, Ordering::Relaxed);
        assert_eq!(hash(&p, &c).unwrap_err().kind(), io::ErrorKind::Interrupted);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn source_directory_links_are_not_followed() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let w = Work::new(&std::env::temp_dir())?;
        let other = Work::new(&std::env::temp_dir())?;
        fs::write(w.path.join("clip.mp4"), b"clip")?;
        fs::write(other.path.join("secret.mp4"), b"secret")?;
        symlink(&other.path, w.path.join("redirect"))?;
        let mut r = request(&w);
        r.sources = vec![w.path.clone()];
        r.recursive = true;
        assert_eq!(collect(&r, &AtomicBool::new(false))?.len(), 1);
        r.sources = vec![w.path.join("redirect")];
        assert!(collect(&r, &AtomicBool::new(false)).is_err());
        Ok(())
    }
    #[test]
    fn plans_preserve_fractional_fps_and_allow_hdr_conversion() -> io::Result<()> {
        let w = Work::new(&std::env::temp_dir())?;
        let p = w.path.join("clip.mp4");
        fs::write(&p, b"private")?;
        let cancel = AtomicBool::new(false);
        let (h, metadata, identity) = hash(&p, &cancel)?;
        let first = Clip {
            path: p,
            metadata,
            identity,
            hash: h,
            duration: 1.0,
            width: 160,
            height: 90,
            fps: "30000/1001".into(),
            hdr: false,
            audio: None,
            stream: serde_json::json!({"codec_name":"h264","width":160,"height":90,"pix_fmt":"yuv420p","avg_frame_rate":"30000/1001","extradata":"same"}),
        };
        let tools = (PathBuf::from("ffmpeg"), PathBuf::from("ffprobe"));
        let encoders = vec!["libx264".into(), "libx265".into()];
        let build = |r, clips| assemble(r, clips, 0, encoders.clone(), tools.clone());
        assert!(build(request(&w), vec![first.clone(), first.clone()])?.copy);
        let mut second = first.clone();
        second.width = 120;
        second.stream["width"] = 120.into();
        let plan = build(request(&w), vec![first.clone(), second.clone()])?;
        assert!(!plan.copy);
        assert_eq!(plan.fps, "30000/1001");
        assert_eq!((plan.width, plan.height), (160, 90));
        let hardware = vec!["libx264".into(), "h264_videotoolbox".into()];
        let automatic = assemble(
            request(&w),
            vec![first.clone(), second.clone()],
            0,
            hardware.clone(),
            tools.clone(),
        )?;
        assert_eq!(automatic.encoder, "h264_videotoolbox");
        assert!(
            assemble(
                request(&w),
                vec![first.clone()],
                0,
                hardware.clone(),
                tools.clone()
            )?
            .copy
        );
        let mut software = request(&w);
        software.settings.codec = Codec::H264;
        assert_eq!(
            assemble(software, vec![first.clone()], 0, hardware, tools.clone())?.encoder,
            "libx264"
        );
        second.hdr = true;
        second.path = w.path.join("hdr.mp4");
        let converted = build(request(&w), vec![first.clone(), second])?;
        assert!(!converted.copy);
        assert!(converted.clips[1].hdr);
        let mut hdr = first.clone();
        hdr.hdr = true;
        hdr.stream["pix_fmt"] = "yuv420p10le".into();
        assert!(build(request(&w), vec![hdr.clone(), hdr])?.copy);
        let mut r = request(&w);
        r.settings.codec = Codec::H265;
        let plan = build(r, vec![first.clone()])?;
        assert!(!plan.copy);
        assert_eq!(plan.encoder, "libx265");
        let mut r = request(&w);
        r.settings.quality = Some(22);
        let mut odd = first.clone();
        odd.width = 159;
        odd.height = 89;
        let rounded = build(r.clone(), vec![odd])?;
        assert_eq!((rounded.width, rounded.height), (160, 90));
        assert!(!build(r, vec![first])?.copy);
        Ok(())
    }
    #[test]
    #[ignore = "requires installed FFmpeg/FFprobe with libx264 and libx265; uses only private generated media"]
    fn real_ffmpeg_copy_conversion_dedup_csv_and_changed_sources() -> io::Result<()> {
        let w = Work::new(&std::env::temp_dir())?;
        let cancel = AtomicBool::new(false);
        let ffmpeg = tool("", "ffmpeg")?;
        let make = |name: &str, color: &str, size: &str, fps: &str| -> io::Result<PathBuf> {
            let p = w.path.join(name);
            let mut a = args(&[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c={color}:s={size}:r={fps}:d=0.4"),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-y",
            ]);
            a.push(p.as_os_str().into());
            run(&ffmpeg, &a, &w, &cancel, false, &mut |_| {})?;
            Ok(p)
        };
        let a = make("Ёжик 'a' 2.mp4", "red", "160x90", "30000/1001")?;
        let b = make("clip10.mp4", "blue", "160x90", "30000/1001")?;
        let duplicate = w.path.join("duplicate.mp4");
        fs::copy(&a, &duplicate)?;
        let mut r = request(&w);
        r.sources = vec![a.clone(), duplicate, b.clone()];
        r.csv = true;
        let plan = prepare(r.clone(), Default::default(), &cancel, &mut |_| {})?;
        assert!(plan.copy);
        assert_eq!(plan.duplicates, 1);
        assert_eq!(plan.fps, "30000/1001");
        assert_eq!(plan.clips.len(), 2);
        let result = execute(&plan, &cancel, &Default::default(), &mut |_| {})?;
        assert_eq!(result.path, r.output);
        assert!(r.output.is_file());
        let csv = fs::read_to_string(r.output.with_extension("csv"))?;
        assert!(csv.contains("Ёжик"));
        assert!(!csv.contains(&w.path.to_string_lossy().to_string()));
        let c = make("small.mp4", "green", "120x80", "25")?;
        r.sources = vec![a.clone(), c];
        r.output = w.path.join("converted.mp4");
        r.csv = false;
        let mut plan = prepare(r, Default::default(), &cancel, &mut |_| {})?;
        assert!(!plan.copy);
        assert_eq!(plan.fps, "30000/1001");
        execute(&plan, &cancel, &Default::default(), &mut |_| {})?;
        let mut hevc_request = plan.request.clone();
        hevc_request.settings.codec = Codec::H265;
        hevc_request.output = w.path.join("hevc.mp4");
        let hevc = prepare(hevc_request, Default::default(), &cancel, &mut |_| {})?;
        execute(&hevc, &cancel, &Default::default(), &mut |_| {})?;
        assert_eq!(
            video(&probe(&hevc.request.output, &hevc.tools.1, &w, &cancel)?)?["codec_name"],
            "hevc"
        );
        let audio_path = w.path.join("with-audio.mp4");
        let mut audio_args = args(&[
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=yellow:s=160x90:r=25:d=0.4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            "-y",
        ]);
        audio_args.push(audio_path.as_os_str().into());
        run(&ffmpeg, &audio_args, &w, &cancel, false, &mut |_| {})?;
        let mut mixed = plan.request.clone();
        mixed.sources = vec![a.clone(), audio_path];
        mixed.output = w.path.join("audio.mp4");
        let audio_plan = prepare(mixed, Default::default(), &cancel, &mut |_| {})?;
        assert!(!audio_plan.copy);
        execute(&audio_plan, &cancel, &Default::default(), &mut |_| {})?;
        let info = probe(&audio_plan.request.output, &audio_plan.tools.1, &w, &cancel)?;
        assert!(info["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["codec_name"] == "aac"));
        let anamorphic_path = w.path.join("anamorphic.mp4");
        let mut anamorphic_args = args(&[
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=25:d=0.4",
            "-vf",
            "setsar=2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ]);
        anamorphic_args.push(anamorphic_path.as_os_str().into());
        run(&ffmpeg, &anamorphic_args, &w, &cancel, false, &mut |_| {})?;
        let mut ar = plan.request.clone();
        ar.sources = vec![anamorphic_path];
        ar.output = w.path.join("square-pixels.mp4");
        ar.settings.codec = Codec::H264;
        let ar = prepare(ar, Default::default(), &cancel, &mut |_| {})?;
        assert_eq!((ar.width, ar.height), (320, 90));
        execute(&ar, &cancel, &Default::default(), &mut |_| {})?;
        let pixels = Command::new(&ffmpeg)
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&ar.request.output)
            .args([
                "-vf",
                "format=rgb24,crop=1:1:0:0",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "pipe:1",
            ])
            .output()?;
        assert!(pixels.status.success());
        assert_eq!(pixels.stdout.len(), 3);
        assert!(
            pixels.stdout[0] > 180 && pixels.stdout[1] < 80 && pixels.stdout[2] < 80,
            "No black padding is needed for an anamorphic source matched to its display dimensions"
        );
        // Mixed phone HDR and SDR must merge with no quality-policy refusal.
        // Include punctuation/native characters in destination paths: LUT paths
        // are private relative filenames, never interpolated user path strings.
        let destination = w.path.join("HDR 'ёжик, output");
        fs::create_dir(&destination)?;
        for transfer in ["arib-std-b67", "smpte2084", "bt709"] {
            let source = w.path.join(format!("{transfer}.MOV"));
            let mut hdr_args = args(&[
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:s=160x90:r=25:d=0.4",
                "-vf",
                "format=yuv420p10le",
                "-c:v",
                "libx265",
                "-x265-params",
                &format!(
                    "pools=1:frame-threads=1:colorprim=9:colormatrix=9:transfer={}",
                    match transfer {
                        "arib-std-b67" => 18,
                        "smpte2084" => 16,
                        _ => 1,
                    }
                ),
                "-movflags",
                "+write_colr",
            ]);
            hdr_args.push(source.as_os_str().into());
            run(&ffmpeg, &hdr_args, &w, &cancel, false, &mut |_| {})?;
            let mut hdr_request = plan.request.clone();
            hdr_request.sources = vec![source.clone(), a.clone()];
            hdr_request.output = destination.join(format!("{transfer}-mixed.mp4"));
            let hdr_plan = prepare(hdr_request, Default::default(), &cancel, &mut |_| {})?;
            assert!(!hdr_plan.copy);
            assert!(hdr_plan.clips[0].hdr);
            assert_eq!(hdr_plan.clips[0].stream["color_transfer"], transfer);
            execute(&hdr_plan, &cancel, &Default::default(), &mut |_| {})?;
            let result = probe(&hdr_plan.request.output, &hdr_plan.tools.1, &w, &cancel)?;
            let stream = video(&result)?;
            assert_eq!(stream["pix_fmt"], "yuv420p");
            assert_eq!(stream["color_transfer"], "bt709");
            assert_eq!(stream["color_primaries"], "bt709");
            assert_eq!(stream["color_space"], "bt709");
            let pixels = Command::new(&ffmpeg)
                .args(["-nostdin", "-v", "error", "-i"])
                .arg(&hdr_plan.request.output)
                .args([
                    "-vf",
                    "format=rgb24,crop=1:1:80:44",
                    "-frames:v",
                    "1",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ])
                .output()?;
            assert!(pixels.status.success());
            assert_eq!(pixels.stdout.len(), 3);
            assert!(pixels.stdout.iter().all(|v| *v > 30));
            assert!(pixels.stdout.iter().max().unwrap() - pixels.stdout.iter().min().unwrap() < 8);
            let mut copy_request = hdr_plan.request.clone();
            copy_request.sources = vec![source];
            copy_request.output = destination.join(format!("{transfer}-copy.mp4"));
            let copy = prepare(copy_request, Default::default(), &cancel, &mut |_| {})?;
            assert!(copy.copy);
            execute(&copy, &cancel, &Default::default(), &mut |_| {})?;
            let result = probe(&copy.request.output, &copy.tools.1, &w, &cancel)?;
            assert_eq!(video(&result)?["color_transfer"], transfer);
        }
        let mut stopped = plan.clone();
        stopped.request.output = w.path.join("cancelled.mp4");
        let e = execute(&stopped, &cancel, &Default::default(), &mut |_| {
            cancel.store(true, Ordering::Relaxed)
        })
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        assert!(!stopped.request.output.exists());
        cancel.store(false, Ordering::Relaxed);
        let malformed = w.path.join("malformed.mp4");
        fs::write(&malformed, b"not video")?;
        let mut invalid = plan.request.clone();
        invalid.sources = vec![malformed];
        invalid.output = w.path.join("invalid.mp4");
        assert!(prepare(invalid, Default::default(), &cancel, &mut |_| {}).is_err());
        plan.request.output = w.path.join("changed.mp4");
        fs::write(a, b"replaced")?;
        assert!(execute(&plan, &cancel, &Default::default(), &mut |_| {}).is_err());
        assert!(!plan.request.output.exists());
        assert!(!fs::read_dir(&w.path)?.any(|p| p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dn-")));
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) struct SpeedResult {
    pub encoder: String,
    pub speed: Option<f64>,
    pub incomplete: bool,
    pub reason: String,
}
pub(super) fn benchmark(
    plan: &Plan,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> io::Result<Vec<SpeedResult>> {
    if plan.capabilities.as_ref().is_some_and(|c| !c.unchanged()) {
        return Err(error("FFmpeg/FFprobe changed; Analyze again"));
    }
    let work = Work::new(&std::env::temp_dir())?;
    let clip = plan.clips.iter().find(|c| c.hdr).unwrap_or(&plan.clips[0]);
    let hevc = plan.encoder.contains("265") || plan.encoder.starts_with("hevc");
    let software = if hevc { "libx265" } else { "libx264" };
    let mut encoders = vec![software.to_owned()];
    if let Some(hardware) = plan
        .encoders
        .iter()
        .find(|e| !e.starts_with("lib") && (e.starts_with("hevc") == hevc))
    {
        encoders.push(hardware.clone());
    }
    let mut results = vec![];
    let mut luts = HashSet::new();
    for encoder in encoders {
        check(cancel)?;
        clip.verify()?;
        let mut sample = plan.clone();
        sample.copy = false;
        sample.encoder = encoder.clone();
        sample.request.settings.codec = if hevc { Codec::H265 } else { Codec::H264 };
        let target = work.path.join(format!("sample-{}.mp4", results.len()));
        let mut a = clip_args(&sample, clip, &target, &work, &mut luts, cancel)?;
        let output = a.pop().unwrap();
        a.extend(args(&["-t", "10"]));
        a.push(output);
        let start = Instant::now();
        let mut media = 0.0;
        progress(Progress::new(
            "Testing speed",
            encoder_name(&encoder).into(),
        ));
        let result = run_bounded(
            &plan.tools.0,
            &a,
            &work,
            cancel,
            false,
            Some(Duration::from_secs(60)),
            &mut |p| {
                media = p.processed.unwrap_or(media);
                progress(Progress::new(
                    "Testing speed",
                    format!("{} {}", encoder_name(&encoder), p.detail),
                ));
            },
        );
        let elapsed = start.elapsed().as_secs_f64();
        check(cancel)?;
        clip.verify()?;
        let incomplete = result
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::TimedOut);
        let reason = match result {
            Ok(_) => {
                media = clip.duration.min(10.0);
                String::new()
            }
            Err(e) => e.to_string(),
        };
        let speed = if (reason.is_empty() || incomplete) && media > 0.0 && elapsed > 0.0 {
            Some(media.min(10.0) / elapsed)
        } else {
            None
        };
        results.push(SpeedResult {
            encoder,
            speed,
            incomplete,
            reason,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod speed_tests {
    use super::*;
    #[test]
    #[ignore = "requires real FFmpeg/libx265 and optionally Apple hardware; private 4K fixtures"]
    fn real_4k_hdr_speed_comparison() -> io::Result<()> {
        let work = Work::new(&std::env::temp_dir())?;
        let cancel = AtomicBool::new(false);
        let ffmpeg = tool("", "ffmpeg")?;
        let source = work.path.join("private-HLG.mp4");
        let mut a = args(&[
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:s=3840x2160:r=60:d=2",
            "-vf",
            "format=yuv420p10le",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-x265-params",
            "pools=1:frame-threads=1:colorprim=9:colormatrix=9:transfer=18",
        ]);
        a.push(source.as_os_str().into());
        run(&ffmpeg, &a, &work, &cancel, false, &mut |_| {})?;
        let request = Request {
            sources: vec![source],
            output: work.path.join("never-published.mp4"),
            recursive: false,
            csv: false,
            settings: Settings {
                codec: Codec::H264,
                ..Default::default()
            },
        };
        let plan = prepare(request, Preferences::default(), &cancel, &mut |_| {})?;
        assert_eq!((plan.width, plan.height), (3840, 2160));
        assert!(plan.clips[0].hdr);
        for encoder in &plan.capabilities.as_ref().unwrap().encoders {
            eprintln!(
                "{}: {}",
                encoder.name,
                if encoder.reason.is_empty() {
                    "verified"
                } else {
                    &encoder.reason
                }
            );
        }
        let rows = benchmark(&plan, &cancel, &mut |_| {})?;
        assert!(!rows.is_empty());
        for row in &rows {
            eprintln!(
                "SPEED {}: {:?}× incomplete={} {}",
                row.encoder, row.speed, row.incomplete, row.reason
            );
            assert!(row.speed.is_some_and(|s| s.is_finite() && s > 0.0));
        }
        assert!(!plan.request.output.exists());
        let mut resized = plan.clone();
        resized.width = 1920;
        resized.height = 1080;
        resized.fps = "30".into();
        let rows = benchmark(&resized, &cancel, &mut |_| {})?;
        for row in rows {
            eprintln!("1080p30 SPEED {}: {:?}×", row.encoder, row.speed);
        }
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            benchmark(&plan, &cancel, &mut |_| {}).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        Ok(())
    }
}
