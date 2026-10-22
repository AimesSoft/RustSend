use crate::{
    files::{self, SelectedFile},
    identity::Identity,
    network::{self, Network},
    platform,
    sending::{self, Payload},
    storage::{self, Config, Favorite, History},
};
use anyhow::{Context, Result, ensure};
use bern::media::MediaSource;
use localsend::{
    discovery::HttpChannel,
    http::server::{
        v2::ServerEventV2,
        web::{WebConfig, WebDownloadConfig, WebDownloadEvent, WebMode},
    },
    model::discovery::ProtocolType,
    util::interface::{InterfaceFilter, local_interface_addresses},
};
use rustsend_protocol::transfer::{Appearance, Device, Phase, State, TransferItem};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub enum Request {
    Discover,
    Address(String),
    File(MediaSource),
    Path(String),
    PickFiles,
    PickFolder,
    Remove(String),
    ClearFiles,
    Send {
        device: String,
        text: Option<String>,
        pin: String,
    },
    Favorite(String),
    SelectIncoming(String, bool),
    Answer(String, bool),
    Cancel(String),
    ClearHistory,
    OpenReceived(String),
    OpenDestination,
    ExportReceived(String, MediaSource),
    ExportDestination(MediaSource),
    DismissExportNotice,
    PickDestination,
    Settings {
        alias: String,
        destination: String,
        port: String,
        pin: String,
    },
    QuickSave(bool),
    RandomizeAlias,
    FavoritesOnly(bool),
    Appearance(Appearance),
    DismissNotice,
    DismissError,
    DismissText,
    CopyText,
    Share(bool),
    AnswerWeb(bool),
}

pub enum Event {
    Collected(Result<Vec<SelectedFile>>),
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Destination(Option<PathBuf>),
    Exported(Result<()>),
    SendStarted {
        id: String,
        session: String,
        total: u64,
    },
    SendEnded {
        id: String,
        result: Result<sending::Outcome>,
    },
    Received {
        session: String,
        file: String,
        result: Result<PathBuf>,
    },
    DiscoveryDone(Result<()>),
    Failure(String),
}

pub struct Sending {
    pub peer: String,
    pub host: String,
    pub session: Option<String>,
    pub summary: String,
    pub timestamp: String,
    pub total: u64,
    pub progress: Arc<AtomicU64>,
    pub cancel: CancellationToken,
    pub started: Instant,
}

pub struct Worker {
    pub state: State,
    pub config: Config,
    pub dir: PathBuf,
    pub network: Option<Network>,
    pub selected: Vec<SelectedFile>,
    pub exporting: bool,
    pub pending: Option<crate::receiving::Pending>,
    pub receiving: HashMap<String, crate::receiving::Receiving>,
    pub sending: HashMap<String, Sending>,
    pub history: Vec<History>,
    pub events: mpsc::Sender<Event>,
    pub server_events: mpsc::Sender<ServerEventV2>,
    web_events: mpsc::Sender<WebDownloadEvent>,
    web_pending: Option<oneshot::Sender<bool>>,
    shared: Arc<Mutex<State>>,
    waker: Option<bern::wake::Waker>,
    // Cached imports own their temporary directory for the worker's lifetime.
    imports: tempfile::TempDir,
}

pub fn initial_state() -> State {
    State {
        alias: String::new(),
        fingerprint: String::new(),
        addresses: String::new(),
        destination: String::new(),
        port: "53317".into(),
        pin: String::new(),
        mobile: cfg!(any(target_os = "android", target_os = "ios")),
        ready: false,
        loading: false,
        discovering: false,
        busy: false,
        quick_save: false,
        favorites_only: true,
        appearance: Appearance::System,
        notice: None,
        export_complete: false,
        devices: vec![],
        has_devices: false,
        files: vec![],
        has_files: false,
        selected_size: "0 B".into(),
        transfers: vec![],
        has_transfers: false,
        error: String::new(),
        has_error: false,
        pending: false,
        pending_id: String::new(),
        pending_alias: String::new(),
        pending_fingerprint: String::new(),
        pending_files: vec![],
        pending_size: String::new(),
        pending_count: 0,
        can_accept: false,
        pending_text: String::new(),
        has_pending_text: false,
        received_text: String::new(),
        has_received_text: false,
        sharing: false,
        share_urls: String::new(),
        web_pending: false,
        web_peer: String::new(),
    }
}

