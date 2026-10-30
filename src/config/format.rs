//! TOML v2 only: typed preferences with unknown-value preservation.
use super::Config;
use std::io;
use toml::{Table, Value};
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn records(config: &Config) -> Vec<(&'static str, String, Value)> {
    let sort = |mode| match mode {
        crate::panel::SortMode::Name => "name",
        crate::panel::SortMode::Extension => "extension",
        crate::panel::SortMode::Size => "size",
        crate::panel::SortMode::Date => "date",
    };
    let mut fields = Vec::new();
    for setting in crate::settings::definition::ALL {
        let field = setting.definition();
        fields.push((
            field.group,
            field.key.to_owned(),
            Value::from((field.get)(config)),
        ));
    }
    macro_rules! add {
        ($group:literal,$key:expr,$value:expr) => {
            fields.push(($group, $key.to_owned(), Value::from($value)));
        };
    }
    add!("application", "shortcuts", config.shortcuts.key());
    add!("interface", "palette", config.palette.key());
    add!("regional", "language", config.language.key());
    add!("regional", "date_order", config.datetime.date_order.key());
    add!(
        "regional",
        "date_separator",
        config.datetime.date_separator.to_string()
    );
    add!(
        "regional",
        "time_format",
        if config.datetime.twelve_hour {
            12i64
        } else {
            24i64
        }
    );
    add!(
        "regional",
        "time_separator",
        config.datetime.time_separator.to_string()
    );
    add!("screensaver", "minutes", i64::from(config.saver_minutes));
    add!("panels.left", "sort", sort(config.sorts[0]));
    add!("panels.right", "sort", sort(config.sorts[1]));
    add!("panels.left", "layout", config.layouts[0].key());
    add!("panels.right", "layout", config.layouts[1].key());
    add!("panels.left", "columns", config.columns[0].key());
    add!("panels.right", "columns", config.columns[1].key());
    add!("editor", "auto_indent", config.editor.auto_indent);
    add!("editor", "syntax", config.editor.syntax);
    add!("editor", "expand_tabs", config.editor.expand_tabs);
    add!("editor", "tab_size", i64::from(config.editor.tab_size));
    for (index, path) in config.bookmarks.iter().enumerate() {
        add!(
            "bookmarks",
            (index + 1).to_string(),
            path.as_ref()
                .map_or_else(String::new, |path| crate::bookmarks::encode(path))
        );
    }
    for item in crate::palette::theme::ITEMS {
        add!(
            "colors",
            item.key,
            i64::from(config.palette.slot(item.slot))
        );
    }
    fields
}
fn checked(group: &str, key: &str, value: &Value, default: &Value) -> io::Result<String> {
    let text = match default {
        Value::Boolean(_) => value.as_bool().map(|v| v.to_string()),
        Value::Integer(_) => value.as_integer().map(|v| v.to_string()),
        _ => value.as_str().map(str::to_owned),
    }
    .ok_or_else(|| invalid(format!("Invalid type for {group}.{key}")))?;
    if group == "colors" && text.parse::<u8>().is_err() {
        return Err(invalid(format!(
            "Invalid color attribute for {key}: expected 0..255"
        )));
    }
    let allowed: Option<&[&str]> = match (group, key) {
        ("application", "shortcuts") => Some(&["auto", "classic_dn", "mac"]),
        ("interface", "palette") => Some(&["classic", "terminal"]),
        ("regional", "language") => Some(&["en", "ru"]),
        ("regional", "date_order") => Some(&["dmy", "mdy", "ymd"]),
        ("regional", "date_separator") => Some(&["-", ".", "/"]),
        ("regional", "time_separator") => Some(&[":", "."]),
        ("regional", "time_format") => Some(&["12", "24"]),
        ("panels.left" | "panels.right", "sort") => Some(&["name", "extension", "size", "date"]),
        ("panels.left" | "panels.right", "layout") => Some(&["brief", "details"]),
        ("panels.left" | "panels.right", "columns") => {
            Some(&["name", "name_size", "name_size_modified"])
        }
        _ => None,
    };
    if text.contains(['\n', '\r'])
        || allowed.is_some_and(|values| !values.contains(&text.as_str()))
        || (group, key) == ("editor", "tab_size")
            && !text.parse::<u8>().is_ok_and(|n| (1..=16).contains(&n))
        || (group, key) == ("screensaver", "minutes")
            && !text
                .parse::<u8>()
                .is_ok_and(|n| crate::screensaver::DELAYS.contains(&n))
    {
        return Err(invalid(format!("Invalid value for {group}.{key}")));
    }
    Ok(text)
}
fn assign(config: &mut Config, group: &str, key: &str, text: &str) -> io::Result<()> {
    for setting in crate::settings::definition::ALL {
        let field = setting.definition();
        if (group, key) == (field.group, field.key) {
            (field.set)(config, text == "true");
            return Ok(());
        }
    }
    if group == "colors" {
        if let Some(item) = crate::palette::theme::ITEMS.iter().find(|i| i.key == key) {
            config.palette = config.palette.with_slot(
                item.slot,
                text.parse()
                    .map_err(|_| invalid("Invalid color attribute"))?,
            );
        }
        return Ok(());
    }
    let index = usize::from(group == "panels.right");
    match (group, key) {
        ("application", "shortcuts") => config.shortcuts = crate::shortcuts::Profile::parse(text),
        ("interface", "palette") => config.palette = crate::palette::PaletteProfile::parse(text),
        ("regional", "language") => config.language = crate::localization::Language::parse(text),
        ("regional", "date_order") => {
            config.datetime.date_order = crate::regional::DateOrder::parse(text)
        }
        ("regional", "date_separator") => {
            config.datetime.date_separator = text
                .chars()
                .next()
                .ok_or_else(|| invalid("Missing date separator"))?
        }
        ("regional", "time_separator") => {
            config.datetime.time_separator = text
                .chars()
                .next()
                .ok_or_else(|| invalid("Missing time separator"))?
        }
        ("regional", "time_format") => config.datetime.twelve_hour = text == "12",
        ("screensaver", "minutes") => {
            config.saver_minutes = text
                .parse()
                .map_err(|_| invalid("Invalid screensaver delay"))?
        }
        ("panels.left" | "panels.right", "sort") => {
            config.sorts[index] = match text {
                "name" => crate::panel::SortMode::Name,
                "extension" => crate::panel::SortMode::Extension,
                "size" => crate::panel::SortMode::Size,
                _ => crate::panel::SortMode::Date,
            }
        }
        ("panels.left" | "panels.right", "layout") => {
            config.layouts[index] = crate::panel_layout::PanelLayout::parse(text)
        }
        ("panels.left" | "panels.right", "columns") => {
            config.columns[index] = crate::columns::ColumnPreset::parse(text)
        }
        ("editor", "auto_indent") => config.editor.auto_indent = text == "true",
        ("editor", "syntax") => config.editor.syntax = text == "true",
        ("editor", "expand_tabs") => config.editor.expand_tabs = text == "true",
        ("editor", "tab_size") => {
            config.editor.tab_size = text.parse().map_err(|_| invalid("Invalid tab width"))?
        }
        ("bookmarks", slot) => {
            if let Some(index) = slot.parse::<usize>().ok().filter(|n| (1..=9).contains(n)) {
                config.bookmarks[index - 1] = crate::bookmarks::decode(text);
            }
        }
        _ => {}
    }
    Ok(())
}
fn document(bytes: &[u8]) -> io::Result<Table> {
    let text = std::str::from_utf8(bytes).map_err(|e| invalid(e.to_string()))?;
    let root = text.parse::<Table>().map_err(|e| invalid(e.to_string()))?;
    if root.get("config_version").and_then(Value::as_integer) != Some(2) {
        return Err(invalid(
            "Unsupported configuration version (expected config_version = 2)",
        ));
    }
    Ok(root)
}
fn section<'a>(root: &'a Table, group: &str) -> io::Result<Option<&'a Table>> {
    let mut table = root;
    for name in group.split('.') {
        match table.get(name) {
            Some(Value::Table(next)) => table = next,
            Some(_) => return Err(invalid(format!("Expected a settings section: {group}"))),
            None => return Ok(None),
        }
    }
    Ok(Some(table))
}
fn set(root: &mut Table, group: &str, key: String, value: Value) -> io::Result<()> {
    let mut table = root;
    for name in group.split('.') {
        table = table
            .entry(name.to_owned())
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| invalid(format!("Expected a settings section: {group}")))?;
    }
    table.insert(key, value);
    Ok(())
}
fn plugin_ids(value: &Value) -> io::Result<std::collections::BTreeSet<String>> {
    value
        .as_array()
        .ok_or_else(|| invalid("Invalid type for plugin enable/disable list"))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| invalid("Invalid plugin ID in enable/disable list"))
        })
        .collect()
}
pub(super) fn parse(bytes: &[u8]) -> io::Result<Config> {
    if bytes.len() > super::store::MAX_CONFIG_BYTES as usize {
        return Err(invalid("Configuration exceeds 1 MiB"));
    }
    let root = document(bytes)?;
    let mut config = Config::default();
    for (group, key, default) in records(&Config::default()) {
        if let Some(value) = section(&root, group)?.and_then(|table| table.get(&key)) {
            assign(
                &mut config,
                group,
                &key,
                &checked(group, &key, value, &default)?,
            )?;
        }
    }
    if let Some(value) = section(&root, "plugins")?.and_then(|t| t.get("disabled")) {
        config.disabled_plugins = plugin_ids(value)?;
    }
    if let Some(value) = section(&root, "plugins")?.and_then(|t| t.get("enabled")) {
        config.enabled_plugins = plugin_ids(value)?;
    }
    if !config.enabled_plugins.is_disjoint(&config.disabled_plugins) {
        return Err(invalid("A plugin cannot be both enabled and disabled"));
    }
    Ok(config)
}
pub(super) fn encode(config: &Config, previous: Option<&[u8]>) -> io::Result<Vec<u8>> {
    let mut root = match previous {
        Some(bytes) => {
            parse(bytes)?;
            document(bytes)?
        }
        None => Table::new(),
    };
    root.insert("config_version".into(), Value::Integer(2));
    if let Some(table) = root.get_mut("bookmarks").and_then(Value::as_table_mut) {
        for slot in 1..=9 {
            table.remove(&slot.to_string());
        }
    }
    let disabled = Value::Array(
        config
            .disabled_plugins
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    );
    plugin_ids(&disabled)?;
    set(&mut root, "plugins", "disabled".into(), disabled)?;
    let enabled = Value::Array(
        config
            .enabled_plugins
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    );
    plugin_ids(&enabled)?;
    if !config.enabled_plugins.is_disjoint(&config.disabled_plugins) {
        return Err(invalid("A plugin cannot be both enabled and disabled"));
    }
    set(&mut root, "plugins", "enabled".into(), enabled)?;
    for (group, key, value) in records(config) {
        if group == "bookmarks" && value.as_str() == Some("") {
            continue;
        }
        checked(group, &key, &value, &value)?;
        set(&mut root, group, key, value)?;
    }
    root.entry("bookmarks".to_owned())
        .or_insert_with(|| Value::Table(Table::new()));
    let text = toml::to_string_pretty(&root).map_err(|e| invalid(e.to_string()))?;
    Ok(format!("# Directory Navigator preferences\n# Saved on normal exit; Options > Save desktop saves now.\n{text}").into_bytes())
}
