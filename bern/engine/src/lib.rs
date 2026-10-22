//! RustSend: native Bern application backed by upstream LocalSend Rust networking.
mod alias;
mod files;
mod identity;
mod network;
mod platform;
mod receiving;
mod sending;
mod storage;
mod worker;

use bern::{CommandError, media::MediaSource};
use rustsend_protocol::transfer::{self, Appearance, State};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use worker::Request;

pub struct RustSendEngine {
    state: State,
    shared: Arc<Mutex<State>>,
    requests: Option<mpsc::Sender<Request>>,
    stop: CancellationToken,
    directory: PathBuf,
}

impl Default for RustSendEngine {
    fn default() -> Self {
        Self::with_data_dir(storage::data_dir())
    }
}

impl RustSendEngine {
    /// Explicit storage root for portable deployments and isolated integration tests.
    pub fn with_data_dir(directory: PathBuf) -> Self {
        let state = worker::initial_state();
        Self {
            shared: Arc::new(Mutex::new(state.clone())),
            state,
            requests: None,
            stop: CancellationToken::new(),
            directory,
        }
    }

    fn start(&mut self) {
        if self.requests.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel(64);
        let shared = self.shared.clone();
        let stop = self.stop.clone();
        let directory = self.directory.clone();
        let waker = bern::wake::waker();
        self.requests = Some(tx);
        let failure = shared.clone();
        let failure_waker = waker.clone();
        let started = std::thread::Builder::new()
            .name("rustsend-network".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        runtime.block_on(worker::run(directory, rx, shared, waker, stop))
                    }
                    Err(e) => worker::publish_failure(&shared, &waker, e.to_string()),
                }
            });
        if let Err(e) = started {
            worker::publish_failure(&failure, &failure_waker, e.to_string());
        }
    }

    fn submit(&mut self, request: Request) -> Result<(), CommandError> {
        self.start();
        self.requests
            .as_ref()
            .unwrap()
            .try_send(request)
            .map_err(|e| CommandError::new("unavailable", e.to_string()))
    }
}

impl transfer::Engine for RustSendEngine {
    fn state(&self) -> &State {
        &self.state
    }
    fn wake(&mut self) {
        self.start();
        self.state = self
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
    }
}

impl Drop for RustSendEngine {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl transfer::Commands for RustSendEngine {
    fn discover(&mut self) -> Result<(), CommandError> {
        self.submit(Request::Discover)
    }
    fn add_address(&mut self, address: String) -> Result<(), CommandError> {
        self.submit(Request::Address(address))
    }
    fn add_file(&mut self, source: MediaSource) -> Result<(), CommandError> {
        self.submit(Request::File(source))
    }
    fn add_path(&mut self, path: String) -> Result<(), CommandError> {
        self.submit(Request::Path(path))
    }
    fn choose_files(&mut self) -> Result<(), CommandError> {
        self.submit(Request::PickFiles)
    }
    fn choose_folder(&mut self) -> Result<(), CommandError> {
        self.submit(Request::PickFolder)
    }
    fn remove_file(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Request::Remove(id))
    }
    fn clear_files(&mut self) -> Result<(), CommandError> {
        self.submit(Request::ClearFiles)
    }
    fn send_files(&mut self, device: String, pin: String) -> Result<(), CommandError> {
        self.submit(Request::Send {
            device,
            text: None,
            pin,
        })
    }
    fn send_text(&mut self, device: String, text: String, pin: String) -> Result<(), CommandError> {
        self.submit(Request::Send {
            device,
            text: Some(text),
            pin,
        })
    }
    fn toggle_favorite(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Request::Favorite(id))
    }
    fn select_incoming(&mut self, id: String, selected: bool) -> Result<(), CommandError> {
        self.submit(Request::SelectIncoming(id, selected))
    }
    fn answer(&mut self, session: String, accept: bool) -> Result<(), CommandError> {
        self.submit(Request::Answer(session, accept))
    }
    fn cancel(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Request::Cancel(id))
    }
    fn clear_history(&mut self) -> Result<(), CommandError> {
        self.submit(Request::ClearHistory)
    }
    fn open_received(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Request::OpenReceived(id))
    }
    fn open_destination(&mut self) -> Result<(), CommandError> {
        self.submit(Request::OpenDestination)
    }
    fn export_received(&mut self, id: String, directory: MediaSource) -> Result<(), CommandError> {
        self.submit(Request::ExportReceived(id, directory))
    }
    fn export_destination(&mut self, directory: MediaSource) -> Result<(), CommandError> {
        self.submit(Request::ExportDestination(directory))
    }
    fn dismiss_export_notice(&mut self) -> Result<(), CommandError> {
        self.submit(Request::DismissExportNotice)
    }
    fn choose_destination(&mut self) -> Result<(), CommandError> {
        self.submit(Request::PickDestination)
    }
    fn save_settings(
        &mut self,
        alias: String,
        destination: String,
        port: String,
        pin: String,
    ) -> Result<(), CommandError> {
        self.submit(Request::Settings {
            alias,
            destination,
            port,
            pin,
        })
    }
    fn set_quick_save(&mut self, value: bool) -> Result<(), CommandError> {
        self.submit(Request::QuickSave(value))
    }
    fn randomize_alias(&mut self) -> Result<(), CommandError> {
        self.submit(Request::RandomizeAlias)
    }
    fn set_favorites_only(&mut self, value: bool) -> Result<(), CommandError> {
        self.submit(Request::FavoritesOnly(value))
    }
    fn set_appearance(&mut self, value: Appearance) -> Result<(), CommandError> {
        self.submit(Request::Appearance(value))
    }
    fn dismiss_notice(&mut self) -> Result<(), CommandError> {
        self.submit(Request::DismissNotice)
    }
    fn dismiss_error(&mut self) -> Result<(), CommandError> {
        self.submit(Request::DismissError)
    }
    fn dismiss_text(&mut self) -> Result<(), CommandError> {
        self.submit(Request::DismissText)
    }
    fn copy_text(&mut self) -> Result<(), CommandError> {
        self.submit(Request::CopyText)
    }
    fn set_sharing(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.submit(Request::Share(enabled))
    }
    fn answer_web(&mut self, accept: bool) -> Result<(), CommandError> {
        self.submit(Request::AnswerWeb(accept))
    }
}
