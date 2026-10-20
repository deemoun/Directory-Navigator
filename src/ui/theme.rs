pub fn entry_color(entry: &crate::filesystem::Entry) -> u8 {
    if entry.symlink {
        return 0x89;
    }
    if entry.directory {
        return 0x8f;
    }
    let extension = entry
        .path
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if entry.executable || matches!(extension.as_str(), "exe" | "com" | "bat" | "cmd" | "sh") {
        return 0x8a;
    }
    match extension.as_str() {
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" | "zst" => 0x8d,
        "rs" | "pas" | "asm" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "json" | "toml"
        | "yaml" | "yml" => 0x8b,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "mp3" | "wav" | "mp4" | "mkv" => 0x8c,
        "md" | "txt" | "pdf" | "docx" => 0x8f,
        _ => 0x87,
    }
}
