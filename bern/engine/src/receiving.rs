use crate::{
    files, network,
    storage::{self, History},
    worker::{Event, Worker, fraction, progress_detail},
};
use anyhow::{Context, Result, ensure};
use localsend::{
    http::{
        client::LsHttpClientV2,
        dto_v2::RegisterDtoV2,
        server::{
            common::save::FileUploadTarget,
            v2::{PrepareUploadDecisionV2, ServerEventV2, SessionEndReasonV2},
        },
    },
    model::{discovery::ProtocolType, transfer::FileDto},
};
use rustsend_protocol::transfer::{Phase, TransferItem};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub struct Pending {
    pub id: String,
    pub host: String,
    pub info: RegisterDtoV2,
    pub fingerprint: String,
    pub files: HashMap<String, FileDto>,
    pub selected: HashSet<String>,
    pub decision: oneshot::Sender<PrepareUploadDecisionV2>,
}

pub struct Receiving {
    pub host: String,
    pub info: RegisterDtoV2,
    pub fingerprint: String,
    pub files: HashMap<String, FileDto>,
    pub progress: HashMap<String, Arc<AtomicU64>>,
    pub in_flight: HashSet<String>,
    pub results: HashMap<String, Result<PathBuf>>,
    pub ended: Option<SessionEndReasonV2>,
    pub failed: bool,
    pub cancel: CancellationToken,
    pub directory: Arc<tempfile::TempDir>,
    pub destination: PathBuf,
    pub timestamp: String,
    pub started: Instant,
}