pub async fn run(
    dir: PathBuf,
    mut requests: mpsc::Receiver<Request>,
    shared: Arc<Mutex<State>>,
    waker: Option<bern::wake::Waker>,
    stop: CancellationToken,
) {
    let (events, mut event_rx) = mpsc::channel(128);
    let (server_events, mut server_rx) = mpsc::channel(32);
    let (web_events, mut web_rx) = mpsc::channel(16);
    let initialized = (|| -> Result<_> {
        std::fs::create_dir_all(&dir)?;
        let config: Config = storage::load(&dir.join("settings.json"))?;
        config.validate()?;
        // Persist generated defaults before starting discovery, including old settings
        // without an alias. A restart must never silently rename a device.
        storage::save(&dir.join("settings.json"), &config)?;
        let history: Vec<History> = storage::load(&dir.join("history.json"))?;
        let imports = storage::import_directory(&dir)?;
        Ok((config, history, imports))
    })();
    let (config, history, imports) = match initialized {
        Ok(value) => value,
        Err(e) => {
            publish_failure(&shared, &waker, format!("{e:#}"));
            return;
        }
    };
    let mut worker = Worker {
        state: initial_state(),
        config,
        dir,
        network: None,
        selected: vec![],
        exporting: false,
        pending: None,
        receiving: HashMap::new(),
        sending: HashMap::new(),
        history,
        events,
        server_events,
        web_events,
        web_pending: None,
        shared,
        waker,
        imports,
    };
    if let Err(e) = worker.start_network().await {
        worker.error(e);
    }
    worker.discover();
    let mut interval = tokio::time::interval(Duration::from_millis(150));
    loop {
        worker.publish();
        tokio::select! {
            _ = stop.cancelled() => break,
            request = requests.recv() => match request {
                Some(request) => if let Err(e) = worker.request(request).await { worker.error(e); },
                None => break,
            },
            Some(event) = event_rx.recv() => if let Err(e) = worker.event(event).await { worker.error(e); },
            Some(event) = server_rx.recv() => if let Err(e) = worker.server_event(event).await { worker.error(e); },
            Some(event) = web_rx.recv() => worker.web_event(event),
            _ = interval.tick() => {},
        }
    }
    for send in worker.sending.values() {
        send.cancel.cancel();
    }
    for receive in worker.receiving.values() {
        receive.cancel.cancel();
    }
    if let Some(net) = &mut worker.network {
        net.stop().await;
    }
}

pub fn publish_failure(
    shared: &Arc<Mutex<State>>,
    waker: &Option<bern::wake::Waker>,
    error: String,
) {
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    state.error = error;
    state.has_error = true;
    state.ready = false;
    drop(state);
    if let Some(waker) = waker {
        waker.wake();
    }
}

impl Worker {
    pub fn error(&mut self, error: impl std::fmt::Display) {
        self.state.error = error.to_string();
        self.state.has_error = true;
    }

    pub fn idle(&self) -> Result<()> {
        ensure!(!self.exporting, "Wait for the file export to finish");
        ensure!(
            self.pending.is_none() && self.receiving.is_empty() && self.sending.is_empty(),
            "Finish or cancel the current transfer first"
        );
        Ok(())
    }

    fn export(&mut self, path: PathBuf, directory: MediaSource) -> Result<()> {
        self.idle()?;
        ensure!(!self.state.loading, "Wait for file selection to finish");
        ensure!(path.exists(), "The received file is no longer available");
        ensure!(
            (cfg!(target_os = "android") && directory.url.starts_with("content://"))
                || (cfg!(target_os = "ios") && directory.url.starts_with("file://")),
            "Choose a destination with the system folder picker"
        );
        self.exporting = true;
        self.state.export_complete = false;
        self.state.notice = None;
        self.state.has_error = false;
        self.state.error.clear();
        let tx = self.events.clone();
        tokio::task::spawn_blocking(move || {
            let result = platform::export_path(&path, &directory.url);
            let _ = tx.blocking_send(Event::Exported(result));
        });
        Ok(())
    }

