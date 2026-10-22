//! Native desktop and mobile interfaces share the same transfer engine.
use bern::prelude::*;
use rustsend_protocol::transfer;
use rustsend_protocol::transfer::Phase;

// Keep standard controls native; only the floating notice needs a custom surface.
style! {
    accents { monochrome: color(light: "#333333", dark: "#D6D6D6"), }
    Stack.shell { background: surface.canvas, }
    Stack.toast { background: surface.raised, foreground: text.primary, corner: 10, }
    #[target(macos)]
    Stack.toast { shadow: 8, }
}

#[bern::value]
pub enum MonoAccent {
    #[label(tr! { "zh-Hans" => "灰色", "en" => "Monochrome" })]
    Monochrome,
}

#[bern::value]
pub enum Page {
    #[label(tr! { "zh-Hans" => "接收", "en" => "Receive" })]
    Receive,
    #[label(tr! { "zh-Hans" => "发送", "en" => "Send" })]
    Send,
    #[label(tr! { "zh-Hans" => "传输记录", "en" => "Transfers" })]
    History,
    #[label(tr! { "zh-Hans" => "设置", "en" => "Settings" })]
    Settings,
}

#[bern::session]
pub struct NavigationDraft {
    pub page: Page,
}

#[bern::session]
pub struct SendDraft {
    pub file: MediaSource,
    pub address: String,
    pub path: String,
    pub text: String,
    pub pin: String,
}

#[bern::value]
pub enum SettingsPage {
    #[label(tr! { "zh-Hans" => "通用", "en" => "General" })]
    General,
    #[label(tr! { "zh-Hans" => "接收", "en" => "Receiving" })]
    Receiving,
    #[label(tr! { "zh-Hans" => "网络", "en" => "Network" })]
    Network,
}

#[bern::session]
pub struct SettingsDraft {
    pub page: SettingsPage,
    pub alias: String,
    pub destination: MediaSource,
    pub port: String,
    pub pin: String,
}

#[bern::session]
pub struct ExportDraft {
    pub directory: MediaSource,
}