impl Receiving {
    pub fn view(&self, id: &str) -> TransferItem {
        let done: u64 = self
            .progress
            .values()
            .map(|p| p.load(Ordering::Relaxed))
            .sum();
        let total: u64 = self.files.values().map(|f| f.size).sum();
        TransferItem {
            id: id.into(),
            peer: self.info.alias.clone(),
            summary: self
                .files
                .values()
                .map(|f| f.file_name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            detail: progress_detail(done, total, self.started),
            timestamp: self.timestamp.clone(),
            sending: false,
            phase: if self.cancel.is_cancelled() {
                Phase::Cancelled
            } else {
                Phase::Active
            },
            progress: fraction(done, total),
            active: !self.cancel.is_cancelled(),
            can_open: false,
        }
    }
}

fn message(files: &HashMap<String, FileDto>) -> Option<String> {
    if files.len() != 1 {
        return None;
    }
    let file = files.values().next()?;
    if file.file_type != "text" && !file.file_type.starts_with("text/") {
        return None;
    }
    file.preview.as_ref().filter(|s| s.len() <= 64000).cloned()
}

impl Worker {
    pub async fn server_event(&mut self, event: ServerEventV2) -> Result<()> {
        match event {
            ServerEventV2::Register { ip, info } => {
                if let Some(net) = &self.network {
                    net.discovery
                        .add_device(network::registered(ip.to_string(), info))
                        .await;
                }
            }
            ServerEventV2::PrepareUpload {
                session_id,
                ip,
                info,
                cert_fingerprint,
                files,
                decision_tx,
            } => {
                if self.exporting
                    || self.pending.is_some()
                    || !self.receiving.is_empty()
                    || files.len() > 10000
                {
                    let _ = decision_tx.send(PrepareUploadDecisionV2::Decline);
                    return Ok(());
                }
                for file in files.values() {
                    if files::relative_name(&file.file_name).is_err() || file.size > i64::MAX as u64
                    {
                        let _ = decision_tx.send(PrepareUploadDecisionV2::Decline);
                        anyhow::bail!("Rejected an unsafe incoming file name or size");
                    }
                }
                ensure!(
                    files
                        .values()
                        .try_fold(0u64, |n, f| n.checked_add(f.size))
                        .is_some(),
                    "Incoming size overflow"
                );
                let verified = cert_fingerprint.is_some();
                let fingerprint = cert_fingerprint.unwrap_or_else(|| info.fingerprint.clone());
                let selected = files.keys().cloned().collect();
                let auto = self.config.quick_save
                    && (!self.config.favorites_only
                        || (verified && self.config.favorites.contains_key(&fingerprint)));
                self.pending = Some(Pending {
                    id: session_id.clone(),
                    host: ip.to_string(),
                    info,
                    fingerprint,
                    files,
                    selected,
                    decision: decision_tx,
                });
                if auto {
                    self.answer(&session_id, true)?;
                }
            }
            ServerEventV2::FileUpload {
                session_id,
                file_id,
                file,
                target_tx,
            } => {
                let Some(receive) = self.receiving.get_mut(&session_id) else {
                    return Ok(());
                };
                if receive.cancel.is_cancelled()
                    || !receive.files.contains_key(&file_id)
                    || !receive.in_flight.insert(file_id.clone())
                {
                    return Ok(());
                }
                let temporary = receive
                    .directory
                    .path()
                    .join(uuid::Uuid::new_v4().to_string());
                let progress = Arc::new(AtomicU64::new(0));
                receive.progress.insert(file_id.clone(), progress.clone());
                let (binary_tx, binary_rx) = mpsc::channel(8);
                let (result_tx, result_rx) = oneshot::channel::<Result<(), String>>();
                let events = self.events.clone();
                let directory = receive.directory.clone();
                let destination = receive.destination.clone();
                let cancel = receive.cancel.clone();
                tokio::spawn(async move {
                    let result = save_stream(
                        binary_rx,
                        &temporary,
                        &destination,
                        &file,
                        &progress,
                        &cancel,
                    )
                    .await;
                    // Only acknowledge HTTP success after the verified file is published.
                    let _ =
                        result_tx.send(result.as_ref().map(|_| ()).map_err(ToString::to_string));
                    drop(directory);
                    let _ = events
                        .send(Event::Received {
                            session: session_id,
                            file: file_id,
                            result,
                        })
                        .await;
                });
                let _ = target_tx.send(FileUploadTarget::Stream {
                    binary_tx,
                    result_rx,
                });
            }
            ServerEventV2::SessionEnd { session_id, reason } => {
                if let Some(receive) = self.receiving.get_mut(&session_id) {
                    receive.ended = Some(reason);
                    if reason == SessionEndReasonV2::Cancelled {
                        receive.cancel.cancel();
                    }
                }
                self.finish_receive(&session_id);
            }
            ServerEventV2::PrepareUploadAborted { session_id } => {
                if self.pending.as_ref().is_some_and(|p| p.id == session_id) {
                    self.pending = None;
                }
            }
            ServerEventV2::CancelReceived { ip, session_id } => {
                for send in self.sending.values() {
                    if send.session.as_deref() == Some(&session_id) && send.host == ip.to_string() {
                        send.cancel.cancel();
                    }
                }
            }
            ServerEventV2::ListenerFailed { error } => {
                self.state.ready = false;
                self.error(format!("Listener stopped: {error}"));
                self.network = None;
            }
        }
        Ok(())
    }

    pub fn publish_pending(&mut self) {
        self.state.pending = self.pending.is_some();
        if let Some(p) = &self.pending {
            self.state.pending_id = p.id.clone();
            self.state.pending_alias = p.info.alias.clone();
            self.state.pending_fingerprint = p.fingerprint.clone();
            let mut files: Vec<_> = p
                .files
                .values()
                .map(|f| files::view(f, p.selected.contains(&f.id)))
                .collect();
            files.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
            self.state.pending_count = p.selected.len() as i64;
            self.state.pending_size = files::format_bytes(
                p.files
                    .values()
                    .filter(|f| p.selected.contains(&f.id))
                    .map(|f| f.size)
                    .sum(),
            );
            self.state.can_accept = !p.selected.is_empty() || message(&p.files).is_some();
            self.state.pending_files = files;
            self.state.pending_text = message(&p.files).unwrap_or_default();
            self.state.has_pending_text = message(&p.files).is_some();
        } else {
            self.state.pending_count = 0;
            self.state.can_accept = false;
            self.state.pending_id.clear();
            self.state.pending_files.clear();
            self.state.pending_text.clear();
            self.state.has_pending_text = false;
        }
    }

