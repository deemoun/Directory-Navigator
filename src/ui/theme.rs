pub fn entry_color(entry: &crate::filesystem::Entry) -> u8 {
    crate::highlight_groups::builtin_color(entry)
}
