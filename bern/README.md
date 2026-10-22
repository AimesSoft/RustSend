# RustSend 开发指南

应用介绍与下载见[仓库首页](../README.md)，1.0 的发行包、签名与平台限制见[发布说明](../docs/releases/1.0.0.md)。本目录包含 RustSend 的 UI 声明、协议与业务引擎；网络协议复用 `packages/core/` 中的 LocalSend Rust 核心。

## 构建与运行

以下命令在仓库根目录执行。工具链由 `rust-toolchain.toml` 固定，Bern 源码已包含在 `vendor/bern/`，不需要另外检出框架。

```sh
cargo build --manifest-path vendor/bern/Cargo.toml -p bern-cli --release
vendor/bern/target/release/bern check
vendor/bern/target/release/bern build --target macos --release
open target/bern/macos/RustSend.app
```

在对应主机上将目标改为 `windows`、`linux`、`android` 或 `ios`；Windows CLI 路径为 `vendor\bern\target\release\bern.exe`。平台开发包与完整 CI 命令见 [Release workflow](../.github/workflows/release.yml)。使用 Release 构建验证体积和性能，调试符号保留在应用包外。

Android 构建与连接设备运行：

```sh
rustup target add aarch64-linux-android
vendor/bern/target/release/bern build --target android --release
vendor/bern/target/release/bern run --target android --release
```

`run` 要求一台已授权 USB 调试的真机或模拟器，安装并启动后建立 IPC 转发。Android 产物及生成工程位于 `target/bern/android/`。

iOS 真机构建：

```sh
rustup target add aarch64-apple-ios
BERN_IOS_DEVICE=1 \
BERN_IOS_LOCAL_NETWORK_DESCRIPTION='RustSend uses the local network to discover devices and transfer files.' \
vendor/bern/target/release/bern build --target ios --release
```

生成的 `target/bern/ios/RustSend.app` 可由发布流程打包为未签名 IPA；安装前需要自己的签名证书与描述文件。iOS 组播发现需要 Apple 批准的组播权限，未授权时可手动输入 IP。iOS 真机验收尚未完成。

`Bern.toml` 中的内部应用键为 `rust_send`，平台应用 ID 为 `com.aimessoft.rustsend`。生成的命令行程序为 `rust-send`，业务命名空间为 `rustsend`。例如在 macOS 应用运行后：

```sh
target/bern/macos/bin/rust-send --help
target/bern/macos/bin/rust-send ui inspect
```

## 代码结构

| 路径 | Crate／职责 |
| --- | --- |
| `protocol/` | `rustsend-protocol`：状态、类型与命令契约 |
| `engine/` | `rustsend-engine`：`RustSendEngine`、Tokio 工作线程、传输、设置与历史 |
| `ui/` | `rustsend-ui`：原生界面声明、翻译、紧凑布局与资源 |
| `engine/tests/transfers.rs` | 独立引擎实例之间的真实 HTTPS 集成测试 |
| `../packages/core/` | 上游 LocalSend v2 协议、HTTPS 与组播发现 |
| `../vendor/bern/` | 固定的 Bern 框架源码与必要修复 |

引擎通过类型化命令处理 UI 操作。网络和磁盘任务在后台运行，状态变化通过 Bern waker 唤醒界面。Bern 将 Rust UI 声明编译为各平台代码；SwiftUI、WinUI 3、GTK 4 与 Jetpack Compose 控件由各自原生后端提供。

`packages/core` 的默认 Cargo features 为空。应用依赖必须继续启用 `features = ["full"]`，单独检查或测试核心时也必须使用 `--features full`：

```sh
cargo test -p localsend --features full
```

Bern 来源与本地修复范围见 [vendor/bern/UPSTREAM.md](../vendor/bern/UPSTREAM.md)。升级框架时显式更新源码快照，不依赖移动分支。

## 文件与持久化

桌面配置、设备证书、收藏与历史继续保存在系统配置目录的 `localsend-bern/`，兼容早期开发版。首次接收目录为下载目录下的 `RustSend/`；已有配置的接收目录保持不变。设备名只在首次缺失时生成并保存，一键换名保留证书与指纹，并重新广播名称。