    pub fn answer(&mut self, id: &str, accept: bool) -> Result<()> {
        ensure!(
            self.pending.as_ref().is_some_and(|p| p.id == id),
            "This request is no longer pending"
        );
        if !accept {
            let p = self.pending.take().unwrap();
            let _ = p.decision.send(PrepareUploadDecisionV2::Decline);
            return Ok(());
        }
        let p = self.pending.as_ref().unwrap();
        if let Some(text) = message(&p.files) {
            let p = self.pending.take().unwrap();
            p.decision
                .send(PrepareUploadDecisionV2::Accept(HashSet::new()))
                .map_err(|_| anyhow::anyhow!("Sender disconnected"))?;
            self.state.received_text = text;
            self.state.has_received_text = true;
            self.add_history(History {
                id: p.id,
                peer: p.info.alias,
                summary: p.files.values().next().unwrap().file_name.clone(),
                detail: files::format_bytes(self.state.received_text.len() as u64),
                timestamp: storage::timestamp(),
                sending: false,
                phase: "complete".into(),
                path: None,
            });
            return Ok(());
        }
        ensure!(
            !p.selected.is_empty(),
            "Select at least one file or decline the request"
        );
        // Desktop staging stays on the destination filesystem. Mobile stores
        // both folders in private app storage, keeping partial files out of exports.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let staging_root = &self.dir;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let staging_root = &self.config.destination;
        let directory = tempfile::Builder::new()
            .prefix(".localsend-")
            .tempdir_in(staging_root)
            .context("Cannot prepare receive directory")?;
        let p = self.pending.take().unwrap();
        p.decision
            .send(PrepareUploadDecisionV2::Accept(p.selected.clone()))
            .map_err(|_| anyhow::anyhow!("Sender disconnected"))?;
        self.receiving.insert(
            p.id,
            Receiving {
                host: p.host,
                info: p.info,
                fingerprint: p.fingerprint,
                files: p
                    .files
                    .into_iter()
                    .filter(|(id, _)| p.selected.contains(id))
                    .collect(),
                progress: HashMap::new(),
                in_flight: HashSet::new(),
                results: HashMap::new(),
                ended: None,
                failed: false,
                cancel: CancellationToken::new(),
                directory: Arc::new(directory),
                destination: self.config.destination.clone(),
                timestamp: storage::timestamp(),
                started: Instant::now(),
            },
        );
        Ok(())
    }

    pub async fn received(&mut self, session: &str, file: &str, result: Result<PathBuf>) {
        let mut failed = false;
        if let Some(receive) = self.receiving.get_mut(session) {
            failed = result.is_err() && !receive.cancel.is_cancelled();
            receive.failed |= failed;
            receive.in_flight.remove(file);
            receive.results.insert(file.into(), result);
        }
        if failed {
            // Core permits retries after an upload error. End this application's
            // session explicitly so a failed transfer never leaves the receiver busy.
            let _ = self.cancel(session).await;
        }
        self.finish_receive(session);
    }

