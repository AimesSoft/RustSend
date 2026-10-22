# 发布 RustSend

版本来源是 `Bern.toml` 的 `app.version`。`bern/engine`、`bern/protocol` 和
`bern/ui` 的 Cargo 包版本必须相同；发行标签使用 `v1.0.0`，更新内容保存到
`docs/releases/1.0.0.md`。本仓库固定了 Bern 源码和 Rust 工具链，CI 不依赖相邻的
Bern 工作目录。

## Android 签名

首次发布前，在仓库的 **Settings → Secrets and variables → Actions** 配置：

| Secret | 内容 |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | 持久发布 keystore 的单行 Base64 编码 |
| `ANDROID_KEYSTORE_PASSWORD` | keystore 密码 |
| `ANDROID_KEY_ALIAS` | 私钥别名 |
| `ANDROID_KEY_PASSWORD` | 可选；未设置时使用 keystore 密码 |

私钥和密码保存在仓库外，并做离线备份。以后所有版本都使用同一把钥匙，保证
Android 用户能够覆盖升级。工作流仅把它解码到 runner 临时目录，结束时删除。
公开仓库和 Actions artifact 中不得放入 keystore 或密码。

发布运行缺少签名配置会失败。手动运行且不勾选 `publish` 时，允许使用临时
debug 证书做编译验证，产物名会明确带上 `-debug`，不会进入 GitHub Release。

## 构建和发布

1. 更新 `Bern.toml`、三个应用 crate 的版本，以及 `docs/releases/<版本>.md`。
2. 提交变更。需要先验证所有平台时，在 **Actions → Release → Run workflow**
   运行，保持 `publish` 未勾选。五个平台产物出现在该运行的 Artifacts 中。
3. 对要发布的提交创建并推送 `v<版本>` 标签。标签触发全平台构建、格式检查、
   Clippy、应用测试和 Bern UI 检查。
4. 全部成功后，工作流核对完整包集合，生成 `SHA256SUMS.txt`，上传到草稿
   Release，最后公开发布。任一平台失败都不会公开一个缺包的 Release。

也可以在手动运行时勾选 `publish`，但所选提交必须已经有对应版本标签。
已公开的 Release 不会被工作流覆盖，重复发布会明确报错。如果上传阶段中断，
重新运行发布 job 会自动恢复同标签草稿、重新上传资产，并核对服务端文件大小与
SHA-256（API 提供时）；无需重新构建已经成功的 matrix jobs。草稿里出现本次
发行之外的文件时会停止，保留它们供人工检查。

## 产物和平台要求

以 `1.0.0` 为例：

| 平台 | 文件 | 说明 |
| --- | --- | --- |
| macOS | `RustSend-1.0.0-macOS-arm64.zip` | Apple Silicon，应用有 ad-hoc 签名，尚未做 Apple 公证 |
| Linux | `RustSend-1.0.0-Linux-x64.tar.gz`、`.deb` | GTK ≥ 4.14、libadwaita ≥ 1.5、WebKitGTK 6；Ubuntu 24.04 推荐 `.deb` |
| Windows | `RustSend-1.0.0-Windows-x64.zip`、`RustSend-1.0.0-Windows-x64-setup.exe` | WinUI 3；Windows App Runtime 2.5；尚未做 Authenticode 签名 |
| Android | `RustSend-1.0.0-Android-arm64.apk`、`.dm` | Android 8.0+；APK 用持久发布密钥签名；`.dm` 是可选的 AOT 编译元数据 |
| iOS | `RustSend-1.0.0-iOS-arm64-unsigned.ipa` | iOS 17+ 真机 unsigned 包，需要自己的签名和配置描述文件才能安装 |

iOS 打包通过 `BERN_IOS_DEVICE=1` 选择真机 SDK。UDP 组播发现还需要 Apple
批准的 multicast entitlement；重签名者应按自己的 provisioning 配置启用。
未获得该授权时可通过手动 IP 连接。这个 unsigned IPA 不是 App Store 已签名包。

Android 的 `.dm` 可与同名 APK 一起通过 `adb install-multiple -r` 安装到
Android 12+；直接安装 APK 也能使用，系统会在后台完成编译优化。

每个运行器都从 `vendor/bern` 构建 CLI：

```sh
cargo build --locked --manifest-path vendor/bern/Cargo.toml -p bern-cli
```

Linux 和 Windows 使用 `bern package --target <平台>`；其他平台使用
`bern build --target <平台> --release`，随后由 `scripts/release.py collect`
统一发行包命名。iOS 包明确带有 `-unsigned` 后缀。
