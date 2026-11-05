//! Optional Android SDK adapters. Literal argv, bounded output, cancellable owned
//! processes. APK inspection never starts tools or an ADB server implicitly.
use super::{
    binary::error,
    project::{Project, Stage},
};
use crate::{archive::extract::Destination, platform_fs, task::Cancellation};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Default, Debug)]
pub struct Tools {
    pub adb: Option<PathBuf>,
    pub zipalign: Option<PathBuf>,
    pub signer: Option<PathBuf>,
    pub java: Option<PathBuf>,
    pub keytool: Option<PathBuf>,
}
fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|p| p.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
}
impl Tools {
    pub fn discover() -> Self {
        let mut s = Self {
            adb: executable("adb"),
            zipalign: executable("zipalign"),
            java: executable("java"),
            keytool: executable("keytool"),
            signer: None,
        };
        if let Some(home) = std::env::var_os("JAVA_HOME") {
            let bin = PathBuf::from(home).join("bin");
            let java = bin.join(format!("java{}", std::env::consts::EXE_SUFFIX));
            let keytool = bin.join(format!("keytool{}", std::env::consts::EXE_SUFFIX));
            if java.is_file() {
                s.java = Some(java);
            }
            if keytool.is_file() {
                s.keytool = Some(keytool);
            }
        }
        let inferred = s
            .adb
            .as_ref()
            .and_then(|p| p.parent())
            .and_then(Path::parent)
            .map(|p| p.as_os_str().to_owned());
        let home =
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        let default_sdk = home.as_ref().map(|p| {
            p.join(if cfg!(target_os = "macos") {
                "Library/Android/sdk"
            } else if cfg!(windows) {
                "AppData/Local/Android/Sdk"
            } else {
                "Android/Sdk"
            })
            .into_os_string()
        });
        let roots = [
            std::env::var_os("ANDROID_HOME"),
            std::env::var_os("ANDROID_SDK_ROOT"),
            inferred,
            default_sdk,
        ];
        if cfg!(target_os = "macos") {
            let jbr =
                PathBuf::from("/Applications/Android Studio.app/Contents/jbr/Contents/Home/bin");
            if jbr.join("java").is_file()
                && (s.java.is_none() || s.java.as_deref() == Some(Path::new("/usr/bin/java")))
            {
                s.java = Some(jbr.join("java"));
                s.keytool = Some(jbr.join("keytool"));
            }
        }
        for root in roots.into_iter().flatten().map(PathBuf::from) {
            let adb = root
                .join("platform-tools")
                .join(format!("adb{}", std::env::consts::EXE_SUFFIX));
            if s.adb.is_none() && adb.is_file() {
                s.adb = Some(adb);
            }
            let mut builds = fs::read_dir(root.join("build-tools"))
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect::<Vec<_>>();
            builds.sort_by_cached_key(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .split('.')
                    .map(|s| s.parse::<u32>().unwrap_or(0))
                    .collect::<Vec<_>>()
            });
            for build in builds.into_iter().rev() {
                let zipalign = build.join(format!("zipalign{}", std::env::consts::EXE_SUFFIX));
                let signer = build.join("lib/apksigner.jar");
                if s.zipalign.is_none() && zipalign.is_file() {
                    s.zipalign = Some(zipalign);
                }
                if s.signer.is_none() && signer.is_file() {
                    s.signer = Some(signer);
                }
            }
        }
        s
    }
    pub fn signing_available(&self) -> bool {
        self.zipalign.is_some() && self.signer.is_some() && self.java.is_some()
    }
    fn adb_command(&self, serial: Option<&str>) -> io::Result<Command> {
        let mut c = Command::new(
            self.adb
                .as_ref()
                .ok_or_else(|| error("ADB is unavailable; configure Android SDK tools"))?,
        );
        if let Some(s) = serial {
            if s.is_empty() || s.starts_with('-') || s.chars().any(char::is_control) {
                return Err(error("Invalid device serial"));
            }
            c.arg("-s").arg(s);
        }
        Ok(c)
    }
    pub fn devices(&self, cancel: &Cancellation) -> io::Result<Vec<Device>> {
        let mut c = self.adb_command(None)?;
        c.args(["devices", "-l"]);
        let text = run(c, cancel, Duration::from_secs(20), None)?;
        Ok(text
            .lines()
            .skip(1)
            .filter_map(|l| {
                let mut fields = l.split_whitespace();
                Some(Device {
                    serial: fields.next()?.into(),
                    state: fields.next()?.into(),
                    description: fields.collect::<Vec<_>>().join(" "),
                })
            })
            .collect())
    }
    pub fn device_action(
        &self,
        serial: &str,
        action: DeviceAction,
        cancel: &Cancellation,
    ) -> io::Result<String> {
        let mut c = self.adb_command(Some(serial))?;
        match action {
            DeviceAction::Install(path) => {
                let mut input = crate::filesystem::open_regular(&path)?;
                let before = input.metadata()?;
                let stage = Stage::new(&fs::canonicalize(std::env::temp_dir())?)?;
                let snapshot = stage.path.join("install.apk");
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&snapshot)?;
                super::project::copy(&mut input, &mut output, cancel, 512 * 1024 * 1024)?;
                output.sync_all()?;
                drop(output);
                let after = input.metadata()?;
                if before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                    || !platform_fs::owns_file(&path, &input)
                {
                    return Err(error("APK changed before installation"));
                }
                c.arg("install").arg("-r").arg(snapshot);
                return run(c, cancel, Duration::from_secs(120), None);
            }
            DeviceAction::Start { package, activity } => {
                identifier(&package)?;
                identifier(&activity)?;
                c.args([
                    "shell",
                    "am",
                    "start",
                    "-n",
                    &format!("'{package}/{activity}'"),
                ]);
            }
            DeviceAction::Stop(package) => {
                identifier(&package)?;
                // adb shell joins argv on the device. Whitelisted identifiers may
                // contain '$', so quote this remote argument as well as host argv.
                c.args(["shell", "am", "force-stop", &format!("'{package}'")]);
            }
        }
        run(c, cancel, Duration::from_secs(120), None)
    }
    pub fn logcat(&self, serial: &str) -> io::Result<Logcat> {
        let mut c = self.adb_command(Some(serial))?;
        c.args(["logcat", "-v", "threadtime"]);
        Logcat::start(c)
    }
    pub fn create_test_key(&self, destination: &Path, cancel: &Cancellation) -> io::Result<()> {
        let receipt = Destination::capture(destination)?;
        if destination.exists() {
            return Err(error("Keystore destination already exists"));
        }
        let stage = Stage::new(
            destination
                .parent()
                .ok_or_else(|| error("Missing keystore parent"))?,
        )?;
        let key = stage.path.join("key.p12");
        let mut c = Command::new(
            self.keytool
                .as_ref()
                .ok_or_else(|| error("Java keytool is unavailable"))?,
        );
        c.args([
            "-genkeypair",
            "-noprompt",
            "-storetype",
            "PKCS12",
            "-alias",
            "androiddebugkey",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
            "-dname",
            "CN=DN APK Workshop Test",
            "-storepass:env",
            "DN_APK_PASSWORD",
            "-keypass:env",
            "DN_APK_PASSWORD",
            "-keystore",
        ])
        .arg(&key);
        run(c, cancel, Duration::from_secs(60), Some("android"))?;
        cancel.check()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600))?;
        }
        receipt.validate()?;
        stage.validate()?;
        platform_fs::rename_no_replace(&key, destination)?;
        platform_fs::sync_parent(destination)
    }
    pub fn sign(
        &self,
        project: &Project,
        destination: &Path,
        key: &Path,
        alias: &str,
        password: &str,
        cancel: &Cancellation,
    ) -> io::Result<()> {
        self.sign_with_key(
            project,
            destination,
            &SigningKey {
                path: key,
                alias,
                password,
                key_password: None,
            },
            cancel,
        )
    }
    pub fn sign_with_key(
        &self,
        project: &Project,
        destination: &Path,
        key: &SigningKey<'_>,
        cancel: &Cancellation,
    ) -> io::Result<()> {
        let alias = key.alias;
        let password = key.password;
        let key_password = key.key_password.unwrap_or(password);
        let key = key.path;
        if !self.signing_available() {
            return Err(error("Signing requires zipalign, apksigner.jar and Java"));
        }
        let _ = crate::filesystem::open_regular(key)?;
        if alias.is_empty() || alias.chars().any(char::is_control) {
            return Err(error("Specify a keystore alias"));
        }
        let receipt = Destination::capture(destination)?;
        if destination.exists() {
            return Err(error("APK destination already exists"));
        }
        let stage = Stage::new(
            destination
                .parent()
                .ok_or_else(|| error("Missing export parent"))?,
        )?;
        let unsigned = stage.path.join("unsigned.apk");
        let aligned = stage.path.join("aligned.apk");
        let signed = stage.path.join("signed.apk");
        project.build(&unsigned, cancel)?;
        let mut c = Command::new(self.zipalign.as_ref().unwrap());
        c.args(["-P", "16", "4"]).arg(&unsigned).arg(&aligned);
        run(c, cancel, Duration::from_secs(120), None)?;
        let signer = || {
            let mut c = Command::new(self.java.as_ref().unwrap());
            c.arg("-jar").arg(self.signer.as_ref().unwrap());
            c
        };
        let mut c = signer();
        c.args(["sign", "--ks"])
            .arg(key)
            .args([
                "--ks-key-alias",
                alias,
                "--ks-pass",
                "env:DN_APK_PASSWORD",
                "--key-pass",
                "env:DN_APK_KEY_PASSWORD",
                "--out",
            ])
            .arg(&signed)
            .arg(&aligned);
        c.env("DN_APK_KEY_PASSWORD", key_password);
        run_redacted(
            c,
            cancel,
            Duration::from_secs(120),
            Some(password),
            Some(key_password),
        )?;
        let mut c = signer();
        c.args(["verify", "--verbose"]).arg(&signed);
        run(c, cancel, Duration::from_secs(60), None)?;
        let mut c = Command::new(self.zipalign.as_ref().unwrap());
        c.args(["-c", "-P", "16", "4"]).arg(&signed);
        run(c, cancel, Duration::from_secs(60), None)?;
        cancel.check()?;
        receipt.validate()?;
        stage.validate()?;
        platform_fs::rename_no_replace(&signed, destination)?;
        platform_fs::sync_parent(destination)
    }
}
/// Borrowed signing credentials; callers retain/zeroize their owned secrets.
pub struct SigningKey<'a> {
    pub path: &'a Path,
    pub alias: &'a str,
    pub password: &'a str,
    pub key_password: Option<&'a str>,
}
fn identifier(s: &str) -> io::Result<()> {
    if s.is_empty()
        || s.len() > 512
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$'))
    {
        Err(error("Invalid Android package or component name"))
    } else {
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub description: String,
}
#[derive(Clone)]
pub enum DeviceAction {
    Install(PathBuf),
    Start { package: String, activity: String },
    Stop(String),
}
fn spawn(mut c: Command, password: Option<&str>) -> io::Result<Process> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = password {
        c.env("DN_APK_PASSWORD", p);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let mut child = c.spawn()?;
    match super::job::Job::new(&child) {
        Ok(job) => Ok(Process {
            child,
            _job: job,
            finished: false,
        }),
        Err(e) => {
            stop(&mut child);
            Err(e)
        }
    }
}
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}
struct Process {
    child: Child,
    finished: bool,
    _job: super::job::Job,
}
impl std::ops::Deref for Process {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl std::ops::DerefMut for Process {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !self.finished {
            stop(&mut self.child);
        }
    }
}
fn collect(mut r: impl Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
    let (send, reply) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            let mut b = [0; 4096];
            loop {
                let n = r.read(&mut b)?;
                if n == 0 {
                    return Ok(bytes);
                }
                let take = n.min((1024 * 1024usize).saturating_sub(bytes.len()));
                bytes.extend_from_slice(&b[..take]);
            }
        })();
        let _ = send.send(result);
    });
    reply
}
pub(super) fn run(
    c: Command,
    cancel: &Cancellation,
    timeout: Duration,
    password: Option<&str>,
) -> io::Result<String> {
    run_redacted(c, cancel, timeout, password, None)
}
fn run_redacted(
    c: Command,
    cancel: &Cancellation,
    timeout: Duration,
    password: Option<&str>,
    additional: Option<&str>,
) -> io::Result<String> {
    cancel.check()?;
    let mut child = spawn(c, password)?;
    let stdout = collect(child.stdout.take().unwrap());
    let stderr = collect(child.stderr.take().unwrap());
    let start = Instant::now();
    let mut out = None;
    let mut err = None;
    let status = loop {
        cancel.check()?;
        if out.is_none() {
            match stdout.try_recv() {
                Ok(v) => out = Some(v?),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(error("Tool output reader stopped"))
                }
                Err(_) => {}
            }
        }
        if err.is_none() {
            match stderr.try_recv() {
                Ok(v) => err = Some(v?),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(error("Tool error reader stopped"))
                }
                Err(_) => {}
            }
        }
        // Do not reap the leader while descendant-held pipes are open: its PID
        // reserves the process group until cancellation can safely terminate it.
        if out.is_some() && err.is_some() {
            if let Some(status) = child.try_wait()? {
                child.finished = true;
                break status;
            }
        }
        if start.elapsed() > timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Android tool timed out",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let mut data = out.unwrap();
    data.extend(err.unwrap());
    let mut text = String::from_utf8_lossy(&data).into_owned();
    for secret in [password, additional]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
    {
        text = text.replace(secret, "[redacted]");
    }
    if !status.success() {
        return Err(io::Error::other(format!(
            "Android tool failed ({status}): {text}"
        )));
    }
    Ok(text)
}
pub struct Logcat {
    child: Process,
    rx: Receiver<String>,
    pub lines: VecDeque<String>,
    pub disconnected: bool,
}
impl Logcat {
    fn start(c: Command) -> io::Result<Self> {
        let mut child = spawn(c, None)?;
        let mut stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let _ = collect(stderr);
        let (tx, rx) = mpsc::sync_channel(128);
        thread::spawn(move || {
            let mut pending = Vec::new();
            let mut chunk = [0; 4096];
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                for &b in &chunk[..n] {
                    if b == b'\n' {
                        let line = String::from_utf8_lossy(&pending).into_owned();
                        if matches!(tx.try_send(line), Err(mpsc::TrySendError::Disconnected(_))) {
                            return;
                        }
                        pending.clear();
                    } else if pending.len() < 8192 {
                        pending.push(b);
                    }
                }
            }
        });
        Ok(Self {
            child,
            rx,
            lines: VecDeque::new(),
            disconnected: false,
        })
    }
    pub fn poll(&mut self) {
        for _ in 0..256 {
            match self.rx.try_recv() {
                Ok(line) => {
                    if self.lines.len() == 2000 {
                        self.lines.pop_front();
                    }
                    self.lines.push_back(line);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.child.finished {
                        stop(&mut self.child);
                        self.child.finished = true;
                    }
                    self.disconnected = true;
                    break;
                }
                Err(_) => break,
            }
        }
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let receipt = Destination::capture(path)?;
        let stage = Stage::new(path.parent().ok_or_else(|| error("Missing log parent"))?)?;
        let temp = stage.path.join("logcat.txt");
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        use std::io::Write;
        for line in &self.lines {
            writeln!(f, "{line}")?;
        }
        f.sync_all()?;
        receipt.validate()?;
        platform_fs::rename_no_replace(&temp, path)?;
        platform_fs::sync_parent(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_process_fixture() {
        let Ok(mode) = std::env::var("DN_APK_FIXTURE") else {
            return;
        };
        if mode == "hang" {
            loop {
                thread::park_timeout(Duration::from_secs(30));
            }
        }
        println!(
            "fake output {}",
            std::env::var("DN_APK_PASSWORD").unwrap_or_default()
        );
        if mode == "failure" {
            std::process::exit(7);
        }
    }
    fn command(mode: &str) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "apk::tools::tests::sdk_process_fixture",
            "--nocapture",
        ])
        .env("DN_APK_FIXTURE", mode);
        c
    }
    #[test]
    fn sdk_adapter_redacts_secrets_and_bounds_failure_and_timeout() -> io::Result<()> {
        let c = Cancellation::default();
        let text = run(
            command("success"),
            &c,
            Duration::from_secs(10),
            Some("PRIVATE_TEST_PASSWORD"),
        )?;
        assert!(text.contains("[redacted]"));
        assert!(!text.contains("PRIVATE_TEST_PASSWORD"));
        let error = run(
            command("failure"),
            &c,
            Duration::from_secs(10),
            Some("PRIVATE_TEST_PASSWORD"),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("PRIVATE_TEST_PASSWORD"));
        assert!(error.to_string().contains("Android tool failed"));
        assert_eq!(
            run(command("hang"), &c, Duration::from_millis(30), None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        c.cancel();
        assert_eq!(
            run(command("success"), &c, Duration::from_secs(10), None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(Tools::default().devices(&Cancellation::default()).is_err());
        Ok(())
    }
}