    fn finish_receive(&mut self, session: &str) {
        if !self
            .receiving
            .get(session)
            .is_some_and(|r| r.ended.is_some() && r.in_flight.is_empty())
        {
            return;
        }
        let receive = self.receiving.remove(session).unwrap();
        let good = receive.results.values().filter(|r| r.is_ok()).count();
        let total = receive.files.len();
        let error = receive
            .results
            .values()
            .find_map(|r| r.as_ref().err())
            .map(ToString::to_string);
        let phase = if receive.failed {
            if good > 0 { "partial" } else { "failed" }
        } else if receive.cancel.is_cancelled() {
            "cancelled"
        } else if good == total {
            "complete"
        } else if good > 0 {
            "partial"
        } else {
            "failed"
        };
        if let Some(error) = &error {
            self.error(error);
        }
        let path = if good == 1 {
            receive
                .results
                .values()
                .find_map(|r| r.as_ref().ok())
                .cloned()
        } else if good > 0 {
            Some(receive.destination.clone())
        } else {
            None
        };
        self.add_history(History {
            id: session.into(),
            peer: receive.info.alias,
            summary: receive
                .files
                .values()
                .map(|f| f.file_name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            detail: error.unwrap_or_else(|| format!("{good} / {total}")),
            timestamp: receive.timestamp,
            sending: false,
            phase: phase.into(),
            path,
        });
    }

    pub async fn cancel(&mut self, id: &str) -> Result<()> {
        if let Some(send) = self.sending.get(id) {
            send.cancel.cancel();
            return Ok(());
        }
        let receive = self
            .receiving
            .get_mut(id)
            .context("Transfer is no longer active")?;
        receive.cancel.cancel();
        receive.ended = Some(SessionEndReasonV2::Cancelled);
        if let Some(net) = &self.network {
            net.server.cancel_v2_session(id).await;
            let identity = net.identity.clone();
            let host = receive.host.clone();
            let info = receive.info.clone();
            let fingerprint = receive.fingerprint.clone();
            let id = id.to_string();
            tokio::spawn(async move {
                if let Ok(client) = LsHttpClientV2::try_new(
                    &identity.key_pem,
                    &identity.cert_pem,
                    (info.protocol == ProtocolType::Https).then_some(fingerprint),
                    Some(Duration::from_secs(3)),
                ) {
                    let _ = client.cancel(info.protocol, &host, info.port, &id).await;
                }
            });
        }
        self.finish_receive(id);
        Ok(())
    }
}

/// Bounded streaming, cancellation and verification precede final publication.
async fn save_stream(
    mut chunks: mpsc::Receiver<bytes::Bytes>,
    temporary: &std::path::Path,
    destination: &std::path::Path,
    metadata: &FileDto,
    progress: &AtomicU64,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncWriteExt;
    let file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temporary)
        .await?;
    let mut output = tokio::io::BufWriter::with_capacity(512 * 1024, file);
    let mut hash = metadata.sha256.as_ref().map(|_| Sha256::new());
    let mut written = 0u64;
    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel.cancelled() => anyhow::bail!("Transfer cancelled"),
            chunk = tokio::time::timeout(Duration::from_secs(60), chunks.recv()) => chunk.context("Sender stopped transmitting for 60 seconds")?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        written = written
            .checked_add(chunk.len() as u64)
            .context("File size overflow")?;
        ensure!(
            written <= metadata.size,
            "Received more bytes than advertised"
        );
        if let Some(hash) = &mut hash {
            hash.update(&chunk);
        }
        output.write_all(&chunk).await?;
        progress.store(written, Ordering::Relaxed);
    }
    ensure!(
        written == metadata.size,
        "Incomplete file: expected {} bytes, received {written}",
        metadata.size
    );
    if let (Some(hash), Some(expected)) = (hash, &metadata.sha256) {
        let actual: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ensure!(actual.eq_ignore_ascii_case(expected), "Checksum mismatch");
    }
    output.flush().await?;
    output.get_ref().sync_all().await?;
    let file = output.into_inner().into_std().await;
    if let Some(meta) = &metadata.metadata {
        let mut times = std::fs::FileTimes::new();
        if let Some(time) = meta.modified.as_ref().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        }) {
            times = times.set_modified(time.into());
        }
        if let Some(time) = meta.accessed.as_ref().and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        }) {
            times = times.set_accessed(time.into());
        }
        let _ = file.set_times(times);
    }
    drop(file);
    ensure!(!cancel.is_cancelled(), "Transfer cancelled");
    files::persist_received(temporary, destination, &metadata.file_name)
}