#[component]
pub fn app(app: transfer::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main, accent: MonoAccent::Monochrome, appearance: app.appearance) {
            Window(id: main, title: tr! { "zh-Hans" => "RustSend", "en" => "RustSend" }, initial: open, chrome: unified, width: 440, height: 560, min_width: 360, min_height: 440) {
                Stack(variant: shell, align: bottom_trailing) {
                    Stack(enabled: !app.pending && !app.web_pending) {
                        if app.mobile {
                            mobile(app: app)
                        } else {
                            Adaptive(id: navigation) {
                                Layout(size: compact) { compact(app: app) }
                                Layout(size: medium) { desktop(app: app) }
                                Layout(size: expanded) { desktop(app: app) }
                            }
                        }
                    }
                    if !app.pending && !app.web_pending {
                        transfer_notice(app: app)
                    }
                    if app.pending {
                        if app.mobile {
                            Modal(width: 600, height: 420, cover: compact) { incoming(app: app) }
                        } else {
                            Modal(width: 600, height: 420) { incoming(app: app) }
                        }
                    } else if app.web_pending {
                        Modal(width: 440) {
                            Stack {
                                Column(gap: 20, padding: 24) {
                                    Text(content: tr! { "zh-Hans" => "下载请求", "en" => "Download request" }, style: title2)
                                    Text(content: tr! { "zh-Hans" => "{peer} 请求下载共享文件", "en" => "{peer} requests your shared files"; peer: app.web_peer })
                                    Row(gap: 12) {
                                        Button(id: decline_web, label: tr! { "zh-Hans" => "拒绝", "en" => "Decline" }) => app.answer_web(false)
                                        Button(id: accept_web, label: tr! { "zh-Hans" => "允许下载", "en" => "Allow download" }, prominence: prominent) => app.answer_web(true)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// A separate, content-sized layer: notifications never enter the page's layout.
#[component]
pub fn transfer_notice(app: transfer::Client) -> impl View {
    ui! {
        if app.has_error || app.export_complete {
            notice_overlay(app: app)
        } else { if let Some(notice) = app.notice {
            notice_overlay(app: app)
        } }
    }
}

#[component]
pub fn notice_overlay(app: transfer::Client) -> impl View {
    ui! {
        if window.compact {
                Column(padding_x: 12, padding_y: if app.mobile { 88 } else { 12 }) {
                    notice_content(app: app)
                }
            } else {
            Column(padding: 16, width: 392) { notice_content(app: app) }
        }
    }
}

#[component]
pub fn notice_content(app: transfer::Client, #[session] draft: ExportDraft) -> impl View {
    ui! {
        Stack(variant: toast) {
            Column(gap: 10, padding: 16) {
                if app.has_error {
                    Row(gap: 12) {
                        Label(title: tr! { "zh-Hans" => "操作未完成", "en" => "Action could not complete" }, icon: "exclamationmark.triangle", style: headline)
                        Spacer
                        IconButton(id: dismiss_error, icon: "xmark", label: tr! { "zh-Hans" => "关闭提示", "en" => "Dismiss" }) => app.dismiss_error()
                    }
                    Text(content: app.error, style: caption, lines: 3, emphasis: secondary)
                } else if app.export_complete {
                    Row(gap: 12) {
                        Label(title: tr! { "zh-Hans" => "已导出到所选文件夹", "en" => "Exported to the selected folder" }, icon: "checkmark.circle", style: headline)
                        Spacer
                        IconButton(id: dismiss_export, icon: "xmark", label: tr! { "zh-Hans" => "关闭提示", "en" => "Dismiss" }) => app.dismiss_export_notice()
                    }
                } else { if let Some(notice) = app.notice {
                    Row(gap: 12) {
                        if notice.phase == Phase::Complete {
                            if notice.sending {
                                Label(title: tr! { "zh-Hans" => "发送完成", "en" => "Transfer sent" }, icon: "checkmark.circle", style: headline)
                            } else {
                                Label(title: tr! { "zh-Hans" => "接收完成", "en" => "Transfer received" }, icon: "checkmark.circle", style: headline)
                            }
                        } else {
                            Label(title: tr! { "zh-Hans" => "{phase}", "en" => "{phase}"; phase: notice.phase }, icon: "exclamationmark.triangle", style: headline)
                        }
                        Spacer
                        IconButton(id: dismiss_notice, icon: "xmark", label: tr! { "zh-Hans" => "关闭完成提示", "en" => "Dismiss completion notice" }) => app.dismiss_notice()
                    }
                    Text(content: tr! { "zh-Hans" => "{peer} · {summary}", "en" => "{peer} · {summary}"; peer: notice.peer, summary: notice.summary }, style: caption, lines: 2, emphasis: secondary)
                    if notice.can_open {
                        Flow(gap: 12) {
                            Button(id: open_notice, label: tr! { "zh-Hans" => "打开文件", "en" => "Open file" }, icon: "doc", prominence: plain) => app.open_received(notice.id)
                            if app.mobile {
                                Button(id: export_notice, label: tr! { "zh-Hans" => "导出…", "en" => "Export…" }, icon: "folder", pick: draft.directory, files: "folder") => app.export_received(notice.id, draft.directory)
                            }
                        }
                    }
                } }
            }
        }
    }
}

// The navigation stays mounted; page changes never alter the system toolbar.
#[component]
pub fn desktop(app: transfer::Client, #[session] draft: NavigationDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            Row(gap: 16, padding: 16) {
                Image(source: "bern/ui/assets/rustsend-512.png", width: 28, height: 28)
                Text(content: tr! { "zh-Hans" => "RustSend", "en" => "RustSend" }, style: title2)
                Spacer
                Column(width: 360) {
                    Picker(id: pages, label: tr! { "zh-Hans" => "页面", "en" => "Pages" }, label_hidden: true, value: draft.page, style: segmented)
                }
            }
            Divider
            page_content(app: app)
        }
    }
}

#[component]
pub fn compact(app: transfer::Client, #[session] draft: NavigationDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            Column(padding: 12) {
                Picker(id: pages, label: tr! { "zh-Hans" => "页面", "en" => "Pages" }, label_hidden: true, value: draft.page, style: segmented)
            }
            Divider
            page_content(app: app)
        }
    }
}

#[component]
pub fn mobile(app: transfer::Client, #[session] draft: NavigationDraft) -> impl View {
    ui! {
        Tabs(id: phone_tabs, selection: draft.page) {
            Tab(value: Page::Receive, label: tr! { "zh-Hans" => "接收", "en" => "Receive" }, icon: "arrow.down.circle") { receive_page(app: app) }
            Tab(value: Page::Send, label: tr! { "zh-Hans" => "发送", "en" => "Send" }, icon: "paperplane") { send_page(app: app) }
            Tab(value: Page::History, label: tr! { "zh-Hans" => "记录", "en" => "Transfers" }, icon: "arrow.up.arrow.down") { history_page(app: app) }
            Tab(value: Page::Settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape") { settings_page(app: app) }
        }
    }
}

#[component]
pub fn page_content(app: transfer::Client, #[session] draft: NavigationDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            match draft.page {
                Page::Receive => { receive_page(app: app) }
                Page::Send => { send_page(app: app) }
                Page::History => { history_page(app: app) }
                Page::Settings => { settings_page(app: app) }
            }
        }
    }
}

