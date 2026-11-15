use super::{
    obex::{self, Cancel, Link},
    Offer, Progress, Source,
};
use std::{
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
pub struct JobContext {
    pub cancel: Cancel,
    pub progress: Arc<Mutex<Progress>>,
    pub approvals: mpsc::Receiver<(u64, bool, PathBuf)>,
    pub conflicts: crate::operations::conflict::Mailbox,
}
impl JobContext {
    pub fn update(&self, phase: &str, done: u64, total: u64) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.phase = phase.into();
        p.done = done;
        p.total = total;
    }
    fn completed(&self) {
        let mut p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        p.completed += 1;
    }
    fn approve(
        &self,
        name: String,
        size: u64,
        peer: &str,
        mut pump: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Option<PathBuf>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .offer = Some(Offer {
            id,
            name: name.clone(),
            size,
            peer: peer.chars().take(256).collect(),
        });
        self.update("Waiting for receiver approval", 0, size);
        let answer = loop {
            obex::check(&self.cancel)?;
            if let Err(e) = pump() {
                break Err(e);
            }
            match self.approvals.recv_timeout(Duration::from_millis(100)) {
                Ok((serial, accept, path)) if serial == id => {
                    break if accept { Ok(Some(path)) } else { Ok(None) }
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(e) => break Err(io::Error::other(e)),
            }
        };
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .offer = None;
        answer
    }
}
// Legacy peers get one 110-second budget, leaving room for the 120-second
// transport timeout. Negotiated peers get ten minutes with five-second pulses.
struct DecisionWait {
    deadline: Instant,
    next_pulse: Instant,
    keepalive: bool,
}
impl DecisionWait {
    fn new(now: Instant, keepalive: bool) -> Self {
        Self {
            deadline: now
                + if keepalive {
                    obex::DECISION_WAIT
                } else {
                    Duration::from_secs(110)
                },
            next_pulse: now,
            keepalive,
        }
    }
    fn pump(&mut self, link: &mut dyn Link, cancel: &Cancel, now: Instant) -> io::Result<()> {
        obex::check(cancel)?;
        if now >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Bluetooth receiver decision timed out",
            ));
        }
        if self.keepalive && now >= self.next_pulse {
            obex::write(link, 0x90, obex::WAIT_HEADER, cancel)?;
            self.next_pulse = now + Duration::from_secs(5);
        }
        Ok(())
    }
}
fn decision_response(link: &mut dyn Link, cancel: &Cancel, keepalive: bool) -> io::Result<u8> {
    let deadline = Instant::now() + obex::DECISION_WAIT + Duration::from_secs(30);
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Bluetooth receiver decision timed out",
            ));
        }
        let (code, body) = obex::read(link, cancel, obex::MTU)?;
        let marked = obex::headers(&body)?.waiting;
        if !marked {
            return Ok(code);
        }
        if !keepalive || code != 0x90 || body != obex::WAIT_HEADER {
            return Err(obex::invalid("Unexpected Bluetooth wait response"));
        }
    }
}
fn source(file: &Source) -> io::Result<fs::File> {
    let handle = crate::filesystem::open_regular(&file.path)?;
    let meta = handle.metadata()?;
    super::regular(&meta)?;
    if !crate::platform_fs::same_identity(&meta, &file.metadata)
        || meta.len() != file.metadata.len()
        || meta.modified().ok() != file.metadata.modified().ok()
        || !crate::platform_fs::owns_file(&file.path, &handle)
    {
        return Err(io::Error::other(
            "Bluetooth source changed; capture the selection again",
        ));
    }
    Ok(handle)
}
pub fn send(link: &mut dyn Link, files: &[Source], ctx: &JobContext) -> io::Result<()> {
    let mut greeting = vec![0x10, 0, (obex::MTU >> 8) as u8, obex::MTU as u8];
    greeting.extend(obex::WAIT_HEADER);
    obex::write(link, 0x80, &greeting, &ctx.cancel)?;
    let connected = obex::response(link, &ctx.cancel, 0xa0)?;
    if connected.len() < 4 || connected[0] >> 4 != 1 {
        return Err(obex::invalid("Incompatible OBEX connection response"));
    }
    let mtu = usize::from(u16::from_be_bytes([connected[2], connected[3]])).min(obex::MTU);
    if mtu < 255 {
        return Err(obex::invalid("Invalid OBEX receive capacity"));
    }
    let capabilities = obex::headers(&connected[4..])?;
    let connection = capabilities.connection;
    for file in files {
        let mut input = source(file)?;
        let mut head = Vec::new();
        if let Some(id) = connection {
            head.push(0xcb);
            head.extend(id.to_be_bytes());
        }
        head.extend(obex::name_header(&file.name)?);
        head.push(0xc3);
        head.extend((file.metadata.len() as u32).to_be_bytes());
        if head.len() + 3 > mtu {
            return Err(obex::invalid(
                "Filename exceeds receiver's OBEX packet size",
            ));
        }
        ctx.update("Waiting for receiver approval", 0, file.metadata.len());
        obex::write(link, 0x02, &head, &ctx.cancel)?;
        let response = decision_response(link, &ctx.cancel, capabilities.waiting)?;
        if response == 0xc3 {
            ctx.progress
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .skipped += 1;
            continue;
        }
        if response != 0x90 {
            return Err(io::Error::other(format!(
                "Receiver rejected Bluetooth file (OBEX {response:02X})"
            )));
        }
        let overhead = if connection.is_some() { 11 } else { 6 };
        let mut buffer = vec![0; mtu - overhead];
        let mut done = 0;
        loop {
            obex::check(&ctx.cancel)?;
            let remaining = file.metadata.len() - done;
            let count = buffer.len().min(remaining as usize);
            input.read_exact(&mut buffer[..count])?;
            done += count as u64;
            let final_packet = done == file.metadata.len();
            if final_packet {
                let _ = source(file)?;
                let mut extra = [0];
                if input.read(&mut extra)? != 0 {
                    return Err(io::Error::other("Bluetooth source grew during sending"));
                }
            }
            let mut body = Vec::new();
            if let Some(id) = connection {
                body.push(0xcb);
                body.extend(id.to_be_bytes());
            }
            body.push(if final_packet { 0x49 } else { 0x48 });
            body.extend(((count + 3) as u16).to_be_bytes());
            body.extend(&buffer[..count]);
            ctx.update("Sending Bluetooth file", done, file.metadata.len());
            let result = obex::write(
                link,
                if final_packet { 0x82 } else { 0x02 },
                &body,
                &ctx.cancel,
            )
            .and_then(|()| {
                obex::response(link, &ctx.cancel, if final_packet { 0xa0 } else { 0x90 })
                    .map(|_| ())
            });
            if let Err(error) = result {
                if final_packet {
                    let message = format!(
                        "Delivery unconfirmed: the receiver may have published this file. {error}"
                    );
                    return Err(io::Error::new(error.kind(), message));
                }
                return Err(error);
            }
            if final_packet {
                ctx.completed();
                break;
            }
        }
    }
    // Every completed file already has its own final publication receipt.
    if let Err(error) = obex::write(link, 0x81, &[], &ctx.cancel)
        .and_then(|()| obex::response(link, &ctx.cancel, 0xa0).map(|_| ()))
    {
        ctx.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .warning = format!("File results confirmed; Bluetooth session close failed: {error}");
    }
    Ok(())
}
pub fn receive(link: &mut dyn Link, peer: &str, ctx: &JobContext) -> io::Result<()> {
    let (opcode, body) = obex::read(link, &ctx.cancel, obex::MTU)?;
    if opcode != 0x80 || body.len() < 4 || body[0] >> 4 != 1 {
        return Err(obex::invalid("Expected OBEX Object Push connection"));
    }
    let peer_mtu = usize::from(u16::from_be_bytes([body[2], body[3]]));
    if peer_mtu < 255 {
        return Err(obex::invalid("Invalid OBEX sender capacity"));
    }
    let keepalive = obex::headers(&body[4..])?.waiting;
    let mut greeting = vec![0x10, 0, (obex::MTU >> 8) as u8, obex::MTU as u8];
    if keepalive {
        greeting.extend(obex::WAIT_HEADER);
    }
    obex::write(link, 0xa0, &greeting, &ctx.cancel)?;
    let mut count = 0;
    loop {
        let (mut opcode, body) = obex::read(link, &ctx.cancel, obex::MTU)?;
        if opcode == 0x81 {
            obex::write(link, 0xa0, &[], &ctx.cancel)?;
            return Ok(());
        }
        if opcode == 0xff {
            obex::write(link, 0xa0, &[], &ctx.cancel)?;
            continue;
        }
        if !matches!(opcode, 0x02 | 0x82) {
            obex::write(link, 0xc0, &[], &ctx.cancel)?;
            return Err(obex::invalid("Only OBEX file pushes are supported"));
        }
        count += 1;
        if count > 256 {
            return Err(obex::invalid("Bluetooth session exceeds 256 files"));
        }
        let mut h = obex::headers(&body)?;
        let name = h
            .name
            .take()
            .ok_or_else(|| obex::invalid("OBEX file has no name"))?;
        let size = h
            .size
            .ok_or_else(|| obex::invalid("OBEX file must declare its size before receiving"))?;
        // One budget spans both approval and the possible replacement prompt.
        let mut waiting = DecisionWait::new(Instant::now(), keepalive);
        let destination = match ctx.approve(name.clone(), size, peer, || {
            waiting.pump(link, &ctx.cancel, Instant::now())
        })? {
            Some(p) => p,
            None => {
                obex::write(link, 0xc3, &[], &ctx.cancel)?;
                ctx.progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .skipped += 1;
                continue;
            }
        };
        let directory = fs::symlink_metadata(&destination)?;
        if !directory.is_dir() || directory.file_type().is_symlink() {
            return Err(io::Error::other(
                "Bluetooth destination must be a real local directory",
            ));
        }
        let target = destination.join(obex::filename(&name)?);
        let output = match crate::operations::generated::Output::new_named_pumping(
            target,
            &ctx.cancel,
            &ctx.conflicts,
            "Incoming Bluetooth file",
            size,
            || waiting.pump(link, &ctx.cancel, Instant::now()),
        ) {
            Ok(o) => o,
            Err(e)
                if e.kind() == io::ErrorKind::Interrupted
                    && !ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                obex::write(link, 0xc3, &[], &ctx.cancel)?;
                ctx.progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .skipped += 1;
                continue;
            }
            Err(e) => {
                let _ = obex::write(link, 0xd0, &[], &ctx.cancel);
                return Err(e);
            }
        };
        let mut stage = output.stage()?;
        let mut done = 0_u64;
        loop {
            if h.name.as_ref().is_some_and(|n| n != &name) || h.size.is_some_and(|n| n != size) {
                return Err(obex::invalid("OBEX file metadata changed during receiving"));
            }
            done = done
                .checked_add(h.body.len() as u64)
                .ok_or_else(|| obex::invalid("OBEX length overflow"))?;
            if done > size {
                return Err(obex::invalid("Bluetooth payload exceeds declared length"));
            }
            stage.file.write_all(&h.body)?;
            ctx.update("Receiving Bluetooth file", done, size);
            if opcode == 0x82 && !h.end {
                return Err(obex::invalid("Final OBEX packet has no end-of-body header"));
            }
            if h.end && opcode != 0x82 {
                return Err(obex::invalid("OBEX end of body must use a final PUT"));
            }
            if h.end {
                if done != size {
                    return Err(obex::invalid("Bluetooth file is incomplete"));
                }
                match output.publish(&stage, &ctx.cancel) {
                    Ok(warning) => {
                        ctx.completed();
                        if let Some(warning) = warning {
                            ctx.progress
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .warning = warning;
                        }
                    }
                    Err(e) => {
                        let _ = obex::write(link, 0xd0, &[], &ctx.cancel);
                        return Err(e);
                    }
                }
                obex::write(link, 0xa0, &[], &ctx.cancel)?;
                break;
            }
            obex::write(link, 0x90, &[], &ctx.cancel)?;
            let (next, body) = obex::read(link, &ctx.cancel, obex::MTU)?;
            if next == 0xff {
                obex::write(link, 0xa0, &[], &ctx.cancel)?;
                break;
            }
            if !matches!(next, 0x02 | 0x82) {
                return Err(obex::invalid("Unexpected OBEX operation inside a file"));
            }
            opcode = next;
            h = obex::headers(&body)?;
            if next == 0x82 && !h.end {
                return Err(obex::invalid("Final OBEX packet has no end-of-body header"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decision_budget_spans_prompts_and_pulses_never_grant_readiness() -> io::Result<()> {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let now = Instant::now();
        let mut waiting = DecisionWait::new(now, true);
        let mut wire = io::Cursor::new(Vec::new());
        waiting.pump(&mut wire, &cancel, now)?;
        waiting.pump(&mut wire, &cancel, now + Duration::from_secs(4))?;
        assert_eq!(wire.get_ref().len(), obex::WAIT_HEADER.len() + 3);
        waiting.pump(&mut wire, &cancel, now + Duration::from_secs(130))?;
        assert!(waiting
            .pump(&mut wire, &cancel, now + obex::DECISION_WAIT)
            .is_err());
        let mut legacy = DecisionWait::new(now, false);
        assert!(legacy
            .pump(&mut wire, &cancel, now + Duration::from_secs(110))
            .is_err());
        wire.get_mut().extend([0x90, 0, 3]);
        wire.set_position(0);
        assert_eq!(decision_response(&mut wire, &cancel, true)?, 0x90);
        let mut malformed = io::Cursor::new(vec![
            0x90, 0, 13, 0x70, 0, 10, b'D', b'N', b'w', b'a', b'i', b't', b'1',
        ]);
        assert!(decision_response(&mut malformed, &cancel, false).is_err());
        Ok(())
    }
}
