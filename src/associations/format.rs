//! RDN-style extension mappings, with native whole-argument expansion.
use crate::user_menu::{
    format::words,
    runner::{CommandSpec, Plan},
};
use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub const MAX_BYTES: usize = 64 * 1024;
pub const DEFAULT: &str = "# Directory Navigator file associations\n# One final extension per rule, case-insensitive. <none> matches extensionless names.\n# Targets: @viewer, @hex, @editor, @default, or executable and arguments.\n# Whole arguments: %f = full file path, %d = directory, %n = file stem.\n# Quotes group arguments. Commands run without an implicit shell.\n# Examples (uncomment and adapt for your system):\n# .txt = @editor\n# .log = @viewer\n# .bin = @hex\n# .md = code --wait %f\n# .pdf = /usr/bin/open -a Preview %f\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Viewer,
    Hex,
    Editor,
    Default,
    Command(Vec<String>),
}
#[derive(Clone, Debug)]
pub struct Rule {
    pub extension: String,
    pub target: Target,
}
impl Rule {
    pub fn matches(&self, path: &Path) -> bool {
        match path.extension() {
            Some(ext) => ext
                .to_str()
                .is_some_and(|e| format!(".{}", e.to_lowercase()) == self.extension),
            None => self.extension == "<none>",
        }
    }
}
pub fn parse(text: &str) -> io::Result<Vec<Rule>> {
    if text.len() > MAX_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(io::Error::other(
            "Associations exceed 64 KiB or contain control characters",
        ));
    }
    let mut rules = Vec::new();
    let mut seen = HashSet::new();
    for (line, record) in text.lines().enumerate() {
        let record = record.trim();
        if record.is_empty() || record.starts_with(['#', ';']) {
            continue;
        }
        let error =
            |message: &str| io::Error::other(format!("Associations line {}: {message}", line + 1));
        let (extension, command) = record
            .split_once('=')
            .ok_or_else(|| error("Expected .ext = target"))?;
        let extension = extension.trim().to_lowercase();
        if extension != "<none>"
            && (!extension.starts_with('.')
                || extension.len() < 2
                || extension[1..]
                    .chars()
                    .any(|c| c.is_whitespace() || ".*/\\?%=:".contains(c)))
        {
            return Err(error("Use one final extension such as .txt, or <none>"));
        }
        if rules.len() >= 128 || !seen.insert(extension.clone()) {
            return Err(error("Duplicate extension or more than 128 rules"));
        }
        let args = words(command.trim()).map_err(|e| error(&e.to_string()))?;
        if args.is_empty() || args[0].is_empty() || args.len() > 256 {
            return Err(error("Expected a target and at most 256 arguments"));
        }
        let target = match args[0].as_str() {
            "@viewer" if args.len() == 1 => Target::Viewer,
            "@hex" if args.len() == 1 => Target::Hex,
            "@editor" if args.len() == 1 => Target::Editor,
            "@default" if args.len() == 1 => Target::Default,
            s if s.starts_with('@') => {
                return Err(error("Unknown internal target or unexpected arguments"))
            }
            _ => {
                if args[0].contains(['%', '{', '}']) {
                    return Err(error("Executable cannot contain placeholders"));
                }
                for arg in &args[1..] {
                    if !matches!(arg.as_str(), "%f" | "%d" | "%n")
                        && ["%f", "%d", "%n"].iter().any(|p| arg.contains(p))
                    {
                        return Err(error("%f, %d and %n must occupy a whole argument"));
                    }
                }
                Target::Command(args)
            }
        };
        rules.push(Rule { extension, target });
    }
    Ok(rules)
}
pub fn read(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    crate::filesystem::open_regular(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("Associations exceed 64 KiB"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("Associations must be UTF-8"))
}
pub fn load(path: &Path) -> io::Result<Vec<Rule>> {
    match read(path) {
        Ok(text) => parse(&text),
        Err(e)
            if e.kind() == io::ErrorKind::NotFound
                && fs::symlink_metadata(path)
                    .is_err_and(|e| e.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(Vec::new())
        }
        Err(e) => Err(e),
    }
}
pub fn command(args: &[String], path: &Path, cwd: PathBuf) -> io::Result<Plan> {
    let path = std::path::absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing file directory"))?;
    let stem = path
        .file_stem()
        .ok_or_else(|| io::Error::other("Missing file stem"))?;
    let mut expanded = Vec::new();
    for arg in args.iter().skip(1) {
        expanded.push(match arg.as_str() {
            "%f" => path.as_os_str().to_owned(),
            "%d" => parent.as_os_str().to_owned(),
            "%n" => stem.to_owned(),
            _ => OsString::from(arg),
        });
    }
    let program = args
        .first()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| io::Error::other("Missing executable"))?;
    Ok(Plan {
        cwd,
        commands: vec![CommandSpec {
            program: program.into(),
            args: expanded,
        }],
        env: Vec::new(),
        list_files: Vec::new(),
    })
}
