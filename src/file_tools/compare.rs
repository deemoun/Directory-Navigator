use crate::filesystem::Entry;
use crate::panel::Panel;
use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsString,
};

#[derive(Clone, Copy, Debug)]
pub struct CompareOptions {
    pub size: bool,
    pub time: bool,
    pub unselect: bool,
}
impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            size: true,
            time: true,
            unselect: false,
        }
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    pub left_only: usize,
    pub right_only: usize,
    pub different: usize,
    pub indeterminate: usize,
    pub differences: [BTreeSet<OsString>; 2],
    pub equivalent: [BTreeSet<OsString>; 2],
}
fn eligible(entry: &&Entry) -> bool {
    !entry.parent && !entry.directory
}
/// Only the current cached listing is inspected. No path is opened or followed.
pub fn compare(left: &[Entry], right: &[Entry], options: CompareOptions) -> Comparison {
    let mut result = Comparison::default();
    let right_index: HashMap<_, _> = right
        .iter()
        .filter(|e| !e.parent)
        .map(|e| (&e.name, e))
        .collect();
    let left_index: HashMap<_, _> = left
        .iter()
        .filter(|e| !e.parent)
        .map(|e| (&e.name, e))
        .collect();
    for a in left.iter().filter(eligible) {
        if let Some(b) = right_index.get(&a.name) {
            let type_differs = a.directory != b.directory
                || a.symlink != b.symlink
                || matches!((a.file_type, b.file_type), (Some(a), Some(b)) if a != b);
            let unknown = !type_differs
                && (a.file_type.is_none()
                    || b.file_type.is_none()
                    || (options.size && (a.size.is_none() || b.size.is_none()))
                    || (options.time && (a.modified.is_none() || b.modified.is_none())));
            let different = type_differs
                || unknown
                || (options.size && a.size != b.size)
                || (options.time && a.modified != b.modified);
            if unknown {
                result.indeterminate += 1;
            }
            if different {
                result.different += usize::from(!unknown);
                result.differences[0].insert(a.name.clone());
                if !b.directory {
                    result.differences[1].insert(b.name.clone());
                }
            } else {
                result.equivalent[0].insert(a.name.clone());
                result.equivalent[1].insert(b.name.clone());
            }
        } else {
            result.left_only += 1;
            result.differences[0].insert(a.name.clone());
        }
    }
    for b in right.iter().filter(eligible) {
        match left_index.get(&b.name) {
            None => {
                result.right_only += 1;
                result.differences[1].insert(b.name.clone());
            }
            Some(a) if a.directory => {
                result.different += 1;
                result.differences[1].insert(b.name.clone());
            }
            _ => {}
        }
    }
    result
}
pub fn apply(panels: &mut [Panel; 2], options: CompareOptions) -> Comparison {
    let result = compare(&panels[0].entries, &panels[1].entries, options);
    for (i, panel) in panels.iter_mut().enumerate() {
        if options.unselect {
            panel
                .selected
                .retain(|name| !result.equivalent[i].contains(name));
        } else {
            panel.replace_visible_marks(&result.differences[i]);
        }
    }
    result
}
