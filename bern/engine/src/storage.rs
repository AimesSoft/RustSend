use anyhow::{Context, Result, ensure};
use rustsend_protocol::transfer::{Appearance, Phase, TransferItem};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Favorite {
    pub alias: String,
    pub host: String,
    pub port: u16,
    pub https: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub alias: String,
    pub destination: PathBuf,
    pub port: u16,
    pub pin: String,
    /// Disable multicast and subnet discovery for isolated instances or manual-only networks.
    pub discovery: bool,
    pub quick_save: bool,
    pub favorites_only: bool,
    pub appearance: String,
    pub favorites: BTreeMap<String, Favorite>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            alias: crate::alias::generate(""),
            destination: default_destination(),
            port: 53317,
            pin: String::new(),
            discovery: true,
            quick_save: false,
            favorites_only: true,
            appearance: "system".into(),
            favorites: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.alias.trim().is_empty() && self.alias.chars().count() <= 80,
            "Device name must contain 1–80 characters"
        );
        ensure!(self.port > 0, "Port must be between 1 and 65535");
        ensure!(
            self.pin.is_empty()
                || (self.pin.len() <= 12 && self.pin.bytes().all(|b| b.is_ascii_digit())),
            "PIN must contain 1–12 digits, or be empty"
        );
        ensure!(
            self.destination.is_absolute(),
            "Receive directory must be an absolute path"
        );
        std::fs::create_dir_all(&self.destination).context("Cannot create receive directory")?;
        tempfile::NamedTempFile::new_in(&self.destination)
            .context("Receive directory is not writable")?;
        Ok(())
    }

    pub fn appearance(&self) -> Appearance {
        match self.appearance.as_str() {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            _ => Appearance::System,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct History {
    pub id: String,
    pub peer: String,
    pub summary: String,
    pub detail: String,
    pub timestamp: String,
    pub sending: bool,
    pub phase: String,
    pub path: Option<PathBuf>,
}

impl History {
    pub fn view(&self) -> TransferItem {
        TransferItem {
            id: self.id.clone(),
            peer: self.peer.clone(),
            summary: self.summary.clone(),
            detail: self.detail.clone(),
            timestamp: self.timestamp.clone(),
            sending: self.sending,
            phase: match self.phase.as_str() {
                "complete" => Phase::Complete,
                "cancelled" => Phase::Cancelled,
                "partial" => Phase::Partial,
                _ => Phase::Failed,
            },
            progress: if self.phase == "complete" { 1.0 } else { 0.0 },
            active: false,
            can_open: self.path.as_ref().is_some_and(|p| {
                p.is_file() || (!cfg!(any(target_os = "android", target_os = "ios")) && p.is_dir())
            }),
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn default_destination() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("RustSend")
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn default_destination() -> PathBuf {
    data_dir().join("Received")
}

pub fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("RUSTSEND_DATA_DIR")
        .or_else(|| std::env::var_os("LOCALSEND_BERN_DATA_DIR"))
    {
        return PathBuf::from(path);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        bern::storage::data_dir().expect("Mobile host must provide writable app storage")
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            // Retain the existing settings, certificate and transfer history.
            .join("localsend-bern")
    }
}

pub fn import_directory(data: &Path) -> Result<tempfile::TempDir> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let cache = bern::storage::cache_dir().unwrap_or_else(|| data.join("Cache"));
        std::fs::create_dir_all(&cache)?;
        Ok(tempfile::Builder::new()
            .prefix("imports-")
            .tempdir_in(cache)?)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = data;
        Ok(tempfile::tempdir()?)
    }
}

pub fn load<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).with_context(|| format!("Invalid {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("Cannot read {}", path.display())),
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("No settings directory")?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn timestamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute()
    )
}