#[component]
pub fn receive_page(app: transfer::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: if window.compact { 16 } else { 24 }, padding: if window.compact { 20 } else { 32 }) {
                Row {
                    PopoverButton(id: receive_options, label: tr! { "zh-Hans" => "保存选项", "en" => "Save options" }, icon: "folder") {
                        receive_options(app: app)
                    }
                    Spacer
                    PopoverButton(id: network_info, label: tr! { "zh-Hans" => "网络信息", "en" => "Network info" }, icon: "info.circle") {
                        Column(gap: 12, padding: 20) {
                            Text(content: tr! { "zh-Hans" => "本机地址", "en" => "This device" }, style: headline)
                            Text(content: app.addresses, style: caption)
                            Text(content: tr! { "zh-Hans" => "连接同一 Wi-Fi，即可互相发现。", "en" => "Connect to the same Wi-Fi to discover each other." }, style: caption, emphasis: secondary)
                        }
                    }
                }
                Row {
                    Spacer
                    Column(gap: 12, padding_y: if window.compact { 12 } else { 28 }, align: center) {
                        Svg(source: "bern/ui/assets/localsend.svg", width: if window.compact { 112 } else { 200 }, height: if window.compact { 112 } else { 200 }, label: tr! { "zh-Hans" => "RustSend 标志", "en" => "RustSend logo" }, playing: app.ready && !app.pending, interactive: false)
                        Text(content: app.alias, style: large_title, font_size: if window.compact { 28 } else { 48 }, lines: 1)
                        if app.ready {
                            Label(title: tr! { "zh-Hans" => "已就绪，可以接收", "en" => "Ready to receive" }, icon: "checkmark.circle", style: headline)
                        } else {
                            Button(id: retry, label: tr! { "zh-Hans" => "启动接收服务", "en" => "Start receiver" }, prominence: prominent) => app.discover()
                        }
                        Text(content: tr! { "zh-Hans" => "在另一台设备上选择这个名称", "en" => "Choose this name on your other device" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                }
                Divider
                Switch(id: quick_save, label: tr! { "zh-Hans" => "快速保存", "en" => "Quick save" }, checked: app.quick_save) => app.set_quick_save(app.quick_save)
                if app.has_received_text {
                    Column(gap: 12) {
                        Text(content: tr! { "zh-Hans" => "收到的消息", "en" => "Received message" }, style: headline)
                        Text(content: app.received_text, lines: 2)
                        Row(gap: 12) {
                            PopoverButton(id: full_message, label: tr! { "zh-Hans" => "全文", "en" => "Full message" }) {
                                Column(padding: 16, width: 280, height: 240) {
                                    ScrollView { Text(content: app.received_text) }
                                }
                            }
                            Button(id: copy_message, label: tr! { "zh-Hans" => "复制", "en" => "Copy" }, icon: "square.on.square") => app.copy_text()
                            Button(id: dismiss_message, label: tr! { "zh-Hans" => "关闭消息", "en" => "Dismiss message" }) => app.dismiss_text()
                        }
                    }
                }
                transfers(app: app, active_only: true)
            }
        }
    }
}

