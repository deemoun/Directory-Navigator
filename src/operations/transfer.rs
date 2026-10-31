//! Queue snapshots pin identities, not file contents. Execution retains the
//! original no-replace engine and its ancestor/publication safeguards.
use super::*;
#[derive(Clone)]
pub(crate) struct Prepared {
    identities: Vec<(PathBuf, crate::platform_fs::EntryIdentity)>,
    pub(super) sources: Vec<Source>,
    pub(super) guards: DirectoryGuards,
    pub(crate) destination: PathBuf,
    container: bool,
}
impl Prepared {
    pub(crate) fn capture(plan: &Plan) -> io::Result<Self> {
        let mut guards = DirectoryGuards::default();
        let sources = sources(plan, &mut guards)?;
        let destination = crate::requests::destination_key(&plan.destination)?;
        let container = fs::metadata(&destination).is_ok_and(|m| m.is_dir());
        guards.capture_parent(&destination)?;
        if container {
            guards.capture(&destination)?;
        }
        let mut paths = std::collections::BTreeSet::new();
        for source in &sources {
            for path in source.path.ancestors() {
                paths.insert(path.to_owned());
            }
        }
        let anchor = if container {
            destination.as_path()
        } else {
            destination
                .parent()
                .ok_or_else(|| io::Error::other("Missing destination parent"))?
        };
        for path in anchor.ancestors() {
            paths.insert(path.to_owned());
        }
        let identities = paths
            .into_iter()
            .map(|path| crate::platform_fs::entry_identity(&path).map(|id| (path, id)))
            .collect::<io::Result<_>>()?;
        Ok(Self {
            identities,
            sources,
            guards,
            destination,
            container,
        })
    }
    pub(crate) fn validate(&self, plan: &Plan) -> io::Result<()> {
        for (path, id) in &self.identities {
            if crate::platform_fs::entry_identity(path)? != *id {
                return Err(io::Error::other(format!(
                    "Queued entry replaced: {}",
                    path.display()
                )));
            }
        }
        if self.sources.len() != plan.sources.len() {
            return Err(io::Error::other("Queued source list changed"));
        }
        if crate::requests::destination_key(&plan.destination)? != self.destination
            || fs::metadata(&self.destination).is_ok_and(|m| m.is_dir()) != self.container
        {
            return Err(io::Error::other(
                "Queued destination changed; transfer was not started",
            ));
        }
        self.guards.verify_parent(&self.destination)?;
        if self.container {
            self.guards.verify_directory(&self.destination)?;
        }
        for (source, requested) in self.sources.iter().zip(&plan.sources) {
            if destination_path(requested)? != source.path {
                return Err(io::Error::other("Queued source resolution changed"));
            }
            self.guards.verify_parent(&source.path)?;
            verify_entry(&source.path, &source.metadata)?;
        }
        Ok(())
    }
    fn total(&self, cancel: &AtomicBool) -> Option<u64> {
        let start = Instant::now();
        let mut guards = self.guards.clone();
        let mut stack = self
            .sources
            .iter()
            .map(|s| (s.path.clone(), 0))
            .collect::<Vec<_>>();
        let (mut total, mut count) = (0_u64, 0_usize);
        while let Some((path, depth)) = stack.pop() {
            if cancel.load(Ordering::Relaxed)
                || depth > 512
                || count >= 200_000
                || start.elapsed() > Duration::from_secs(2)
            {
                return None;
            }
            count += 1;
            guards.verify_parent(&path).ok()?;
            let m = fs::symlink_metadata(&path).ok()?;
            if m.is_file() {
                total = total.checked_add(m.len())?;
            } else if m.is_dir() {
                guards.capture(&path).ok()?;
                for child in fs::read_dir(path).ok()? {
                    if stack.len() >= 200_000
                        || cancel.load(Ordering::Relaxed)
                        || start.elapsed() > Duration::from_secs(2)
                    {
                        return None;
                    }
                    stack.push((child.ok()?.path(), depth + 1));
                }
            }
        }
        Some(total)
    }
}
impl Job {
    #[cfg(test)]
    pub(crate) fn controlled_transfer(
        id: crate::transfers::TransferId,
    ) -> (Self, mpsc::SyncSender<Event>) {
        let requests = crate::requests::RequestScope::new(crate::requests::Owner::Workspace);
        let token = requests.start("file-operation");
        let (sender, events) = mpsc::sync_channel(8);
        (
            Self {
                transfer: Some(id),
                requests,
                token,
                events,
                cancel: Arc::new(AtomicBool::new(false)),
                progress: Progress {
                    transfer: Some(id),
                    ..Default::default()
                },
                kind: Kind::Copy,
                focus: None,
            },
            sender,
        )
    }
    pub(crate) fn start_transfer(
        id: crate::transfers::TransferId,
        plan: Plan,
        options: CopyOptions,
        prepared: Option<Prepared>,
    ) -> io::Result<Self> {
        let requests = crate::requests::RequestScope::new(crate::requests::Owner::Workspace);
        let token = requests.start("file-operation");
        let (sender, events) = mpsc::sync_channel(8);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let kind = plan.kind;
        thread::Builder::new()
            .name("dn-transfer".into())
            .spawn(move || {
                let mut latest = Progress {
                    transfer: Some(id),
                    phase: Phase::Preparing,
                    path: plan.sources.first().cloned().unwrap_or_default(),
                    ..Default::default()
                };
                let _ = sender.try_send(Event::Progress(latest.clone()));
                let mut last = Instant::now() - Duration::from_secs(1);
                let result = (|| -> io::Result<Progress> {
                    check(&worker_cancel)?;
                    let prepared = match prepared {
                        Some(p) => p,
                        None => Prepared::capture(&plan)?,
                    };
                    prepared.validate(&plan)?;
                    latest.phase = Phase::Calculating;
                    let _ = sender.try_send(Event::Progress(latest.clone()));
                    let total = prepared.total(&worker_cancel);
                    check(&worker_cancel)?;
                    prepared.validate(&plan)?;
                    let mut progress = run_checked_expected(
                        &plan,
                        options,
                        &worker_cancel,
                        |p| {
                            let changed_phase = latest.phase != p.phase;
                            latest = p.clone();
                            latest.transfer = Some(id);
                            latest.total_bytes = total.filter(|t| p.bytes <= *t && p.skipped == 0);
                            if changed_phase || last.elapsed() >= Duration::from_millis(80) {
                                let _ = sender.try_send(Event::Progress(latest.clone()));
                                last = Instant::now();
                            }
                        },
                        MoveStrategy::PreferRename,
                        |path| crate::volume_space::read(path).map(|s| s.available),
                        Some(&prepared),
                    )?;
                    progress.transfer = Some(id);
                    progress.total_bytes =
                        total.filter(|t| progress.bytes <= *t && progress.skipped == 0);
                    Ok(progress)
                })()
                .map_err(|e| OperationFailure {
                    kind: e.kind(),
                    progress: Box::new(latest.clone()),
                    message: e.to_string(),
                });
                let _ = sender.send(Event::Done(result));
            })?;
        Ok(Self {
            transfer: Some(id),
            requests,
            token,
            events,
            cancel,
            progress: Progress {
                transfer: Some(id),
                ..Default::default()
            },
            kind,
            focus: None,
        })
    }
}
