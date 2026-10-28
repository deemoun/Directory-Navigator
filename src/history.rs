//! Session MRU insertion only. Limits and identity semantics remain caller-owned.
pub fn remember<T: PartialEq>(entries: &mut Vec<T>, value: T, limit: usize) {
    entries.retain(|entry| entry != &value);
    entries.insert(0, value);
    entries.truncate(limit);
}
#[cfg(test)]
mod tests {
    #[test]
    fn duplicates_move_to_front_and_limits_do_not_change_identity() {
        let mut entries = vec!["a", "A", "a", "b"];
        super::remember(&mut entries, "a", 3);
        assert_eq!(entries, vec!["a", "A", "b"]);
        super::remember(&mut entries, "c", 2);
        assert_eq!(entries, vec!["c", "a"]);
        super::remember(&mut entries, "x", 0);
        assert!(entries.is_empty());
    }
}