#[component]
pub fn receive_options(app: transfer::Client, #[session] draft: ExportDraft) -> impl View {
    ui! {
        Column(gap: 16, padding: 20, width: 280) {
            Text(content: tr! { "zh-Hans" => "接收文件", "en" => "Received files" }, style: headline)
            if app.mobile {
                Text(content: tr! { "zh-Hans" => "文件保存在 RustSend 内，可从记录中打开，或导出到手机文件夹。", "en" => "Files are saved in RustSend. Open them from Transfers or export them to a folder." }, style: caption, emphasis: secondary)
                Button(id: export_destination, label: tr! { "zh-Hans" => "导出接收文件…", "en" => "Export received files…" }, icon: "folder", enabled: !app.busy, pick: draft.directory, files: "folder") => app.export_destination(draft.directory)
            } else {
                Text(content: app.destination, style: caption, emphasis: secondary)
                Button(id: open_downloads, label: tr! { "zh-Hans" => "打开文件夹", "en" => "Open folder" }, icon: "folder") => app.open_destination()
                Button(id: destination, label: tr! { "zh-Hans" => "更改保存位置…", "en" => "Change folder…" }, enabled: !app.busy) => app.choose_destination()
            }
            Divider
            Switch(id: favorites_only, label: tr! { "zh-Hans" => "仅自动接受收藏设备", "en" => "Auto-accept favorites only" }, checked: app.favorites_only) => app.set_favorites_only(app.favorites_only)
            Text(content: tr! { "zh-Hans" => "关闭快速保存时，每次接收前由你确认。", "en" => "With Quick save off, you approve each incoming transfer." }, style: caption, emphasis: secondary)
        }
    }
}

