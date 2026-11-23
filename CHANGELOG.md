# 更新记录 / Changelog

## 1.1.0 — 2026-11-23

升级至 **Bern 0.3.2**：彻底移除仓库内 `vendor/bern/` 源码快照，直接通过 crates.io 依赖 `bern = "=0.3.2"` 与官方 `bern` 0.3.2 CLI 构建；启用 Bern 原生 `[app].local_network = true` 与 `bern package --target ios` 设备打包，完整更新内容见 [RustSend 1.1.0 更新内容](docs/releases/1.1.0.md)。

接收页旋转的 Logo 改为位图，由系统的合成器转动，不再为它启动网页视图：Linux 包不再依赖 WebKitGTK 6，Windows 不再为它创建 WebView2。转速不变，每 15 秒一圈；暂停接收时停在当时的角度，系统减少动态效果时不转。

Upgraded to **Bern 0.3.2**: removed the vendored `vendor/bern/` snapshot in favor of `bern = "=0.3.2"` from crates.io and the official `bern` 0.3.2 CLI, using Bern's native `[app].local_network = true` and `bern package --target ios`. The spinning logo on the Receive page is now a bitmap turned by the system compositor instead of an animated SVG in a web view: the Linux package no longer depends on WebKitGTK 6 and Windows no longer creates a WebView2 for it. It still turns once every 15 seconds, stops where it is while receiving is paused, and stays still with reduced motion.

## 1.0.0 — 2026-10-23

RustSend 首个公开版本。使用 Bern 以 Rust 实现原生应用界面与业务逻辑，兼容 LocalSend v2，提供文件与文字互传、紧凑布局、黑白灰原生控件、应用内完成提示和昆虫／甲壳类随机设备名。

首次发布 macOS、Windows、Linux、Android 与 iOS 构建流程，应用 ID 为 `com.aimessoft.rustsend`，遵循 Apache-2.0 开源协议。

完整功能、发行包与安装说明见 [RustSend 1.0 更新内容](docs/releases/1.0.0.md)。

First public release: a Rust application built with Bern's native UI backends, compatible with LocalSend v2. Includes file and text transfers, compact layouts, monochrome platform controls, in-app completion overlays, insect/crustacean device names, and release builds for five platforms.
