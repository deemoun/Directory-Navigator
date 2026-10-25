//! UI-owned request identities. Cancellation limits work; generations gate results.
use crate::desktop::WindowId;
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    Workspace,
    Document(WindowId),
    Panel(usize),
    Modal(crate::modals::ModalId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestToken {
    pub owner: Owner,
    pub slot: &'static str,
    pub generation: u64,
}
pub fn absolute_key(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut key = PathBuf::new();
    for c in absolute.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => key.push(".."),
            c => key.push(c.as_os_str()),
        }
    }
    key
}
pub fn destination_key(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        std::fs::canonicalize(path)
    } else {
        Ok(
            std::fs::canonicalize(path.parent().unwrap_or(Path::new(".")))?
                .join(path.file_name().unwrap_or_default()),
        )
    }
}

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, Weak,
};
struct ScopeState {
    owner: Owner,
    bound: bool,
    generations: HashMap<&'static str, u64>,
}
pub struct RequestScope {
    state: Arc<Mutex<ScopeState>>,
}
impl Default for RequestScope {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(ScopeState {
                owner: Owner::Workspace,
                bound: false,
                generations: HashMap::new(),
            })),
        }
    }
}
#[derive(Clone)]
pub(crate) struct RequestLease {
    state: Weak<Mutex<ScopeState>>,
    slot: &'static str,
    generation: u64,
}
impl RequestScope {
    pub fn new(owner: Owner) -> Self {
        let scope = Self::default();
        scope.bind(owner);
        scope
    }
    pub fn owner(&self) -> Owner {
        self.state.lock().expect("request scope").owner
    }
    pub fn bind(&self, owner: Owner) {
        let mut state = self.state.lock().expect("request scope");
        assert!(
            !state.bound || state.owner == owner,
            "request owner cannot change after binding"
        );
        state.owner = owner;
        state.bound = true;
    }
    pub fn accepts(&self, token: RequestToken) -> bool {
        let state = self.state.lock().expect("request scope");
        state.owner == token.owner && state.generations.get(token.slot) == Some(&token.generation)
    }
    pub fn finish(&self, token: RequestToken) {
        let mut state = self.state.lock().expect("request scope");
        if state.owner == token.owner
            && state.generations.get(token.slot) == Some(&token.generation)
        {
            state.generations.remove(token.slot);
        }
    }
    pub fn start(&self, slot: &'static str) -> RequestToken {
        self.lease(slot).token().expect("live owner")
    }
    pub(crate) fn lease(&self, slot: &'static str) -> RequestLease {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("request generations exhausted");
        self.state
            .lock()
            .expect("request scope")
            .generations
            .insert(slot, generation);
        RequestLease {
            state: Arc::downgrade(&self.state),
            slot,
            generation,
        }
    }
    pub fn spawn<T: Send + 'static>(
        &self,
        slot: &'static str,
        run: impl FnOnce(crate::task::Cancellation) -> T + Send + 'static,
    ) -> std::io::Result<crate::task::Task<T>> {
        crate::task::Task::spawn_owned(self, slot, run)
    }
}
impl RequestLease {
    pub fn token(&self) -> Option<RequestToken> {
        let scope = self.state.upgrade()?;
        let state = scope.lock().ok()?;
        (state.generations.get(self.slot) == Some(&self.generation)).then_some(RequestToken {
            owner: state.owner,
            slot: self.slot,
            generation: self.generation,
        })
    }
    pub fn finish(&self) {
        if let Some(scope) = self.state.upgrade() {
            if let Ok(mut state) = scope.lock() {
                if state.generations.get(self.slot) == Some(&self.generation) {
                    state.generations.remove(self.slot);
                }
            }
        }
    }
    pub fn current(&self) -> bool {
        self.token().is_some()
    }
}
