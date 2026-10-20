use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    Directory,
    Edit,
    View,
}
impl HistoryKind {
    pub fn index(self) -> usize {
        match self {
            Self::Directory => 0,
            Self::Edit => 1,
            Self::View => 2,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Directory => " History of directories ",
            Self::Edit => " File Edit History ",
            Self::View => " File View History ",
        }
    }
}
#[derive(Default)]
pub struct Histories {
    pub lists: [Vec<PathBuf>; 3],
}
impl Histories {
    pub fn record(&mut self, kind: HistoryKind, path: PathBuf) {
        let list = &mut self.lists[kind.index()];
        list.retain(|p| p != &path);
        list.insert(0, path);
        list.truncate(100);
    }
    pub fn get(&self, kind: HistoryKind) -> &[PathBuf] {
        &self.lists[kind.index()]
    }
}
