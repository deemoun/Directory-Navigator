//! DN's >level heading format, with explicit argv commands instead of DOS batches.
use std::io;
pub const MAX_BYTES: usize = 128 * 1024;
#[derive(Clone, Debug)]
pub struct Item {
    pub label: String,
    pub commands: Vec<Vec<String>>,
    pub children: Vec<Item>,
    pub prompts: Vec<Parameter>,
}
#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub label: String,
    pub default: String,
}
impl Item {
    pub fn separator(&self) -> bool {
        self.label == "-"
    }
}
pub fn words(line: &str) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if escape {
            word.push(c);
            escape = false;
            continue;
        }
        if c == '\\'
            && quote != Some('\'')
            && chars.peek().is_some_and(|next| {
                *next == '\\'
                    || *next == '"'
                    || (quote.is_none() && (*next == '\'' || next.is_whitespace()))
            })
        {
            escape = true;
            started = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                started = true;
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(c);
                started = true;
            }
        }
    }
    if escape || quote.is_some() {
        return Err(io::Error::other("Unclosed quote or trailing escape"));
    }
    if started {
        out.push(word);
    }
    if out.len() > 2048 {
        return Err(io::Error::other("Too many command arguments"));
    }
    Ok(out)
}
pub fn parse(text: &str) -> io::Result<Vec<Item>> {
    if text.len() > MAX_BYTES || text.contains('\0') {
        return Err(io::Error::other("Menu exceeds 128 KiB or contains NUL"));
    }
    let mut flat: Vec<(usize, Item)> = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let error = |s: &str| io::Error::other(format!("Menu line {}: {s}", line_no + 1));
        if let Some(heading) = line.strip_prefix('>') {
            let Some((level, label)) = heading.split_once(char::is_whitespace) else {
                return Err(error("Expected >level label"));
            };
            let level = level.parse::<usize>().map_err(|_| error("Invalid level"))?;
            let label = label.trim();
            let previous = flat.last().map(|(n, _)| *n).unwrap_or(0);
            if level == 0
                || level > 8
                || level > previous + 1
                || label.is_empty()
                || label.len() > 256
                || label.chars().any(char::is_control)
                || flat.len() >= 256
            {
                return Err(error("Invalid heading, nesting (1–8), or item limit (256)"));
            }
            if level > previous
                && flat.last().is_some_and(|(_, i)| {
                    !i.commands.is_empty() || !i.prompts.is_empty() || i.separator()
                })
            {
                return Err(error(
                    "A submenu heading cannot contain commands or be a separator",
                ));
            }
            flat.push((
                level,
                Item {
                    label: label.into(),
                    commands: Vec::new(),
                    children: Vec::new(),
                    prompts: Vec::new(),
                },
            ));
        } else {
            let Some((_, item)) = flat.last_mut() else {
                return Err(error("Command before first heading"));
            };
            if item.separator() || item.commands.len() >= 32 {
                return Err(error("Separator command or more than 32 commands"));
            }
            let args = words(line).map_err(|e| error(&e.to_string()))?;
            if args.first().is_some_and(|s| s == "?") {
                if !(3..=4).contains(&args.len())
                    || item.prompts.len() >= 8
                    || args[1].is_empty()
                    || args[1].len() > 32
                    || !args[1]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || args[2].len() > 120
                    || args[2].is_empty()
                    || args[2].chars().any(char::is_control)
                    || args.get(3).is_some_and(|s| s.chars().any(char::is_control))
                    || args.get(3).is_some_and(|s| s.len() > 4096)
                    || item.prompts.iter().any(|p| p.name == args[1])
                {
                    return Err(error(
                        "Expected ? name 'Label' 'default' (up to eight unique parameters)",
                    ));
                }
                item.prompts.push(Parameter {
                    name: args[1].clone(),
                    label: args[2].clone(),
                    default: args.get(3).cloned().unwrap_or_default(),
                });
                continue;
            }
            if args.is_empty() || args[0].is_empty() {
                return Err(error("Missing executable"));
            }
            item.commands.push(args);
        }
    }
    fn group(
        items: &mut std::iter::Peekable<std::vec::IntoIter<(usize, Item)>>,
        level: usize,
    ) -> Vec<Item> {
        let mut out = Vec::new();
        while items.peek().is_some_and(|(n, _)| *n == level) {
            let Some((_, mut item)) = items.next() else {
                break;
            };
            if items.peek().is_some_and(|(n, _)| *n > level) {
                item.children = group(items, level + 1);
            }
            out.push(item);
        }
        out
    }
    let result = group(&mut flat.into_iter().peekable(), 1);
    if result.is_empty() {
        return Err(io::Error::other("Menu has no items"));
    }
    Ok(result)
}
#[cfg(not(windows))]
pub const DEFAULT:&str="; DN user menu: >level ~hotkey~ label; commands are argv, not shell scripts.\n; Placeholders must occupy a whole argument: {file} {name} {dir}\n; {other_file} {other_dir} {selected} {other_selected}. Quotes group arguments.\n; {selected_list}/{other_selected_list}: private lists created only on Run.\n; ? value 'Label' 'default' declares a {param:value} input.\n; Use an explicit shell for pipelines; native DN_* environment values are available.\n>1 ~P~rint directory\nprintf '%s\\n' '{dir}'\n>1 ~F~ile tools\n>2 ~L~ist marked paths\nprintf '%s\\n' '{selected}'\n>2 File ~i~nformation\nls -ld -- '{file}'\n>1 -\n>1 ~S~hell\n'{shell}'\n";
#[cfg(windows)]
pub const DEFAULT:&str="; DN user menu: commands are argv, not implicit shell scripts.\n; Whole arguments: {file} {name} {dir} {other_file} {other_dir} {selected}.\n>1 ~P~anel paths\ncmd /D /C 'set DN_'\n>1 ~S~hell\n'{shell}'\n";
