//! Black-box tests through the Bern command API and upstream v2 HTTPS client.
use localsend::{
    http::{
        client::{ClientError, LsHttpClientV2},
        dto_v2::{PrepareUploadRequestDtoV2, RegisterDtoV2},
    },
    model::{discovery::ProtocolType, transfer::FileDto},
};
use rustsend_engine::RustSendEngine;
use rustsend_protocol::transfer::{Appearance, Commands, Engine, Phase, State};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

struct Instance {
    app: RustSendEngine,
    root: tempfile::TempDir,
    destination: PathBuf,
    port: u16,
}
impl Instance {
    async fn start(name: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("received");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(
            root.path().join("settings.json"),
            serde_json::to_vec(&serde_json::json!({
                "alias": name, "destination": destination, "port": port, "discovery": false
            }))
            .unwrap(),
        )
        .unwrap();
        let app = RustSendEngine::with_data_dir(root.path().into());
        let mut instance = Self {
            app,
            root,
            destination,
            port,
        };
        instance.wait(|s| s.ready).await;
        instance
    }
    async fn wait(&mut self, predicate: impl Fn(&State) -> bool) {
        let end = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            self.app.wake();
            if predicate(self.app.state()) {
                return;
            }
            assert!(
                tokio::time::Instant::now() < end,
                "timed out: {:?}",
                self.app.state()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    async fn connect(&mut self, other: &Instance) -> String {
        self.app
            .add_address(format!("127.0.0.1:{}", other.port))
            .unwrap();
        self.wait(|s| !s.devices.is_empty()).await;
        self.app
            .state()
            .devices
            .iter()
            .find(|d| d.alias == other.app.state().alias)
            .unwrap()
            .id
            .clone()
    }
    async fn accept(&mut self) {
        self.wait(|s| s.pending).await;
        self.app
            .answer(self.app.state().pending_id.clone(), true)
            .unwrap();
    }
    async fn finished(&mut self, n: usize) {
        self.wait(|s| s.transfers.iter().filter(|t| !t.active).count() >= n && !s.busy)
            .await;
    }
    async fn restart(&mut self) {
        let replacement = RustSendEngine::with_data_dir(self.root.path().into());
        drop(std::mem::replace(&mut self.app, replacement));
        // Dropping the engine requests an asynchronous network shutdown. Wait
        // for the old listener to close before starting another on its port.
        let end = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if std::net::TcpListener::bind(("127.0.0.1", self.port)).is_ok() {
                break;
            }
            assert!(tokio::time::Instant::now() < end, "listener did not stop");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.wait(|s| s.ready).await;
    }
}

#[tokio::test]
async fn random_names_persist_and_update_peers_without_changing_identity() {
    let mut observer = Instance::start("Observer").await;
    let mut device = Instance::start("我的 Mac · 自定义名称").await;
    let fingerprint = device.app.state().fingerprint.clone();
    let identity = std::fs::read(device.root.path().join("identity.pem")).unwrap();

    // Loading an existing installation must never replace a user's alias.
    device.restart().await;
    assert_eq!(device.app.state().alias, "我的 Mac · 自定义名称");
    assert_eq!(device.app.state().fingerprint, fingerprint);

    // Older or partial settings may omit the alias. Persist the generated
    // default immediately, before the user changes any setting.
    let settings = device.root.path().join("settings.json");
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("alias");
    std::fs::write(&settings, serde_json::to_vec(&legacy).unwrap()).unwrap();
    device.restart().await;
    let initial_alias = device.app.state().alias.clone();
    assert!(!initial_alias.trim().is_empty());
    assert_ne!(initial_alias, "我的 Mac · 自定义名称");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(saved["alias"].as_str(), Some(initial_alias.as_str()));
    assert_eq!(saved["port"], legacy["port"]);
    assert_eq!(saved["destination"], legacy["destination"]);
    device.restart().await;
    assert_eq!(device.app.state().alias, initial_alias);
    assert_eq!(device.app.state().fingerprint, fingerprint);
    assert_eq!(observer.connect(&device).await, fingerprint);

    for _ in 0..2 {
        let previous = device.app.state().alias.clone();
        device.app.randomize_alias().unwrap();
        device.wait(|s| s.ready && s.alias != previous).await;
        let generated = device.app.state().alias.clone();
        assert!(!generated.trim().is_empty());
        assert!(generated.chars().count() <= 80);
        assert_eq!(device.app.state().fingerprint, fingerprint);
        assert!(!device.app.state().has_error);
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(device.root.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["alias"].as_str(), Some(generated.as_str()));

        // The isolated test uses a real HTTPS register exchange to rediscover
        // the renamed peer, avoiding dependence on the host's multicast LAN.
        observer
            .app
            .add_address(format!("127.0.0.1:{}", device.port))
            .unwrap();
        observer
            .wait(|s| {
                s.devices
                    .iter()
                    .any(|d| d.id == fingerprint && d.alias == generated)
            })
            .await;
        assert_eq!(
            observer
                .app
                .state()
                .devices
                .iter()
                .filter(|d| d.id == fingerprint)
                .count(),
            1
        );

        device.restart().await;
        assert_eq!(device.app.state().alias, generated);
        assert_eq!(device.app.state().fingerprint, fingerprint);
    }
    assert_eq!(
        std::fs::read(device.root.path().join("identity.pem")).unwrap(),
        identity
    );
}

#[tokio::test]
async fn native_engines_transfer_folders_binary_empty_unicode_and_collisions() {
    let mut a = Instance::start("Sender").await;
    let mut b = Instance::start("Receiver").await;
    let target = a.connect(&b).await;
    assert!(a.app.state().devices[0].encrypted);
    let folder = a.root.path().join("测试 folder");
    std::fs::create_dir_all(folder.join("nested")).unwrap();
    let binary: Vec<u8> = (0..1_200_000).map(|n| (n % 251) as u8).collect();
    std::fs::write(folder.join("nested/data.bin"), &binary).unwrap();
    std::fs::write(folder.join("empty.txt"), []).unwrap();
    a.app.add_path(folder.to_string_lossy().into()).unwrap();
    a.wait(|s| s.files.len() == 2).await;
    for n in 1..=2 {
        a.app.send_files(target.clone(), String::new()).unwrap();
        b.accept().await;
        a.finished(n).await;
        b.finished(n).await;
        assert_eq!(a.app.state().transfers[0].phase, Phase::Complete);
        assert_eq!(b.app.state().transfers[0].phase, Phase::Complete);
        let sent = a.app.state().notice.as_ref().unwrap();
        assert!(sent.sending);
        assert_eq!(sent.phase, Phase::Complete);
        let received = b.app.state().notice.as_ref().unwrap();
        assert!(!received.sending);
        assert!(received.can_open);
        assert_eq!(received.id, b.app.state().transfers[0].id);
        // The notice survives unrelated state updates and is explicitly dismissible.
        b.app.set_appearance(Appearance::Dark).unwrap();
        b.wait(|s| s.appearance == Appearance::Dark).await;
        assert!(b.app.state().notice.is_some());
        b.app.dismiss_notice().unwrap();
        b.wait(|s| s.notice.is_none()).await;
    }
    assert_eq!(
        std::fs::read(b.destination.join("测试 folder/nested/data.bin")).unwrap(),
        binary
    );
    assert_eq!(
        std::fs::read(b.destination.join("测试 folder/nested/data (1).bin")).unwrap(),
        binary
    );
    assert_eq!(
        std::fs::metadata(b.destination.join("测试 folder/empty.txt"))
            .unwrap()
            .len(),
        0
    );
    assert!(std::fs::read_dir(&b.destination).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".localsend-")
    }));
}

