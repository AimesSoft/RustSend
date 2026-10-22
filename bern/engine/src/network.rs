use crate::{identity::Identity, storage::Config};
use anyhow::Result;
use localsend::{
    discovery::{
        self, DeviceChannel, DeviceIdentity, DiscoveredDevice, DiscoveryConfig, DiscoveryHandle,
        HttpChannel,
    },
    http::{
        dto_v2::RegisterDtoV2,
        server::{
            ServerConfigV2, ServerHandle, start_with_port, v2::ServerEventV2, web::WebConfig,
        },
    },
    model::discovery::ProtocolType,
    multicast::{DEFAULT_MULTICAST_GROUP, DEFAULT_MULTICAST_GROUP_V6, DEFAULT_PORT},
    util::interface::InterfaceFilter,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

pub struct Network {
    pub identity: Arc<Identity>,
    pub server: Arc<ServerHandle>,
    pub discovery: Arc<DiscoveryHandle>,
    server_stop: Option<oneshot::Sender<()>>,
    discovery_stop: Option<oneshot::Sender<()>>,
}

impl Network {
    pub async fn start(
        identity: Identity,
        config: &Config,
        events: mpsc::Sender<ServerEventV2>,
        web: WebConfig,
    ) -> Result<Self> {
        let (tx, rx) = oneshot::channel();
        let server = start_with_port(
            config.port,
            Some(identity.tls_config()),
            identity.client_info(),
            None,
            Some(ServerConfigV2 {
                pin: (!config.pin.is_empty()).then(|| config.pin.clone()),
                verify_checksums: true,
                event_tx: events,
            }),
            web,
            rx,
        )
        .await?;
        let mut identity = identity;
        identity.port = server.port();
        let (discovery_stop, rx) = oneshot::channel();
        let discovery = discovery::start(
            DiscoveryConfig {
                group: DEFAULT_MULTICAST_GROUP,
                group_v6: Some(DEFAULT_MULTICAST_GROUP_V6),
                port: DEFAULT_PORT,
                interface_filter: InterfaceFilter {
                    whitelist: (!config.discovery).then(Vec::new),
                    blacklist: None,
                },
                device: identity.multicast_device(),
                identity: DeviceIdentity {
                    cert_pem: identity.cert_pem.clone(),
                    private_key_pem: identity.key_pem.clone(),
                },
                timeout: Duration::from_millis(1000),
                event_tx: None,
            },
            rx,
        )
        .await;
        Ok(Self {
            identity: Arc::new(identity),
            server: Arc::new(server),
            discovery: Arc::new(discovery),
            server_stop: Some(tx),
            discovery_stop: Some(discovery_stop),
        })
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.server_stop.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.discovery_stop.take() {
            let _ = tx.send(());
        }
        self.server.wait_stopped().await;
        self.discovery.wait_stopped().await;
    }

    pub fn urls(&self) -> String {
        self.server
            .local_addresses()
            .iter()
            .map(|a| format!("https://{a}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        if let Some(tx) = self.server_stop.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.discovery_stop.take() {
            let _ = tx.send(());
        }
    }
}

pub fn registered(host: String, info: RegisterDtoV2) -> DiscoveredDevice {
    DiscoveredDevice {
        alias: info.alias,
        version: info.version,
        device_model: info.device_model,
        device_type: info.device_type,
        fingerprint: info.fingerprint,
        channel: DeviceChannel::Http(HttpChannel {
            host,
            port: info.port,
            protocol: info.protocol,
        }),
        download: info.download,
    }
}

pub fn parse_address(address: &str) -> Result<HttpChannel> {
    let input = if address.contains("://") {
        address.trim().to_string()
    } else {
        format!("https://{}", address.trim())
    };
    let url = url::Url::parse(&input)?;
    let protocol = match url.scheme() {
        "https" => ProtocolType::Https,
        "http" => ProtocolType::Http,
        _ => anyhow::bail!("Use an HTTP or HTTPS address"),
    };
    anyhow::ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "Enter only a device address and port"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing device address"))?
        .trim_matches(['[', ']'])
        .to_string();
    Ok(HttpChannel {
        host,
        port: url.port().unwrap_or(DEFAULT_PORT),
        protocol,
    })
}