    pub fn save_config(&self) -> Result<()> {
        storage::save(&self.dir.join("settings.json"), &self.config)
    }

    async fn apply_settings(&mut self, new: Config) -> Result<()> {
        self.idle()?;
        ensure!(
            !self.state.sharing,
            "Stop link sharing before changing network settings"
        );
        new.validate()?;
        let old = std::mem::replace(&mut self.config, new);
        let result = match self.restart().await {
            Ok(()) => self.save_config(),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.config = old;
            let _ = self.restart().await;
            return Err(error);
        }
        Ok(())
    }

    pub fn add_history(&mut self, entry: History) {
        self.state.export_complete = false;
        if entry.phase != "cancelled" {
            self.state.notice = Some(entry.view());
        }
        self.history.insert(0, entry);
        self.history.truncate(200);
        if let Err(e) = storage::save(&self.dir.join("history.json"), &self.history) {
            self.error(e);
        }
    }

    async fn start_network(&mut self) -> Result<()> {
        let identity =
            Identity::load_or_generate(&self.dir, self.config.alias.clone(), self.config.port)?;
        let web = if self.state.sharing {
            WebConfig {
                mode: WebMode::Download(WebDownloadConfig {
                    files: self
                        .selected
                        .iter()
                        .map(|f| (f.dto.id.clone(), f.dto.clone()))
                        .collect(),
                    pin: (!self.config.pin.is_empty()).then(|| self.config.pin.clone()),
                    event_tx: self.web_events.clone(),
                }),
                ..Default::default()
            }
        } else {
            WebConfig::default()
        };
        self.network =
            Some(Network::start(identity, &self.config, self.server_events.clone(), web).await?);
        self.state.ready = true;
        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        if let Some(mut net) = self.network.take() {
            net.stop().await;
        }
        self.state.ready = false;
        self.start_network().await?;
        if self.state.discovering && self.config.discovery {
            // An old subnet scan may still be finishing. Announce the new
            // identity now even while that scan keeps the refresh button busy.
            let discovery = self.network.as_ref().unwrap().discovery.clone();
            tokio::spawn(async move { discovery.announce().await });
        } else {
            self.discover();
        }
        Ok(())
    }

    fn discover(&mut self) {
        if self.state.discovering || !self.config.discovery {
            return;
        }
        let Some(net) = &self.network else {
            return;
        };
        self.state.discovering = true;
        let discovery = net.discovery.clone();
        let known = self
            .config
            .favorites
            .values()
            .map(|f| HttpChannel {
                host: f.host.clone(),
                port: f.port,
                protocol: if f.https {
                    ProtocolType::Https
                } else {
                    ProtocolType::Http
                },
            })
            .collect();
        let port = self.config.port;
        let events = self.events.clone();
        tokio::spawn(async move {
            let ips = local_interface_addresses(&InterfaceFilter::default()).unwrap_or_default();
            let result = discovery
                .discover_staged(
                    known,
                    ips,
                    port,
                    ProtocolType::Https,
                    Duration::from_secs(1),
                )
                .await
                .map_err(Into::into);
            let _ = events.send(Event::DiscoveryDone(result)).await;
        });
    }

    fn collect(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        ensure!(
            !self.state.sharing,
            "Stop link sharing before changing the selection"
        );
        ensure!(!self.state.loading, "File selection is still loading");
        self.state.loading = true;
        let tx = self.events.clone();
        tokio::task::spawn_blocking(move || {
            let _ = tx.blocking_send(Event::Collected(files::collect(paths)));
        });
        Ok(())
    }