#[tokio::test]
async fn subset_decline_message_and_cancel_waiting_are_real_protocol_outcomes() {
    let mut a = Instance::start("Sender").await;
    let mut b = Instance::start("Receiver").await;
    let target = a.connect(&b).await;
    for name in ["one.txt", "two.txt"] {
        let path = a.root.path().join(name);
        std::fs::write(&path, name).unwrap();
        a.app.add_path(path.display().to_string()).unwrap();
        a.wait(|s| !s.loading && s.files.iter().any(|f| f.name == name))
            .await;
    }
    a.app.send_files(target.clone(), String::new()).unwrap();
    b.wait(|s| s.pending).await;
    let omitted = b
        .app
        .state()
        .pending_files
        .iter()
        .find(|f| f.name == "two.txt")
        .unwrap()
        .id
        .clone();
    b.app.select_incoming(omitted, false).unwrap();
    b.accept().await;
    a.finished(1).await;
    b.finished(1).await;
    assert_eq!(a.app.state().transfers[0].phase, Phase::Partial);
    assert_eq!(a.app.state().notice.as_ref().unwrap().phase, Phase::Partial);
    assert!(b.destination.join("one.txt").exists());
    assert!(!b.destination.join("two.txt").exists());
    a.app
        .send_text(target.clone(), "Hello 世界 🦀".into(), String::new())
        .unwrap();
    b.accept().await;
    a.finished(2).await;
    b.wait(|s| s.received_text == "Hello 世界 🦀").await;
    a.app.send_files(target.clone(), String::new()).unwrap();
    b.wait(|s| s.pending).await;
    b.app
        .answer(b.app.state().pending_id.clone(), false)
        .unwrap();
    a.finished(3).await;
    assert_eq!(a.app.state().transfers[0].phase, Phase::Failed);
    a.app.send_files(target, String::new()).unwrap();
    b.wait(|s| s.pending).await;
    a.wait(|s| s.transfers.iter().any(|t| t.active)).await;
    let id = a
        .app
        .state()
        .transfers
        .iter()
        .find(|t| t.active)
        .unwrap()
        .id
        .clone();
    a.app.cancel(id).unwrap();
    a.finished(4).await;
    b.wait(|s| !s.pending).await;
    assert_eq!(a.app.state().transfers[0].phase, Phase::Cancelled);
}