`RUSTSEND_DATA_DIR` 可指定独立数据目录，优先级高于兼容变量 `LOCALSEND_BERN_DATA_DIR`。隔离测试可在配置中设置 `discovery: false`，改用手动地址连接。旧主题色字段由反序列化忽略；系统／浅色／深色外观继续持久保存。

移动端通过 Bern 宿主提供的应用私有数据目录保存配置和身份，完成文件放在 `Received/` 下，导入使用应用缓存目录。接收暂存文件位于 `Received/` 外，避免导出尚未完成的内容。1.0 的 Android 应用 ID 与早期开发包不同，二者私有数据独立，不会自动迁移。

Android 文件选择器使用 Storage Access Framework 授权；发送目录会递归导入私有缓存，单文件通过 `MediaSource` 读取。已接收单文件通过 FileProvider 向系统打开操作授予读取权限。导出通过用户选择的 SAF 文件夹进行，保留应用内原文件，同名目标编号避让。多文件历史使用导出全部入口，不尝试将应用私有目录作为外部文件管理器路径打开。

接收文件在长度、SHA-256 校验与写入完成后才报告成功，同名文件使用编号而不覆盖已有文件。拒绝路径穿越和目标目录中的符号链接。Android 使用 `renameat2(RENAME_NOREPLACE)` 原子发布；其他平台使用硬链接，接收目标文件系统须支持硬链接。

移动端传输需要保持应用在前台。Android 已启用网络和组播权限，前台持有 Wi-Fi 组播锁，Activity 停止时释放；持续后台传输服务尚未接入。系统选择器授权后的 Android 文件／目录导入、剪贴板与 SAF 导出仍需完整端到端真机验收。

## 界面约定

- 默认桌面窗口为 440 × 560，最小为 360 × 440；按可用空间调整内容，常用操作保持在视口内。
- 桌面使用常驻分段导航，移动端使用底部导航；切页更新内容，保持窗口与导航稳定。
- 设置按通用／接收／网络分类，采用原生 `Form`、`Section` 与标准控件。配色为黑白灰，Logo 固定为 Rust 橙色 `#CE422B`。
- 接收请求列表可滚动，接收／拒绝按钮固定在底部。完成提示使用应用内独立浮层，不占用正文布局空间。
- 静态 Logo 使用 PNG；接收页保留一个 SVG 动画实例，每 15 秒旋转一圈。暂停接收、页面不可见或系统减少动态效果时停止动画。SVG 由 Bern 平台适配显示；Linux 需要 WebKitGTK 6，Windows 使用 WebView2。

重新生成 Logo PNG（需要 `rsvg-convert`）：

```sh
rsvg-convert -w 512 -h 512 bern/ui/assets/localsend.svg -o bern/ui/assets/rustsend-512.png
rsvg-convert -w 256 -h 256 bern/ui/assets/localsend.svg -o bern/ui/assets/rustsend-256.png
```

## 检查与测试

```sh
cargo fmt -p rustsend-engine -p rustsend-protocol -p rustsend-ui --check
cargo clippy -p rustsend-engine -p rustsend-protocol -p rustsend-ui --all-targets --no-deps -- -D warnings
cargo test -p rustsend-engine -- --test-threads=1
vendor/bern/target/release/bern check
```

格式检查仅针对应用 crate，避免重排上游和框架快照。集成测试使用临时数据目录、本机随机端口和关闭的组播发现，覆盖文件夹与空文件、Unicode、同名冲突、部分接收、文字、取消／拒绝、PIN、损坏校验和、路径穿越、浏览器分享审批、设置持久化及改名后身份不变。文件发布单元测试覆盖并发同名接收、发布失败、符号链接和目录碰撞。

UI 声明检查、原生构建与实机验证分别记录；`bern check` 通过不等于各平台运行验收完成。现有实机覆盖与待验证功能见[首页验证范围](../README.md#验证范围)，发布构建结果见对应 GitHub Actions 运行。

RustSend 与 LocalSend 核心使用 [Apache-2.0](../LICENSE)，Bern 使用 [MIT](../vendor/bern/LICENSE)。来源、版本及版权声明见 [NOTICE](../NOTICE)。