    async fn request(&mut self, request: Request) -> Result<()> {
        match request {
            Request::Discover => {
                if self.network.is_none() {
                    self.start_network().await?;
                }
                self.discover();
            }
            Request::Address(address) => {
                let target = network::parse_address(&address)?;
                let discovery = self
                    .network
                    .as_ref()
                    .context("Server is not running")?
                    .discovery
                    .clone();
                let tx = self.events.clone();
                tokio::spawn(async move {
                    if let Err(e) = discovery
                        .discover(&target.host, target.port, target.protocol)
                        .await
                    {
                        let _ = tx.send(Event::Failure(e.to_string())).await;
                    }
                });
            }
            Request::Path(path) => self.collect(vec![PathBuf::from(path)])?,
            Request::File(source) => {
                if let Ok(url) = url::Url::parse(&source.url)
                    && let Ok(path) = url.to_file_path()
                {
                    self.collect(vec![path])?;
                } else {
                    ensure!(
                        !self.state.loading && !self.state.sharing,
                        "Selection cannot be changed now"
                    );
                    self.state.loading = true;
                    let dir = self.imports.path().join(Uuid::new_v4().to_string());
                    let tx = self.events.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = (|| -> Result<_> {
                            std::fs::create_dir(&dir)?;
                            #[cfg(target_os = "android")]
                            if source.url.starts_with("content://")
                                && source.url.contains("/tree/")
                                && !source.url.contains("/document/")
                            {
                                let folder = bern::platform::import_folder(&source.url, &dir)
                                    .map_err(|error| anyhow::Error::msg(error.message))?;
                                return files::collect(vec![folder]);
                            }
                            let name = localsend::util::filename::sanitize(
                                &source.file_name().unwrap_or_else(|| "file".into()),
                                localsend::util::filename::Rules::Universal,
                            );
                            let path = dir.join(name);
                            let mut input = source
                                .open()
                                .map_err(|error| anyhow::Error::msg(error.message))?;
                            let mut output = std::fs::File::create(&path)?;
                            std::io::copy(&mut input, &mut output)?;
                            files::collect(vec![path])
                        })();
                        let _ = tx.blocking_send(Event::Collected(result));
                    });
                }
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            Request::PickFiles | Request::PickFolder => {
                anyhow::bail!("Use the Files or Folder button to open the system document picker");
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Request::PickFiles | Request::PickFolder => {
                ensure!(
                    !self.state.loading && !self.state.sharing,
                    "Selection cannot be changed now"
                );
                self.state.loading = true;
                let folder = matches!(request, Request::PickFolder);
                let tx = self.events.clone();
                tokio::spawn(async move {
                    let paths: Vec<PathBuf> = if folder {
                        rfd::AsyncFileDialog::new()
                            .pick_folder()
                            .await
                            .into_iter()
                            .map(|f| f.path().to_owned())
                            .collect()
                    } else {
                        rfd::AsyncFileDialog::new()
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|f| f.path().to_owned())
                            .collect()
                    };
                    let result = if paths.is_empty() {
                        Ok(vec![])
                    } else {
                        tokio::task::spawn_blocking(move || files::collect(paths))
                            .await
                            .unwrap_or_else(|e| Err(e.into()))
                    };
                    let _ = tx.send(Event::Collected(result)).await;
                });
            }
            Request::Remove(id) => {
                ensure!(!self.state.sharing, "Stop link sharing first");
                self.selected.retain(|f| f.dto.id != id);
            }
            Request::ClearFiles => {
                ensure!(!self.state.sharing, "Stop link sharing first");
                self.selected.clear();
            }
            Request::Send { device, text, pin } => {
                ensure!(self.sending.len() < 8, "At most 8 concurrent recipients");
                let net = self.network.as_ref().context("Server is not running")?;
                let device = net
                    .discovery
                    .device_by_fingerprint(&device)
                    .context("Device is offline; discover it first")?;
                let payload = match text {
                    Some(text) => Payload::text(text)?,
                    None => {
                        ensure!(!self.selected.is_empty(), "Select files first");
                        Payload::files(&self.selected)
                    }
                };
                let id = Uuid::new_v4().to_string();
                let progress = Arc::new(AtomicU64::new(0));
                let cancel = CancellationToken::new();
                let host = device
                    .get_best_channel()
                    .and_then(|c| c.http())
                    .context("No device address")?
                    .host
                    .clone();
                let summary = payload
                    .files
                    .values()
                    .map(|f| f.file_name.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.sending.insert(
                    id.clone(),
                    Sending {
                        peer: device.device.alias.clone(),
                        host,
                        session: None,
                        summary,
                        timestamp: storage::timestamp(),
                        total: payload.size(),
                        progress: progress.clone(),
                        cancel: cancel.clone(),
                        started: Instant::now(),
                    },
                );
                let job = sending::SendJob {
                    id: id.clone(),
                    identity: net.identity.clone(),
                    device,
                    payload,
                    pin,
                    progress,
                    cancel,
                    events: self.events.clone(),
                };
                let events = self.events.clone();
                tokio::spawn(async move {
                    let result = sending::run(job).await;
                    let _ = events.send(Event::SendEnded { id, result }).await;
                });
            }
            Request::Favorite(id) => {
                if self.config.favorites.remove(&id).is_none() {
                    let device = self
                        .network
                        .as_ref()
                        .and_then(|n| n.discovery.device_by_fingerprint(&id))
                        .context("Device not found")?;
                    let http = device
                        .get_best_channel()
                        .and_then(|c| c.http())
                        .context("No device address")?;
                    self.config.favorites.insert(
                        id,
                        Favorite {
                            alias: device.device.alias.clone(),
                            host: http.host.clone(),
                            port: http.port,
                            https: http.protocol == ProtocolType::Https,
                        },
                    );
                }
                self.save_config()?;
            }
            Request::SelectIncoming(id, selected) => {
                let pending = self.pending.as_mut().context("No incoming request")?;
                ensure!(pending.files.contains_key(&id), "Unknown file");
                if selected {
                    pending.selected.insert(id);
                } else {
                    pending.selected.remove(&id);
                }
            }
            Request::Answer(session, accept) => self.answer(&session, accept)?,
            Request::Cancel(id) => self.cancel(&id).await?,
            Request::ClearHistory => {
                self.state.notice = None;
                self.history.clear();
                storage::save(&self.dir.join("history.json"), &self.history)?;
            }
            Request::OpenReceived(id) => {
                let path = self
                    .history
                    .iter()
                    .find(|h| h.id == id)
                    .and_then(|h| h.path.as_ref())
                    .context("No received file for this transfer")?;
                platform::open_path(path)?;
            }
            Request::OpenDestination => {
                platform::open_path(&self.config.destination)?;
            }
            Request::ExportReceived(id, directory) => {
                let path = self
                    .history
                    .iter()
                    .find(|h| h.id == id)
                    .and_then(|h| h.path.as_ref())
                    .context("No received file for this transfer")?
                    .clone();
                ensure!(
                    path.is_file(),
                    "Export all received files for a multi-file transfer"
                );
                self.export(path, directory)?;
            }
            Request::ExportDestination(directory) => {
                self.export(self.config.destination.clone(), directory)?;
            }
            Request::DismissExportNotice => self.state.export_complete = false,
            #[cfg(any(target_os = "android", target_os = "ios"))]
            Request::PickDestination => {
                anyhow::bail!(
                    "Mobile devices keep received files in app storage. Use Export to save them in a folder of your choice"
                );
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Request::PickDestination => {
                self.idle()?;
                let tx = self.events.clone();
                tokio::spawn(async move {
                    let path = rfd::AsyncFileDialog::new()
                        .pick_folder()
                        .await
                        .map(|f| f.path().to_owned());
                    let _ = tx.send(Event::Destination(path)).await;
                });
            }
            Request::Settings {
                alias,
                destination,
                port,
                pin,
            } => {
                let mut new = self.config.clone();
                new.alias = alias.trim().into();
                new.destination = destination.into();
                new.port = port.parse().context("Port must be between 1 and 65535")?;
                new.pin = pin;
                self.apply_settings(new).await?;
            }
            Request::RandomizeAlias => {
                let mut new = self.config.clone();
                new.alias = crate::alias::generate(&self.config.alias);
                self.apply_settings(new).await?;
            }
            Request::QuickSave(value) => {
                self.config.quick_save = value;
                self.save_config()?;
            }
            Request::FavoritesOnly(value) => {
                self.config.favorites_only = value;
                self.save_config()?;
            }
            Request::Appearance(value) => {
                self.config.appearance = match value {
                    Appearance::System => "system",
                    Appearance::Light => "light",
                    Appearance::Dark => "dark",
                }
                .into();
                self.save_config()?;
            }
            Request::DismissNotice => self.state.notice = None,
            Request::DismissError => {
                self.state.error.clear();
                self.state.has_error = false;
            }
            Request::DismissText => {
                self.state.received_text.clear();
                self.state.has_received_text = false;
            }
            Request::CopyText => {
                platform::copy_text(&self.state.received_text)?;
            }
            Request::Share(value) => {
                self.idle()?;
                ensure!(
                    !value || !self.selected.is_empty(),
                    "Select files to share first"
                );
                if self.state.sharing != value {
                    self.state.sharing = value;
                    self.web_pending = None;
                    self.state.web_pending = false;
                    if let Err(e) = self.restart().await {
                        self.state.sharing = false;
                        let _ = self.restart().await;
                        return Err(e);
                    }
                }
            }
            Request::AnswerWeb(accept) => {
                if let Some(tx) = self.web_pending.take() {
                    let _ = tx.send(accept);
                }
                self.state.web_pending = false;
            }
        }
        Ok(())
    }

    async fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Collected(result) => {
                self.state.loading = false;
                let files = result?;
                let existing: HashSet<_> = self.selected.iter().map(|f| f.path.clone()).collect();
                self.selected
                    .extend(files.into_iter().filter(|f| !existing.contains(&f.path)));
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Event::Destination(path) => {
                if let Some(path) = path {
                    self.idle()?;
                    let mut new = self.config.clone();
                    new.destination = path;
                    new.validate()?;
                    self.config = new;
                    self.save_config()?;
                }
            }
            Event::Exported(result) => {
                self.exporting = false;
                result?;
                self.state.export_complete = true;
            }
            Event::DiscoveryDone(result) => {
                self.state.discovering = false;
                result?;
            }
            Event::Failure(message) => self.error(message),
            Event::SendStarted { id, session, total } => {
                if let Some(send) = self.sending.get_mut(&id) {
                    send.session = Some(session);
                    send.total = total;
                }
            }
            Event::SendEnded { id, result } => {
                if let Some(send) = self.sending.remove(&id) {
                    let (phase, detail) = match result {
                        Ok(outcome) => (
                            if outcome.partial {
                                "partial"
                            } else {
                                "complete"
                            },
                            format!("{} · {}", outcome.files, files::format_bytes(outcome.bytes)),
                        ),
                        Err(e) if send.cancel.is_cancelled() => ("cancelled", e.to_string()),
                        Err(e) => {
                            self.error(&e);
                            ("failed", e.to_string())
                        }
                    };
                    self.add_history(History {
                        id,
                        peer: send.peer,
                        summary: send.summary,
                        detail,
                        timestamp: send.timestamp,
                        sending: true,
                        phase: phase.into(),
                        path: None,
                    });
                }
            }
            Event::Received {
                session,
                file,
                result,
            } => self.received(&session, &file, result).await,
        }
        Ok(())
    }