fn external_client(receiver: &Instance) -> (Arc<LsHttpClientV2>, RegisterDtoV2) {
    let cert = localsend::crypto::cert::generate_self_signed().unwrap();
    let client = LsHttpClientV2::try_new(
        &cert.private_key_pem,
        &cert.certificate_pem,
        Some(receiver.app.state().fingerprint.clone()),
        None,
    )
    .unwrap();
    let info = RegisterDtoV2 {
        alias: "Upstream protocol client".into(),
        version: "2.2".into(),
        device_model: None,
        device_type: None,
        fingerprint: cert.fingerprint,
        port: 53318,
        protocol: ProtocolType::Https,
        download: false,
    };
    (Arc::new(client), info)
}
fn file(name: &str) -> FileDto {
    FileDto {
        id: "external-file".into(),
        file_name: name.into(),
        size: 4,
        file_type: "application/octet-stream".into(),
        sha256: None,
        preview: None,
        metadata: None,
    }
}

#[tokio::test]
async fn upstream_client_pin_checksum_and_traversal_checks() {
    let mut b = Instance::start("Receiver").await;
    b.app
        .save_settings(
            "Renamed".into(),
            b.destination.display().to_string(),
            b.port.to_string(),
            "1234".into(),
        )
        .unwrap();
    b.wait(|s| s.ready && s.alias == "Renamed" && s.pin == "1234")
        .await;
    let (client, info) = external_client(&b);
    let mut dto = file("broken.bin");
    dto.sha256 = Some("0".repeat(64));
    let payload = PrepareUploadRequestDtoV2 {
        info: info.clone(),
        files: HashMap::from([(dto.id.clone(), dto)]),
    };
    let wrong = client
        .prepare_upload(
            ProtocolType::Https,
            "127.0.0.1",
            b.port,
            None,
            payload.clone(),
            Some("9999"),
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(wrong, Err(ClientError::StatusCode(e)) if e.status == 401));
    let port = b.port;
    let c = client.clone();
    let task = tokio::spawn(async move {
        c.prepare_upload(
            ProtocolType::Https,
            "127.0.0.1",
            port,
            None,
            payload,
            Some("1234"),
            CancellationToken::new(),
        )
        .await
    });
    b.accept().await;
    let prepared = task.await.unwrap().unwrap().response.unwrap();
    let uploaded = client
        .upload(
            ProtocolType::Https,
            "127.0.0.1",
            b.port,
            None,
            &prepared.session_id,
            "external-file",
            &prepared.files["external-file"],
            localsend::reqwest::Body::from("data"),
            CancellationToken::new(),
        )
        .await;
    assert!(uploaded.is_err());
    b.finished(1).await;
    assert_eq!(b.app.state().transfers[0].phase, Phase::Failed);
    assert!(!b.destination.join("broken.bin").exists());
    let malicious = file("../escape.txt");
    let result = client
        .prepare_upload(
            ProtocolType::Https,
            "127.0.0.1",
            b.port,
            None,
            PrepareUploadRequestDtoV2 {
                info,
                files: HashMap::from([(malicious.id.clone(), malicious)]),
            },
            Some("1234"),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());
    assert!(!b.root.path().join("escape.txt").exists());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(b.root.path().join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(config["alias"], "Renamed");
    assert_eq!(config["pin"], "1234");
}

#[tokio::test]
async fn browser_share_requires_approval_and_serves_selected_bytes() {
    let mut a = Instance::start("Browser share").await;
    let path = a.root.path().join("browser.txt");
    std::fs::write(&path, "browser bytes").unwrap();
    a.app.add_path(path.display().to_string()).unwrap();
    a.wait(|s| s.has_files).await;
    a.app.set_sharing(true).unwrap();
    a.wait(|s| s.ready && s.sharing).await;
    let client = Arc::new(LsHttpClientV2::try_new_without_cert().unwrap());
    let port = a.port;
    let c = client.clone();
    let task = tokio::spawn(async move {
        c.prepare_download(ProtocolType::Https, "127.0.0.1", port, None, None)
            .await
    });
    a.wait(|s| s.web_pending).await;
    assert!(!task.is_finished());
    a.app.answer_web(true).unwrap();
    let response = task.await.unwrap().unwrap();
    let id = response.files.keys().next().unwrap();
    let raw = localsend::reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .unwrap();
    let mut download = url::Url::parse(&format!(
        "https://127.0.0.1:{}/api/localsend/v2/download",
        a.port
    ))
    .unwrap();
    download.query_pairs_mut().extend_pairs([
        ("sessionId", response.session_id.as_str()),
        ("fileId", id.as_str()),
    ]);
    let bytes = raw
        .get(download)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"browser bytes");
    a.app.set_sharing(false).unwrap();
    a.wait(|s| !s.sharing && s.ready).await;
}

