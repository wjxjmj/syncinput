use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tao::dpi::LogicalSize;
use tao::event::Event;
use tao::event::WindowEvent;
use tao::event_loop::{ControlFlow, EventLoop};
use tao::platform::windows::WindowExtWindows;
use tao::window::WindowBuilder;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::RwLock;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon as TrayIcon, MouseButton, TrayIconBuilder, TrayIconEvent};
use muda::ContextMenu;
use arboard::Clipboard;

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    theme: String,
    font_size: u32,
    font_family: String,
    auto_copy: bool,
    copy_delay: u32,
    width: u32,
    height: u32,
    auto_start: bool,
    close_to_tray: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: "light".into(),
            font_size: 16,
            font_family: String::new(),
            auto_copy: false,
            copy_delay: 2,
            width: 400,
            height: 600,
            auto_start: false,
            close_to_tray: false,
        }
    }
}

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let dir = base.join("syncinput");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

fn load_config() -> Config {
    let path = config_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<Config>(&data) {
            return cfg;
        }
    }
    Config::default()
}

fn save_config(cfg: &Config) {
    if let Ok(data) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(config_path(), data);
    }
}

fn startup_lnk_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join(r"Microsoft\Windows\Start Menu\Programs\Startup\SyncInput.lnk")
}

fn set_auto_start(enable: bool) {
    let path = startup_lnk_path();
    if enable {
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(lnk) = mslnk::ShellLink::new(exe) {
                let _ = lnk.create_lnk(&path);
            }
        }
    } else {
        let _ = std::fs::remove_file(&path);
    }
}

struct Client {
    id: u64,
    tx: UnboundedSender<String>,
}

struct AppState {
    content: RwLock<String>,
    clients: RwLock<Vec<Client>>,
    next_id: AtomicU64,
}

impl AppState {
    async fn broadcast_content(&self, sender_id: u64, text: &str) {
        let msg = json!({"type": "content", "data": text}).to_string();
        let clients = self.clients.read().await;
        for c in clients.iter().filter(|c| c.id != sender_id) {
            let _ = c.tx.send(msg.clone());
        }
    }

    async fn broadcast_all(&self, msg: String) {
        let clients = self.clients.read().await;
        for c in clients.iter() {
            let _ = c.tx.send(msg.clone());
        }
    }

    async fn notify_clients_count(&self) {
        let count = self.clients.read().await.len();
        let msg = json!({"type": "clients", "count": count}).to_string();
        self.broadcast_all(msg).await;
    }

    async fn add_client(&self, tx: UnboundedSender<String>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.clients.write().await.push(Client { id, tx });
        self.notify_clients_count().await;
        id
    }

    async fn remove_client(&self, id: u64) {
        self.clients.write().await.retain(|c| c.id != id);
        self.notify_clients_count().await;
    }
}

async fn moon_handler() -> impl IntoResponse {
    let bytes = include_bytes!("../iconfinder-weather-weather-forecast-moon-night-sky-3859141_121229.png");
    ([(header::CONTENT_TYPE, "image/png")], bytes.as_slice())
}

async fn sun_handler() -> impl IntoResponse {
    let bytes = include_bytes!("../iconfinder-weather-weather-forecast-hot-sun-day-3859136_121222.png");
    ([(header::CONTENT_TYPE, "image/png")], bytes.as_slice())
}

async fn qr_handler() -> impl IntoResponse {
    let url = format!("http://{}:5200", local_ip());
    let code = QrCode::new(url.as_bytes()).unwrap();
    let svg = code.render::<svg::Color>().build();
    ([(header::CONTENT_TYPE, "image/svg+xml")], svg)
}