    fn web_event(&mut self, event: WebDownloadEvent) {
        match event {
            WebDownloadEvent::PrepareDownload {
                ip, decision_tx, ..
            } => {
                if !self.state.sharing || self.web_pending.is_some() {
                    let _ = decision_tx.send(false);
                    return;
                }
                self.state.web_peer = ip.to_string();
                self.state.web_pending = true;
                self.web_pending = Some(decision_tx);
            }
            WebDownloadEvent::FileDownload {
                file_id,
                content_tx,
                ..
            } => {
                if self.state.sharing
                    && let Some(file) = self.selected.iter().find(|f| f.dto.id == file_id)
                {
                    let _ = content_tx.send(localsend::model::transfer::FileContent::Path(
                        file.path.clone(),
                    ));
                }
            }
        }
    }

    fn publish(&mut self) {
        self.state.alias = self.config.alias.clone();
        self.state.destination = self.config.destination.display().to_string();
        self.state.port = self.config.port.to_string();
        self.state.pin = self.config.pin.clone();
        self.state.quick_save = self.config.quick_save;
        self.state.favorites_only = self.config.favorites_only;
        self.state.appearance = self.config.appearance();
        self.state.busy = self.idle().is_err();
        self.state.files = self.selected.iter().map(SelectedFile::view).collect();
        self.state.has_files = !self.selected.is_empty();
        self.state.selected_size =
            files::format_bytes(self.selected.iter().map(|f| f.dto.size).sum());
        if let Some(net) = &self.network {
            self.state.fingerprint = net.identity.fingerprint.clone();
            self.state.addresses = net.urls();
            self.state.share_urls = if self.state.sharing {
                net.urls()
            } else {
                String::new()
            };
            let mut devices: Vec<_> = net
                .discovery
                .devices()
                .into_iter()
                .map(|device| {
                    let http = device.get_best_channel().and_then(|c| c.http());
                    Device {
                        id: device.device.fingerprint.clone(),
                        alias: device.device.alias.clone(),
                        model: device.device.device_model.clone().unwrap_or_default(),
                        address: http
                            .map(|h| format!("{}:{}", h.host, h.port))
                            .unwrap_or_default(),
                        favorite: self
                            .config
                            .favorites
                            .contains_key(&device.device.fingerprint),
                        online: true,
                        encrypted: http.is_some_and(|h| h.protocol == ProtocolType::Https),
                    }
                })
                .collect();
            for (id, favorite) in &self.config.favorites {
                if !devices.iter().any(|d| d.id == *id) {
                    devices.push(Device {
                        id: id.clone(),
                        alias: favorite.alias.clone(),
                        address: format!("{}:{}", favorite.host, favorite.port),
                        model: String::new(),
                        favorite: true,
                        online: false,
                        encrypted: favorite.https,
                    });
                }
            }
            devices.sort_by(|a, b| {
                b.favorite
                    .cmp(&a.favorite)
                    .then(a.alias.cmp(&b.alias))
                    .then(a.id.cmp(&b.id))
            });
            self.state.has_devices = !devices.is_empty();
            self.state.devices = devices;
        }
        self.publish_pending();
        let mut active: Vec<_> = self
            .sending
            .iter()
            .map(|(id, s)| {
                let done = s.progress.load(Ordering::Relaxed);
                TransferItem {
                    id: id.clone(),
                    peer: s.peer.clone(),
                    summary: s.summary.clone(),
                    detail: progress_detail(done, s.total, s.started),
                    timestamp: s.timestamp.clone(),
                    sending: true,
                    phase: if s.session.is_some() {
                        Phase::Active
                    } else {
                        Phase::Waiting
                    },
                    progress: fraction(done, s.total),
                    active: true,
                    can_open: false,
                }
            })
            .collect();
        active.extend(self.receiving.iter().map(|(id, r)| r.view(id)));
        active.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(a.id.cmp(&b.id)));
        active.extend(self.history.iter().map(History::view));
        self.state.has_transfers = !active.is_empty();
        self.state.transfers = active;
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if *shared != self.state {
            *shared = self.state.clone();
            drop(shared);
            if let Some(waker) = &self.waker {
                waker.wake();
            }
        }
    }
}

pub fn fraction(done: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (done as f64 / total as f64).clamp(0.0, 1.0)
    }
}
pub fn progress_detail(done: u64, total: u64, started: Instant) -> String {
    let speed = done as f64 / started.elapsed().as_secs_f64().max(0.1);
    format!(
        "{} / {} · {}/s",
        files::format_bytes(done),
        files::format_bytes(total),
        files::format_bytes(speed as u64)
    )
}