#[tokio::test]
async fn appearance_survives_restart_and_legacy_theme_fields_are_ignored() {
    let mut instance = Instance::start("Appearance test").await;
    let settings_path = instance.root.path().join("settings.json");
    let identity_path = instance.root.path().join("identity.pem");
    let identity = std::fs::read(&identity_path).unwrap();
    let fingerprint = instance.app.state().fingerprint.clone();
    let saved_file = instance.destination.join("previous.txt");
    std::fs::write(&saved_file, "previous transfer").unwrap();
    std::fs::write(
        instance.root.path().join("history.json"),
        serde_json::to_vec(&serde_json::json!([{
            "id": "previous-transfer", "peer": "Peer", "summary": "previous.txt",
            "detail": "Completed", "timestamp": "2026-10-23 10:00",
            "sending": false, "phase": "complete", "path": saved_file
        }]))
        .unwrap(),
    )
    .unwrap();

    for (appearance, stored_value) in [
        (Appearance::Light, "light"),
        (Appearance::Dark, "dark"),
        (Appearance::System, "system"),
    ] {
        instance.app.set_appearance(appearance.clone()).unwrap();
        instance.wait(|s| s.appearance == appearance).await;
        let mut stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(stored["appearance"], stored_value);
        assert!(stored.get("color_mode").is_none());
        assert!(stored.get("custom_color").is_none());

        drop(instance.app);
        let end = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if std::net::TcpListener::bind(("127.0.0.1", instance.port)).is_ok() {
                break;
            }
            assert!(tokio::time::Instant::now() < end, "server did not stop");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        // Existing installs can retain any obsolete color values without affecting
        // their appearance preference or preventing the remaining settings loading.
        stored["color_mode"] = serde_json::json!("oled");
        stored["custom_color"] = serde_json::json!("no longer a valid color");
        std::fs::write(&settings_path, serde_json::to_vec(&stored).unwrap()).unwrap();

        instance.app = RustSendEngine::with_data_dir(instance.root.path().into());
        instance.wait(|s| s.ready).await;
        let state = instance.app.state();
        assert!(!state.has_error, "{}", state.error);
        assert_eq!(state.appearance, appearance);
        assert_eq!(state.alias, "Appearance test");
        assert_eq!(
            state.destination,
            instance.destination.display().to_string()
        );
        assert_eq!(state.fingerprint, fingerprint);
        assert_eq!(std::fs::read(&identity_path).unwrap(), identity);
        assert_eq!(state.transfers.len(), 1);
        assert_eq!(state.transfers[0].id, "previous-transfer");
        assert!(state.transfers[0].can_open);
        assert!(
            state.notice.is_none(),
            "history must not replay a completion notice"
        );
    }
}