async fn index() -> impl IntoResponse {
    let ip = local_ip();
    let config = load_config();
    let html = include_str!("../templates/index.html")
        .replace("__SYNCINPUT_IP__", &format!("{}:5200", ip))
        .replace(
            "__SYNCINPUT_CONFIG__",
            &serde_json::to_string(&config).unwrap(),
        );
    Html(html)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let content = state.content.read().await.clone();
    let init = json!({"type": "content", "data": content}).to_string();
    let _ = socket.send(Message::Text(init.into())).await;

    let (tx, mut rx) = unbounded_channel::<String>();
    let my_id = state.add_client(tx).await;

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(data) = parsed["data"].as_str() {
                                *state.content.write().await = data.to_string();
                                state.broadcast_content(my_id, data).await;
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
            msg = rx.recv() => {
                match msg {
                    Some(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    state.remove_client(my_id).await;
}

async fn run_server() {
    let ws_state = Arc::new(AppState {
        content: RwLock::new(String::new()),
        clients: RwLock::new(Vec::new()),
        next_id: AtomicU64::new(0),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/qr", get(qr_handler))
        .route("/moon.png", get(moon_handler))
        .route("/sun.png", get(sun_handler))
        .merge(
            Router::new()
                .route("/ws", get(ws_handler))
                .with_state(ws_state),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5200").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn wait_for_port() {
    let addr = "127.0.0.1:5200".parse().unwrap();
    for _ in 0..30 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr().map(|a| a.ip().to_string())
        })
        .unwrap_or_else(|_| "0.0.0.0".into())
}

fn main() {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(run_server());
    });

    wait_for_port();

    let cfg = load_config();

    let (icon_rgba, icon_w, icon_h) = {
        let bytes = include_bytes!("../arrow_arrows_direction_rotate_sync_icon_193421.png");
        let img = image::load_from_memory(bytes)
            .expect("failed to load icon")
            .to_rgba8();
        let (w, h) = img.dimensions();
        (img.into_raw(), w, h)
    };

    let window_icon = tao::window::Icon::from_rgba(icon_rgba.clone(), icon_w, icon_h).ok();
    let tray_icon = TrayIcon::from_rgba(icon_rgba.clone(), icon_w, icon_h).unwrap();

    let normal_rgba = Arc::new(icon_rgba);

    let tray = Arc::new(
        TrayIconBuilder::new()
            .with_tooltip("SyncInput")
            .with_icon(tray_icon)
            .build()
            .unwrap(),
    );

    let tray_evt_rx = TrayIconEvent::receiver();
    let muda_menu_rx = MenuEvent::receiver();

    let flash_active = Arc::new(AtomicBool::new(false));
    let last_copied = Arc::new(std::sync::Mutex::new(String::new()));

    let close_to_tray = Arc::new(AtomicBool::new(cfg.close_to_tray));
    let ctt = close_to_tray.clone();

    let event_loop = EventLoop::new();
    let window = Arc::new(
        WindowBuilder::new()
            .with_decorations(false)
            .with_always_on_top(true)
            .with_window_icon(window_icon)
            .with_inner_size(LogicalSize::new(cfg.width as f64, cfg.height as f64))
            .build(&event_loop)
            .unwrap(),
    );

    let pinned = Arc::new(AtomicBool::new(true));
    let p = pinned.clone();
    let w = window.clone();
    let fa = flash_active.clone();
    let lc = last_copied.clone();
    let _webview = wry::WebViewBuilder::new()
        .with_url("http://127.0.0.1:5200")
        .with_devtools(true)
        .with_ipc_handler(move |req| {
            match req.body().as_str() {
                "pin" => {
                    let was = p.fetch_xor(true, Ordering::Relaxed);
                    w.set_always_on_top(!was);
                }
                "min" => w.set_minimized(true),
                "max" => {
                    let m = w.is_maximized();
                    w.set_maximized(!m);
                }
                "close" => {
                    if ctt.load(Ordering::Relaxed) {
                        w.set_visible(false);
                    } else {
                        std::process::exit(0);
                    }
                }
                s if s.starts_with("copy:") => {
                    let text = s.strip_prefix("copy:").unwrap_or("");
                    if let Ok(mut cb) = Clipboard::new() {
                        let _ = cb.set_text(text);
                    }
                    *lc.lock().unwrap() = text.to_string();
                    if !w.is_visible() {
                        fa.store(true, Ordering::Relaxed);
                    }
                }
                s if s.starts_with("resize,") => {
                    if let Some(dims) = s.strip_prefix("resize,") {
                        if let Some((ww, hh)) = dims.split_once('x') {
                            if let (Ok(ww), Ok(hh)) = (ww.parse::<u32>(), hh.parse::<u32>()) {
                                let _ = w.set_inner_size(LogicalSize::new(ww as f64, hh as f64));
                            }
                        }
                    }
                }
                s if s.starts_with("cfg:") => {
                    if let Some(kv) = s.strip_prefix("cfg:") {
                        if let Some((k, v)) = kv.split_once(':') {
                            let mut cfg = load_config();
                            match k {
                                "theme" => cfg.theme = v.to_string(),
                                "fontSize" => {
                                    if let Ok(n) = v.parse::<u32>() { cfg.font_size = n; }
                                }
                                "fontFamily" => cfg.font_family = v.to_string(),
                                "autoCopy" => cfg.auto_copy = v == "true",
                                "copyDelay" => {
                                    if let Ok(n) = v.parse::<u32>() { cfg.copy_delay = n; }
                                }
                                "autoStart" => {
                                    cfg.auto_start = v == "true";
                                    set_auto_start(cfg.auto_start);
                                }
                                "closeToTray" => {
                                    cfg.close_to_tray = v == "true";
                                    ctt.store(cfg.close_to_tray, Ordering::Relaxed);
                                }
                                _ => {}
                            }
                            save_config(&cfg);
                        }
                    }
                }
                _ => {}
            }
        })
        .build(window.as_ref())
        .unwrap();

    let last_resize_save = std::cell::RefCell::new(Instant::now());
    let last_flash = std::cell::RefCell::new(Instant::now());
    let last_clip_check = std::cell::RefCell::new(Instant::now());
    let w2 = window.clone();
    let tr = tray.clone();
    let nr = normal_rgba.clone();
    let lc = last_copied.clone();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50));

        // flash tray icon by showing/hiding (like WeChat)
        if flash_active.load(Ordering::Relaxed) {
            let now = Instant::now();

            // check clipboard: stop flash if content changed (user copied something else)
            if now.duration_since(*last_clip_check.borrow()) > Duration::from_millis(800) {
                *last_clip_check.borrow_mut() = now;
                if let Ok(mut cb) = Clipboard::new() {
                    if let Ok(current) = cb.get_text() {
                        if current != *lc.lock().unwrap() {
                            flash_active.store(false, Ordering::Relaxed);
                            let _ = tr.set_icon(Some(TrayIcon::from_rgba((*nr).clone(), icon_w, icon_h).unwrap()));
                        }
                    }
                }
            }

            if now.duration_since(*last_flash.borrow()) > Duration::from_millis(400) {
                *last_flash.borrow_mut() = now;
                static mut SHOW: bool = false;
                if unsafe { SHOW } {
                    let _ = tr.set_icon(Some(TrayIcon::from_rgba((*nr).clone(), icon_w, icon_h).unwrap()));
                } else {
                    let _ = tr.set_icon(None);
                }
                unsafe { SHOW = !SHOW; }
            }
        }

        // tray clicks
        if let Ok(evt) = tray_evt_rx.try_recv() {
            match evt {
                TrayIconEvent::Click { button: MouseButton::Left, .. } => {
                    w2.set_visible(true);
                    w2.set_focus();
                    flash_active.store(false, Ordering::Relaxed);
                    let _ = tr.set_icon(Some(TrayIcon::from_rgba((*nr).clone(), icon_w, icon_h).unwrap()));
                }
                TrayIconEvent::Click { button: MouseButton::Right, .. } => {
                    let menu = Menu::new();
                    let _ = menu.append_items(&[
                        &MenuItem::with_id(MenuId::new("restore"), "显示窗口", true, None),
                        &MenuItem::with_id(MenuId::new("quit"), "退出", true, None),
                    ]);
                    unsafe { let _ = menu.show_context_menu_for_hwnd(w2.hwnd(), None); }
                }
                _ => {}
            }
        }

        // handle muda menu events from the right-click popup
        if let Ok(ev) = muda_menu_rx.try_recv() {
            match ev.id.0.as_str() {
                "restore" => {
                    w2.set_visible(true);
                    w2.set_focus();
                    flash_active.store(false, Ordering::Relaxed);
                    let _ = tr.set_icon(Some(TrayIcon::from_rgba((*nr).clone(), icon_w, icon_h).unwrap()));
                }
                "quit" => std::process::exit(0),
                _ => {}
            }
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if close_to_tray.load(Ordering::Relaxed) {
                    w2.set_visible(false);
                } else {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(true),
                ..
            } => {
                // stop flashing when window is focused
                flash_active.store(false, Ordering::Relaxed);
                let _ = tr.set_icon(Some(TrayIcon::from_rgba((*nr).clone(), icon_w, icon_h).unwrap()));
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                let now = Instant::now();
                if now.duration_since(*last_resize_save.borrow()) > Duration::from_millis(500) {
                    *last_resize_save.borrow_mut() = now;
                    let mut cfg = load_config();
                    cfg.width = size.width as u32;
                    cfg.height = size.height as u32;
                    save_config(&cfg);
                }
            }
            _ => {}
        }
    });
}
