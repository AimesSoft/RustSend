//! Native UI contract. Network DTOs stay in the upstream LocalSend core.
#[bern::protocol(namespace = "rustsend", version = 1)]
pub mod transfer {
    use bern::prelude::*;

    #[value]
    pub enum Appearance {
        #[label(tr! { "zh-Hans" => "跟随系统", "en" => "System" })]
        System,
        #[label(tr! { "zh-Hans" => "浅色", "en" => "Light" })]
        Light,
        #[label(tr! { "zh-Hans" => "深色", "en" => "Dark" })]
        Dark,
    }

    #[value]
    pub enum Phase {
        #[label(tr! { "zh-Hans" => "等待确认", "en" => "Waiting for approval" })]
        Waiting,
        #[label(tr! { "zh-Hans" => "传输中", "en" => "Transferring" })]
        Active,
        #[label(tr! { "zh-Hans" => "已完成", "en" => "Completed" })]
        Complete,
        #[label(tr! { "zh-Hans" => "已取消", "en" => "Cancelled" })]
        Cancelled,
        #[label(tr! { "zh-Hans" => "失败", "en" => "Failed" })]
        Failed,
        #[label(tr! { "zh-Hans" => "部分完成", "en" => "Partially completed" })]
        Partial,
    }

    #[value]
    pub struct Device {
        #[content]
        pub id: String,
        #[content]
        pub alias: String,
        #[content]
        pub address: String,
        #[content]
        pub model: String,
        pub favorite: bool,
        pub online: bool,
        pub encrypted: bool,
    }

    #[value]
    pub struct FileItem {
        #[content]
        pub id: String,
        #[content]
        pub name: String,
        #[content]
        pub size: String,
        pub selected: bool,
    }

    #[value]
    pub struct TransferItem {
        #[content]
        pub id: String,
        #[content]
        pub peer: String,
        #[content]
        pub summary: String,
        #[content]
        pub detail: String,
        #[content]
        pub timestamp: String,
        pub sending: bool,
        pub phase: Phase,
        pub progress: f64,
        pub active: bool,
        pub can_open: bool,
    }

    #[state]
    pub struct State {
        #[content]
        pub alias: String,
        #[content]
        pub fingerprint: String,
        #[content]
        pub addresses: String,
        #[content]
        pub destination: String,
        #[content]
        pub port: String,
        #[content]
        pub pin: String,
        pub mobile: bool,
        pub ready: bool,
        pub loading: bool,
        pub discovering: bool,
        pub busy: bool,
        pub quick_save: bool,
        pub favorites_only: bool,
        pub appearance: Appearance,
        pub notice: Option<TransferItem>,
        pub export_complete: bool,
        pub devices: Vec<Device>,
        pub has_devices: bool,
        pub files: Vec<FileItem>,
        pub has_files: bool,
        #[content]
        pub selected_size: String,
        pub transfers: Vec<TransferItem>,
        pub has_transfers: bool,
        #[content]
        pub error: String,
        pub has_error: bool,
        pub pending: bool,
        #[content]
        pub pending_id: String,
        #[content]
        pub pending_alias: String,
        #[content]
        pub pending_fingerprint: String,
        pub pending_files: Vec<FileItem>,
        #[content]
        pub pending_size: String,
        pub pending_count: i64,
        pub can_accept: bool,
        #[content]
        pub pending_text: String,
        pub has_pending_text: bool,
        #[content]
        pub received_text: String,
        pub has_received_text: bool,
        pub sharing: bool,
        #[content]
        pub share_urls: String,
        pub web_pending: bool,
        #[content]
        pub web_peer: String,
    }

    #[commands]
    pub trait Commands {
        /// Announce on the LAN and probe saved devices; fall back to a subnet scan.
        fn discover(&mut self) -> Result<(), CommandError>;
        /// Register a device by IP address or an HTTP(S) origin.
        fn add_address(&mut self, address: String) -> Result<(), CommandError>;
        /// Add a file chosen by the native picker or dropped on the window.
        fn add_file(&mut self, source: MediaSource) -> Result<(), CommandError>;
        /// Add a local file or recursively add a folder.
        fn add_path(&mut self, path: String) -> Result<(), CommandError>;
        /// Choose several files using the desktop file picker.
        fn choose_files(&mut self) -> Result<(), CommandError>;
        /// Choose a directory and add its contents.
        fn choose_folder(&mut self) -> Result<(), CommandError>;
        fn remove_file(&mut self, id: String) -> Result<(), CommandError>;
        fn clear_files(&mut self) -> Result<(), CommandError>;
        /// Send the current selection; PIN belongs to the recipient.
        fn send_files(&mut self, device: String, pin: String) -> Result<(), CommandError>;
        fn send_text(
            &mut self,
            device: String,
            text: String,
            pin: String,
        ) -> Result<(), CommandError>;
        fn toggle_favorite(&mut self, id: String) -> Result<(), CommandError>;
        fn select_incoming(&mut self, id: String, selected: bool) -> Result<(), CommandError>;
        /// Respond only to the displayed session, never to a replacement request.
        fn answer(&mut self, session: String, accept: bool) -> Result<(), CommandError>;
        fn cancel(&mut self, id: String) -> Result<(), CommandError>;
        fn clear_history(&mut self) -> Result<(), CommandError>;
        fn open_received(&mut self, id: String) -> Result<(), CommandError>;
        fn open_destination(&mut self) -> Result<(), CommandError>;
        /// Export a received file to a folder selected by the system picker.
        fn export_received(
            &mut self,
            id: String,
            directory: MediaSource,
        ) -> Result<(), CommandError>;
        /// Export the app's received files to a folder selected by the system picker.
        fn export_destination(&mut self, directory: MediaSource) -> Result<(), CommandError>;
        fn dismiss_export_notice(&mut self) -> Result<(), CommandError>;
        fn choose_destination(&mut self) -> Result<(), CommandError>;
        fn save_settings(
            &mut self,
            alias: String,
            destination: String,
            port: String,
            pin: String,
        ) -> Result<(), CommandError>;
        fn set_quick_save(&mut self, value: bool) -> Result<(), CommandError>;
        /// Generate and save a new insect or crustacean device name.
        fn randomize_alias(&mut self) -> Result<(), CommandError>;
        fn set_favorites_only(&mut self, value: bool) -> Result<(), CommandError>;
        fn set_appearance(&mut self, value: Appearance) -> Result<(), CommandError>;
        fn dismiss_notice(&mut self) -> Result<(), CommandError>;
        fn dismiss_error(&mut self) -> Result<(), CommandError>;
        fn dismiss_text(&mut self) -> Result<(), CommandError>;
        fn copy_text(&mut self) -> Result<(), CommandError>;
        /// Serve the selection through LocalSend's browser download page.
        fn set_sharing(&mut self, enabled: bool) -> Result<(), CommandError>;
        fn answer_web(&mut self, accept: bool) -> Result<(), CommandError>;
    }
}