#[component]
pub fn send_page(app: transfer::Client, #[session] draft: SendDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 16, padding: if window.compact { 16 } else { 32 }) {
                if !window.compact {
                    Text(content: tr! { "zh-Hans" => "发送", "en" => "Send" }, style: large_title)
                }
                Drop(id: file_drop, label: tr! { "zh-Hans" => "拖入文件", "en" => "Drop a file" }, value: draft.file) {
                    Column(gap: 12) {
                        Flow(gap: 10) {
                            if app.mobile {
                                Button(id: pick_files, label: tr! { "zh-Hans" => "文件", "en" => "Files" }, icon: "doc", prominence: prominent, enabled: !app.loading && !app.sharing, pick: draft.file, files: "any") => app.add_file(draft.file)
                                Button(id: pick_folder, label: tr! { "zh-Hans" => "文件夹", "en" => "Folder" }, icon: "folder", enabled: !app.loading && !app.sharing, pick: draft.file, files: "folder") => app.add_file(draft.file)
                            } else {
                                Button(id: choose_files, label: tr! { "zh-Hans" => "文件", "en" => "Files" }, icon: "doc", prominence: prominent, enabled: !app.loading && !app.sharing, shortcut: "cmd+o") => app.choose_files()
                                Button(id: choose_folder, label: tr! { "zh-Hans" => "文件夹", "en" => "Folder" }, icon: "folder", enabled: !app.loading && !app.sharing) => app.choose_folder()
                            }
                            PopoverButton(id: compose, label: tr! { "zh-Hans" => "文字", "en" => "Text" }) {
                                Column(gap: 12, padding: 16, width: 280) {
                                    Text(content: tr! { "zh-Hans" => "文字消息", "en" => "Text message" }, style: headline)
                                    TextInput(id: message, label: tr! { "zh-Hans" => "文字消息", "en" => "Text message" }, label_hidden: true, value: draft.text, kind: multiline, prompt: tr! { "zh-Hans" => "输入文字或粘贴链接…", "en" => "Type a message or paste a link…" })
                                    Text(content: tr! { "zh-Hans" => "输入后，选择设备发送消息。", "en" => "Then choose a device to send the message." }, style: caption, emphasis: secondary)
                                }
                            }
                        }
                        if app.loading { Progress(label: tr! { "zh-Hans" => "正在读取文件…", "en" => "Reading selection…" }) }
                        if app.has_files {
                            Row(gap: 12) {
                                PopoverButton(id: selected_files, label: tr! { "zh-Hans" => "已选文件", "en" => "Selected files" }, icon: "doc") {
                                    selected_files(app: app)
                                }
                                Text(content: app.selected_size, style: caption, emphasis: secondary)
                                Spacer
                                Button(id: clear, label: tr! { "zh-Hans" => "清空", "en" => "Clear" }, prominence: plain, enabled: !app.sharing) => app.clear_files()
                            }
                        } else if !app.mobile {
                            Text(content: tr! { "zh-Hans" => "也可以把文件直接拖到这里", "en" => "You can also drop files here" }, style: caption, emphasis: secondary)
                        }
                        if draft.text != "" { Text(content: draft.text, lines: 2, style: caption, emphasis: secondary) }
                    }
                } => app.add_file(draft.file)
                Divider
                Row(gap: 12) {
                    Text(content: tr! { "zh-Hans" => "附近的设备", "en" => "Nearby devices" }, style: headline)
                    Spacer
                    IconButton(id: refresh, icon: "arrow.triangle.2.circlepath", label: tr! { "zh-Hans" => "刷新", "en" => "Refresh" }, enabled: !app.discovering) => app.discover()
                    PopoverButton(id: connection_options, label: tr! { "zh-Hans" => "连接与选项", "en" => "Connection options" }, icon: "ellipsis") {
                        Column(gap: 16, padding: 16, width: 280) {
                            TextInput(id: address, label: tr! { "zh-Hans" => "设备地址", "en" => "Device address" }, value: draft.address, prompt: tr! { "zh-Hans" => "192.168.1.10:53317", "en" => "192.168.1.10:53317" }) => app.add_address(draft.address)
                            Button(id: connect, label: tr! { "zh-Hans" => "手动连接", "en" => "Connect manually" }, prominence: prominent, enabled: draft.address != "") => app.add_address(draft.address)
                            Divider
                            TextInput(id: recipient_pin, label: tr! { "zh-Hans" => "对方的 PIN（如有）", "en" => "Recipient PIN (if required)" }, value: draft.pin, kind: secure)
                            Switch(id: share, label: tr! { "zh-Hans" => "浏览器分享", "en" => "Browser sharing" }, checked: app.sharing) => app.set_sharing(app.sharing)
                            if app.sharing { Text(content: app.share_urls, style: caption) }
                        }
                    }
                }
                if app.discovering { Progress(label: tr! { "zh-Hans" => "正在发现设备…", "en" => "Finding devices…" }) }
                if !app.has_devices {
                    Column(gap: 8, padding_y: 12) {
                        Label(title: tr! { "zh-Hans" => "等待附近的设备", "en" => "Waiting for nearby devices" }, icon: "wifi", style: headline)
                        Text(content: tr! { "zh-Hans" => "在另一台设备打开 RustSend 或 LocalSend，并连接同一 Wi-Fi。", "en" => "Open RustSend or LocalSend on another device on the same Wi-Fi." }, style: caption, emphasis: secondary)
                    }
                }
                ForEach(items: app.devices, key: id) |device| {
                    Row(gap: 12) {
                        Column(gap: 4, width: if window.compact { 140 } else { 260 }) {
                            Text(content: device.alias, style: headline, lines: 1)
                            Text(content: tr! { "zh-Hans" => "{model} · {address}", "en" => "{model} · {address}"; model: device.model, address: device.address }, style: caption2, lines: 1, emphasis: secondary)
                            if !device.online { Text(content: tr! { "zh-Hans" => "离线", "en" => "Offline" }, style: caption) }
                        }
                        Spacer
                        if device.favorite {
                            IconButton(id: unfavorite, icon: "star.fill", label: tr! { "zh-Hans" => "取消收藏", "en" => "Remove favorite" }) => app.toggle_favorite(device.id)
                        } else {
                            IconButton(id: favorite, icon: "star", label: tr! { "zh-Hans" => "收藏设备", "en" => "Favorite device" }) => app.toggle_favorite(device.id)
                        }
                        Column(gap: 8, width: 80) {
                            Button(id: send_files, label: tr! { "zh-Hans" => "发送", "en" => "Send" }, width: fill, prominence: prominent, enabled: device.online && app.has_files) => app.send_files(device.id, draft.pin)
                            if draft.text != "" {
                                Button(id: send_message, label: tr! { "zh-Hans" => "发消息", "en" => "Message" }, width: fill, enabled: device.online) => app.send_text(device.id, draft.text, draft.pin)
                            }
                        }
                    }
                    Divider
                }
                transfers(app: app, active_only: true)
            }
        }
    }
}

