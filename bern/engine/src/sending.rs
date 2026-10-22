use crate::{files::SelectedFile, identity::Identity};
use anyhow::{Result, ensure};
use bytes::Bytes;
use futures_util::StreamExt;
use localsend::{
    discovery::StatefulDevice,
    http::{client::LsHttpClientV2, dto_v2::PrepareUploadRequestDtoV2},
    model::{
        discovery::ProtocolType,
        transfer::{FileContent, FileDto},
    },
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub enum Source {
    Path(std::path::PathBuf),
    Text(Bytes),
}

impl Source {
    pub fn content(self) -> FileContent {
        match self {
            Self::Path(path) => FileContent::Path(path),
            Self::Text(bytes) => {
                let (tx, rx) = mpsc::channel(1);
                let _ = tx.try_send(bytes);
                FileContent::Stream(rx)
            }
        }
    }
}

pub struct Payload {
    pub files: HashMap<String, FileDto>,
    pub sources: HashMap<String, Source>,
}

impl Payload {
    pub fn files(files: &[SelectedFile]) -> Self {
        Self {
            files: files
                .iter()
                .map(|f| (f.dto.id.clone(), f.dto.clone()))
                .collect(),
            sources: files
                .iter()
                .map(|f| (f.dto.id.clone(), Source::Path(f.path.clone())))
                .collect(),
        }
    }
    pub fn text(text: String) -> Result<Self> {
        ensure!(!text.is_empty(), "Enter a message first");
        ensure!(
            text.len() <= 16_000_000,
            "Message exceeds 16 MB; send it as a file"
        );
        let id = Uuid::new_v4().to_string();
        let file = FileDto {
            id: id.clone(),
            file_name: format!("message-{id}.txt"),
            size: text.len() as u64,
            file_type: "text/plain".into(),
            preview: (text.len() <= 64000).then(|| text.clone()),
            sha256: None,
            metadata: None,
        };
        Ok(Self {
            files: HashMap::from([(id.clone(), file)]),
            sources: HashMap::from([(id, Source::Text(Bytes::from(text)))]),
        })
    }
    pub fn size(&self) -> u64 {
        self.files.values().map(|f| f.size).sum()
    }
}

pub struct SendJob {
    pub id: String,
    pub identity: Arc<Identity>,
    pub device: StatefulDevice,
    pub payload: Payload,
    pub pin: String,
    pub progress: Arc<AtomicU64>,
    pub cancel: CancellationToken,
    pub events: mpsc::Sender<crate::worker::Event>,
}

pub struct Outcome {
    pub partial: bool,
    pub files: usize,
    pub bytes: u64,
}

pub async fn run(job: SendJob) -> Result<Outcome> {
    let SendJob {
        id,
        identity,
        device,
        mut payload,
        pin,
        progress,
        cancel,
        events,
    } = job;
    let http = device
        .get_best_channel()
        .and_then(|c| c.http())
        .ok_or_else(|| anyhow::anyhow!("Device has no address"))?;
    let client = LsHttpClientV2::try_new(
        &identity.key_pem,
        &identity.cert_pem,
        (http.protocol == ProtocolType::Https).then(|| device.device.fingerprint.clone()),
        None,
    )?;
    let offered = payload.files.len();
    let message = offered == 1 && payload.files.values().all(|f| f.preview.is_some());
    let prepared = client
        .prepare_upload(
            http.protocol,
            &http.host,
            http.port,
            None,
            PrepareUploadRequestDtoV2 {
                info: identity.register_dto(),
                files: payload.files.clone(),
            },
            (!pin.is_empty()).then_some(pin.as_str()),
            cancel.clone(),
        )
        .await?;
    let Some(response) = prepared.response else {
        ensure!(
            message && prepared.status_code == 204,
            "The recipient did not accept any files"
        );
        progress.store(payload.size(), Ordering::Relaxed);
        return Ok(Outcome {
            partial: false,
            files: 1,
            bytes: payload.size(),
        });
    };
    ensure!(
        prepared.status_code == 200
            && !response.session_id.is_empty()
            && !response.files.is_empty()
            && response
                .files
                .iter()
                .all(|(id, token)| payload.files.contains_key(id) && !token.is_empty()),
        "Invalid response from recipient"
    );
    let total = response.files.keys().map(|id| payload.files[id].size).sum();
    let _ = events
        .send(crate::worker::Event::SendStarted {
            id: id.clone(),
            session: response.session_id.clone(),
            total,
        })
        .await;
    let mut ids: Vec<_> = response.files.keys().cloned().collect();
    ids.sort_by_key(|id| payload.files[id].file_name.clone());
    let mut sent = 0;
    for file_id in &ids {
        let source = payload
            .sources
            .remove(file_id)
            .ok_or_else(|| anyhow::anyhow!("Missing local file"))?;
        let progress = progress.clone();
        let base = sent;
        let mut current = 0;
        let body = localsend::reqwest::Body::wrap_stream(source.content().into_stream().map(
            move |chunk| {
                let chunk = chunk?;
                current += chunk.len() as u64;
                progress.store(base + current, Ordering::Relaxed);
                Ok::<_, std::io::Error>(chunk)
            },
        ));
        let result = client
            .upload(
                http.protocol,
                &http.host,
                http.port,
                None,
                &response.session_id,
                file_id,
                &response.files[file_id],
                body,
                cancel.clone(),
            )
            .await;
        if let Err(error) = result {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                client.cancel(http.protocol, &http.host, http.port, &response.session_id),
            )
            .await;
            return Err(error.into());
        }
        sent += payload.files[file_id].size;
    }
    progress.store(sent, Ordering::Relaxed);
    Ok(Outcome {
        partial: ids.len() != offered,
        files: ids.len(),
        bytes: sent,
    })
}
