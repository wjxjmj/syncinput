# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

局域网内电脑和手机之间的实时同步输入框。Rust 单文件二进制（1.9MB），双击即用，窗口始终置顶。文字和图片实时双向同步。

## 构建

```bash
cargo build --release
# 产物: target/release/syncinput.exe
```

## 架构

- **`src/main.rs`** — axum WebSocket 服务器（端口 5200）+ tao/wry 桌面 WebView 壳
  - `AppState` 持有当前内容 + 客户端列表，broadcast 时排除发送者
  - tokio runtime 在后台线程跑 axum，主线程跑 tao 事件循环
  - `GET /` 返回编译时嵌入的 index.html，`GET /ws` 升级 WebSocket
- **`templates/index.html`** — contenteditable div，原生 WebSocket 客户端
  - 200ms debounce 后全量同步 innerHTML，syncing flag 防回声
  - 图片粘贴/拖入转 base64 插入，断线 2s 自动重连

## 运行

双击 `target/release/syncinput.exe`，或 `cargo run --release`。

手机连同一 WiFi 后浏览器访问 `http://<本机IP>:5200`。

Windows 首次使用需放行防火墙端口：
```
netsh advfirewall firewall add rule name="SyncInput" dir=in action=allow protocol=TCP localport=5200 enable=yes
```

## Android 悬浮球

`android/` 目录下是 Kotlin 项目，用 Android Studio 打开即可编译。

- **MainActivity** — 请求悬浮窗权限，启动 FloatingService
- **FloatingService** — 悬浮球 + WebView 面板，可拖拽，点击展开/收起
- WebView 加载 `http://<电脑IP>:5200`

首次使用需修改 `FloatingService.kt` 中的服务器 IP 地址。