#[component]
pub fn selected_files(app: transfer::Client) -> impl View {
    ui! {
        Column(gap: 12, padding: 16, width: 280, height: 280) {
            Text(content: tr! { "zh-Hans" => "已选文件", "en" => "Selected files" }, style: headline)
            ScrollView {
                Column(gap: 12) {
                    ForEach(items: app.files, key: id) |file| {
                        Row(gap: 8) {
                            Column(gap: 4) {
                                Text(content: file.name, lines: 2)
                                Text(content: file.size, style: caption, emphasis: secondary)
                            }
                            Spacer
                            IconButton(id: remove_file, icon: "xmark", label: tr! { "zh-Hans" => "移除文件", "en" => "Remove file" }, enabled: !app.sharing) => app.remove_file(file.id)
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn incoming(app: transfer::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            ScrollView {
                Column(gap: 16, padding: if window.compact { 16 } else { 28 }) {
                    Row(gap: 18) {
                        Image(source: "bern/ui/assets/rustsend-512.png", width: 40, height: 40)
                        Column(gap: 8) {
                            Text(content: tr! { "zh-Hans" => "收到分享", "en" => "Incoming share" }, style: caption, emphasis: secondary)
                            Text(content: app.pending_alias, style: title)
                            Text(content: tr! { "zh-Hans" => "想要向你发送内容", "en" => "would like to send you something" }, emphasis: secondary)
                        }
                        Spacer
                    }
                    Stack {
                        Column(gap: 12) {
                            Row {
                                if app.has_pending_text {
                                    Text(content: tr! { "zh-Hans" => "文字消息", "en" => "Text message" }, style: headline)
                                } else {
                                    Text(content: tr! { "zh-Hans" => "已选择 {count} 个文件", "en" => "{count} files selected"; count: app.pending_count }, style: headline)
                                }
                                Spacer
                                Text(content: app.pending_size, style: caption, emphasis: secondary)
                            }
                            Divider
                            if app.has_pending_text {
                                Text(content: app.pending_text)
                            } else {
                                ForEach(items: app.pending_files, key: id) |file| {
                                    Column(gap: 4) {
                                        Switch(id: selected, label: file.name, checked: file.selected, style: checkbox) => app.select_incoming(file.id, file.selected)
                                        Text(content: file.size, style: caption, emphasis: secondary)
                                    }
                                }
                            }
                        }
                    }
                    Column(gap: 10) {
                        Row {
                            Label(title: tr! { "zh-Hans" => "保存到", "en" => "Save to" }, icon: "folder", style: caption)
                            Spacer
                            PopoverButton(id: security, label: tr! { "zh-Hans" => "加密连接 · 设备详情", "en" => "Encrypted · Device details" }) {
                                Column(gap: 12, padding: 20, width: 280) {
                                    Text(content: tr! { "zh-Hans" => "设备指纹", "en" => "Device fingerprint" }, style: headline)
                                    Text(content: app.pending_fingerprint, style: caption)
                                }
                            }
                        }
                        if app.mobile {
                            Text(content: tr! { "zh-Hans" => "RustSend 内，可在接收后导出到手机文件夹", "en" => "In RustSend; export to a folder after receiving" }, style: caption, emphasis: secondary)
                        } else {
                            Text(content: app.destination, style: caption, emphasis: secondary)
                        }
                    }
                }
            }
            Divider
            Row(gap: 16, padding: 16) {
                Button(id: decline, label: tr! { "zh-Hans" => "拒绝", "en" => "Decline" }, role: cancel, width: fill) => app.answer(app.pending_id, false)
                Button(id: accept, label: tr! { "zh-Hans" => "接收", "en" => "Accept" }, prominence: prominent, width: fill, enabled: app.can_accept) => app.answer(app.pending_id, true)
            }
        }
    }
}

#[component]
pub fn transfers(
    app: transfer::Client,
    active_only: bool,
    #[session] draft: ExportDraft,
) -> impl View {
    ui! {
        Column(gap: 16) {
            ForEach(items: app.transfers, key: id) |item| {
                if !active_only || item.active {
                    Divider
                    Stack(align: top_leading) {
                        Column(gap: 8, padding_y: 12) {
                            Row(gap: 12) {
                                if item.sending { Label(title: item.peer, icon: "paperplane", style: headline) }
                                else { Label(title: item.peer, icon: "arrow.down.circle", style: headline) }
                                Spacer
                                Text(content: tr! { "zh-Hans" => "{phase}", "en" => "{phase}"; phase: item.phase }, style: caption)
                            }
                            Text(content: item.summary, lines: 2)
                            if item.active { Progress(value: item.progress, total: 1) }
                            Row(gap: 12) {
                                Text(content: item.detail, style: caption, emphasis: secondary)
                                Spacer
                                if item.active { Button(id: cancel, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => app.cancel(item.id) }
                                if item.can_open {
                                    Button(id: open, label: tr! { "zh-Hans" => "打开", "en" => "Open" }) => app.open_received(item.id)
                                    if app.mobile {
                                        Button(id: export, label: tr! { "zh-Hans" => "导出", "en" => "Export" }, pick: draft.directory, files: "folder") => app.export_received(item.id, draft.directory)
                                    }
                                }
                            }
                            Text(content: item.timestamp, style: caption2, emphasis: tertiary)
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn history_page(app: transfer::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: if window.compact { 16 } else { 24 }, padding: if window.compact { 16 } else { 32 }) {
                Row(gap: 12) {
                    Text(content: tr! { "zh-Hans" => "传输记录", "en" => "Transfers" }, style: large_title)
                    Spacer
                    Button(id: clear_history, label: tr! { "zh-Hans" => "清空记录", "en" => "Clear history" }) => app.clear_history()
                }
                if !app.has_transfers { Text(content: tr! { "zh-Hans" => "还没有传输记录。", "en" => "No transfers yet." }, emphasis: secondary) }
                transfers(app: app, active_only: false)
            }
        }
    }
}

#[component]
pub fn settings_page(app: transfer::Client, #[session] draft: SettingsDraft) -> impl View {
    ui! {
        Column(gap: 0) {
            Column(padding_x: 16, padding_y: 8) {
                Picker(id: settings_pages, label: tr! { "zh-Hans" => "设置分类", "en" => "Settings category" }, label_hidden: true, value: draft.page, style: segmented)
            }
            Form(gap: 12) {
                match draft.page {
                    SettingsPage::General => {
                        Section {
                            Picker(id: appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, selected: app.appearance) => app.set_appearance(app.appearance)
                            LanguagePicker(id: language, label: tr! { "zh-Hans" => "语言", "en" => "Language" })
                            Row(gap: 12) {
                                Row {
                                    Text(content: app.alias, style: headline, lines: 1)
                                    Spacer
                                }
                                Button(id: random_alias, label: tr! { "zh-Hans" => "换个名字", "en" => "Random name" }, enabled: app.ready && !app.busy && !app.sharing) => app.randomize_alias()
                            }
                            Row(gap: 12) {
                                TextInput(id: alias, label: tr! { "zh-Hans" => "自定义名称", "en" => "Custom name" }, value: draft.alias, prompt: tr! { "zh-Hans" => "输入新名称", "en" => "Enter a new name" })
                                Button(id: save_alias, label: tr! { "zh-Hans" => "保存", "en" => "Save" }, enabled: app.ready && !app.busy && !app.sharing && draft.alias != "" && draft.alias != app.alias) => app.save_settings(draft.alias, app.destination, app.port, app.pin)
                            }
                        }
                        Section {
                            Row(gap: 12) {
                                Image(source: "bern/ui/assets/rustsend-512.png", width: 32, height: 32)
                                Column(gap: 4) {
                                    Text(content: tr! { "zh-Hans" => "RustSend 1.0.0", "en" => "RustSend 1.0.0" }, style: headline)
                                    Text(content: tr! { "zh-Hans" => "自由、开源的局域网文件共享", "en" => "Free, open-source local file sharing" }, style: caption, emphasis: secondary)
                                }
                                Spacer
                            }
                        }
                    }
                    SettingsPage::Receiving => {
                        Section {
                            Column(gap: 10) {
                                if app.mobile {
                                    Text(content: tr! { "zh-Hans" => "文件保存在 RustSend 内", "en" => "Files are saved in RustSend" }, style: caption, emphasis: secondary)
                                    Button(id: export_destination, label: tr! { "zh-Hans" => "导出接收文件…", "en" => "Export received files…" }, icon: "folder", enabled: !app.busy, pick: draft.destination, files: "folder") => app.export_destination(draft.destination)
                                } else {
                                    Text(content: app.destination, style: caption, lines: 2, emphasis: secondary)
                                    Button(id: destination, label: tr! { "zh-Hans" => "选择文件夹…", "en" => "Choose folder…" }, icon: "folder", enabled: !app.busy) => app.choose_destination()
                                }
                            }
                            Switch(id: quick_save, label: tr! { "zh-Hans" => "快速保存", "en" => "Quick save" }, checked: app.quick_save, style: switch) => app.set_quick_save(app.quick_save)
                            Switch(id: favorites_only, label: tr! { "zh-Hans" => "仅自动接受收藏设备", "en" => "Auto-accept favorites only" }, checked: app.favorites_only, style: switch) => app.set_favorites_only(app.favorites_only)
                        }
                    }
                    SettingsPage::Network => {
                        Section(footer: tr! { "zh-Hans" => "修改端口或 PIN 会重新启动接收服务。", "en" => "Changing the port or PIN restarts the receiver." }) {
                            Row(gap: 12) {
                                TextInput(id: port, label: tr! { "zh-Hans" => "端口", "en" => "Port" }, value: draft.port, prompt: app.port)
                                Button(id: save_port, label: tr! { "zh-Hans" => "保存", "en" => "Save" }, enabled: !app.busy && !app.sharing && draft.port != "") => app.save_settings(app.alias, app.destination, draft.port, app.pin)
                            }
                            Row(gap: 12) {
                                TextInput(id: pin, label: tr! { "zh-Hans" => "接收 PIN", "en" => "Receive PIN" }, value: draft.pin, kind: secure)
                                Button(id: save_pin, label: tr! { "zh-Hans" => "更新", "en" => "Update" }, enabled: !app.busy && !app.sharing) => app.save_settings(app.alias, app.destination, app.port, draft.pin)
                            }
                            if app.pin != "" { Text(content: tr! { "zh-Hans" => "已启用 PIN；留空更新可关闭。", "en" => "PIN enabled; save an empty PIN to disable." }, style: caption, emphasis: secondary) }
                        }
                    }
                }
            }
        }
    }
}
