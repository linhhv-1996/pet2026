#![allow(unused)]
mod windows;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    App, Manager, WebviewUrl, WebviewWindowBuilder,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default)]
struct PetRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub frog: String,
    pub focus_mins: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { frog: "Frog_1".into(), focus_mins: 25 }
    }
}

/// State của focus session — None = không có session nào đang chạy
#[derive(Debug, Clone, Default)]
struct FocusState {
    /// Thời điểm session kết thúc. None = không focus.
    end_at: Option<Instant>,
}

impl FocusState {
    fn start(&mut self, mins: u32) {
        self.end_at = Some(Instant::now() + Duration::from_secs(mins as u64 * 60));
    }
    fn stop(&mut self) {
        self.end_at = None;
    }
    fn is_active(&self) -> bool {
        self.end_at.is_some()
    }
    fn secs_left(&self) -> Option<u64> {
        self.end_at.map(|e| {
            let now = Instant::now();
            if e > now { (e - now).as_secs() } else { 0 }
        })
    }
}

const AFK_THRESHOLD_SECS: u64 = 45;

pub fn run() {
    let pet_rect    = Arc::new(Mutex::new(PetRect::default()));
    let last_input  = Arc::new(Mutex::new(Instant::now()));
    let settings    = Arc::new(Mutex::new(AppSettings::default()));
    let focus_state = Arc::new(Mutex::new(FocusState::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            update_pet_rect,
            get_windows,
            request_accessibility,
            set_frog,
            set_focus_mins,
            get_settings,
            toggle_pet,
            quit_app,
            start_focus,
            stop_focus,
            get_focus_state,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            setup_tray(app)?;
            setup_pet_window(app)?;

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Thread: push danh sách cửa sổ sang pet mỗi 500ms
            let handle = app.handle().clone();
            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(500));
                let wins = windows::fetch_windows();
                if let Some(w) = handle.get_webview_window("pet") {
                    if let Ok(json) = serde_json::to_string(&wins) {
                        let js = format!("window.__onWindowsUpdated({})", json);
                        let _ = w.eval(&js);
                    }
                }
            });

            // Thread: event tap (mouse/keyboard) — macOS only
            let handle2     = app.handle().clone();
            let rect2       = Arc::clone(&pet_rect);
            let last_input2 = Arc::clone(&last_input);
            thread::spawn(move || {
                #[cfg(target_os = "macos")]
                unsafe { run_event_tap(handle2, rect2, last_input2); }
            });

            // Thread: AFK watcher
            let handle3     = app.handle().clone();
            let last_input3 = Arc::clone(&last_input);
            thread::spawn(move || { afk_watcher(handle3, last_input3); });

            // Thread: Focus timer — tick mỗi giây, emit secs_left sang cả pet lẫn panel
            let handle4      = app.handle().clone();
            let focus_state4 = Arc::clone(&focus_state);
            thread::spawn(move || { focus_ticker(handle4, focus_state4); });

            app.manage(Arc::clone(&pet_rect));
            app.manage(Arc::clone(&settings));
            app.manage(Arc::clone(&focus_state));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ── Focus ticker ──────────────────────────────────────────────────────────────

fn focus_ticker(handle: tauri::AppHandle, focus_state: Arc<Mutex<FocusState>>) {
    loop {
        thread::sleep(Duration::from_secs(1));

        let secs = { focus_state.lock().unwrap().secs_left() };

        let Some(secs_left) = secs else { continue };

        // Push secs_left sang pet (hiện countdown trên đầu ếch)
        if let Some(w) = handle.get_webview_window("pet") {
            let js = format!("window.__onFocusTick && window.__onFocusTick({})", secs_left);
            let _ = w.eval(&js);
        }

        // Push secs_left sang panel (cập nhật countdown trong UI)
        if let Some(p) = handle.get_webview_window("panel") {
            let js = format!("window.__onFocusTick && window.__onFocusTick({})", secs_left);
            let _ = p.eval(&js);
        }

        // Hết giờ → kết thúc session
        if secs_left == 0 {
            focus_state.lock().unwrap().stop();

            if let Some(w) = handle.get_webview_window("pet") {
                let _ = w.eval("window.__onFocusEnd && window.__onFocusEnd(true)");
            }
            if let Some(p) = handle.get_webview_window("panel") {
                let _ = p.eval("window.__onFocusEnd && window.__onFocusEnd(true)");
            }
        }
    }
}

// ── Focus commands ────────────────────────────────────────────────────────────

/// Panel gọi khi nhấn Start — Rust bật timer, emit __onFocusStart sang pet
#[tauri::command]
fn start_focus(
    mins: u32,
    app: tauri::AppHandle,
    focus_state: tauri::State<Arc<Mutex<FocusState>>>,
) {
    focus_state.lock().unwrap().start(mins);

    if let Some(w) = app.get_webview_window("pet") {
        let js = format!("window.__onFocusStart && window.__onFocusStart({})", mins);
        let _ = w.eval(&js);
    }
}

/// Panel gọi khi nhấn Stop — Rust dừng timer, emit __onFocusEnd sang pet
#[tauri::command]
fn stop_focus(
    app: tauri::AppHandle,
    focus_state: tauri::State<Arc<Mutex<FocusState>>>,
) {
    focus_state.lock().unwrap().stop();

    if let Some(w) = app.get_webview_window("pet") {
        let _ = w.eval("window.__onFocusEnd && window.__onFocusEnd(false)");
    }
}

/// Panel gọi khi mount để khôi phục UI nếu app vẫn đang focus (panel bị đóng/mở lại)
#[tauri::command]
fn get_focus_state(
    focus_state: tauri::State<Arc<Mutex<FocusState>>>,
) -> Option<u64> {
    focus_state.lock().unwrap().secs_left()
}

// ─────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(settings: tauri::State<Arc<Mutex<AppSettings>>>) -> (String, u32) {
    let s = settings.lock().unwrap();
    (s.frog.clone(), s.focus_mins)
}

#[tauri::command]
fn set_frog(frog: String, settings: tauri::State<Arc<Mutex<AppSettings>>>, app: tauri::AppHandle) {
    let valid = ["Frog_1", "Frog_2", "Frog_3", "Frog_4"];
    if !valid.contains(&frog.as_str()) { return; }
    settings.lock().unwrap().frog = frog.clone();
    if let Some(w) = app.get_webview_window("pet") {
        let js = format!("window.__onFrogChanged && window.__onFrogChanged('{}')", frog);
        let _ = w.eval(&js);
    }
}

#[tauri::command]
fn set_focus_mins(mins: u32, settings: tauri::State<Arc<Mutex<AppSettings>>>, app: tauri::AppHandle) {
    settings.lock().unwrap().focus_mins = mins;
    if let Some(w) = app.get_webview_window("pet") {
        let js = format!("window.__onFocusChanged && window.__onFocusChanged({})", mins);
        let _ = w.eval(&js);
    }
}

fn afk_watcher(handle: tauri::AppHandle, last_input: Arc<Mutex<Instant>>) {
    let mut is_afk = false;
    loop {
        thread::sleep(Duration::from_secs(5));
        let elapsed = { let t = last_input.lock().unwrap(); t.elapsed() };
        if !is_afk && elapsed >= Duration::from_secs(AFK_THRESHOLD_SECS) {
            is_afk = true;
            if let Some(w) = handle.get_webview_window("pet") {
                let _ = w.eval("window.__onUserAFK && window.__onUserAFK()");
            }
        } else if is_afk && elapsed < Duration::from_secs(AFK_THRESHOLD_SECS) {
            is_afk = false;
            if let Some(w) = handle.get_webview_window("pet") {
                let _ = w.eval("window.__onUserActive && window.__onUserActive()");
            }
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn run_event_tap(
    handle: tauri::AppHandle,
    rect: Arc<Mutex<PetRect>>,
    last_input: Arc<Mutex<Instant>>,
) {
    use std::os::raw::c_void;
    type CFMachPortRef      = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef       = *mut c_void;
    type CGEventRef         = *mut c_void;
    type CGEventMask        = u64;
    type CGEventType        = u32;
    type CGEventTapLocation  = u32;
    type CGEventTapPlacement = u32;
    type CGEventTapOptions   = u32;

    const K_CG_EVENT_LEFT_MOUSE_DOWN:    CGEventType = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP:      CGEventType = 2;
    const K_CG_EVENT_LEFT_MOUSE_DRAGGED: CGEventType = 6;
    const K_CG_EVENT_MOUSE_MOVED:        CGEventType = 5;
    const K_CG_EVENT_KEY_DOWN:           CGEventType = 10;
    const K_CG_EVENT_SCROLL_WHEEL:       CGEventType = 22;

    const K_CG_SESSION_EVENT_TAP:        CGEventTapLocation  = 1;
    const K_CG_HEAD_INSERT_EVENT_TAP:    CGEventTapPlacement = 0;
    const K_CG_EVENT_TAP_OPTION_DEFAULT: CGEventTapOptions   = 0;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint { x: f64, y: f64 }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: CGEventTapLocation, place: CGEventTapPlacement, options: CGEventTapOptions,
            events_of_interest: CGEventMask,
            callback: extern "C" fn(*mut c_void, CGEventType, CGEventRef, *mut c_void) -> CGEventRef,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CFMachPortCreateRunLoopSource(alloc: *const c_void, tap: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    struct DragState { dragging: bool, offset_x: f64, offset_y: f64 }
    struct TapContext {
        rect: Arc<Mutex<PetRect>>,
        handle: tauri::AppHandle,
        last_input: Arc<Mutex<Instant>>,
        down_in_pet: Mutex<bool>,
        drag: Mutex<DragState>,
    }

    extern "C" fn event_callback(_: *mut c_void, event_type: CGEventType, event: CGEventRef, user_info: *mut c_void) -> CGEventRef {
        unsafe {
            let ctx = &*(user_info as *const TapContext);
            let loc = CGEventGetLocation(event);
            let is_pet_drag = *ctx.down_in_pet.lock().unwrap();

            if !is_pet_drag {
                *ctx.last_input.lock().unwrap() = Instant::now();
            }

            match event_type {
                K_CG_EVENT_LEFT_MOUSE_DOWN => {
                    let r = ctx.rect.lock().unwrap();
                    let in_pet = loc.x >= r.x && loc.x <= r.x + r.w && loc.y >= r.y && loc.y <= r.y + r.h;
                    *ctx.down_in_pet.lock().unwrap() = in_pet;

                    if in_pet {
                        let mut drag = ctx.drag.lock().unwrap();
                        drag.dragging = false; drag.offset_x = loc.x - r.x; drag.offset_y = loc.y - r.y;
                        drop(drag); drop(r);
                        return std::ptr::null_mut();
                    } else {
                        let mut consume_click = false;

                        if let Some(panel) = ctx.handle.get_webview_window("panel") {
                            if panel.is_visible().unwrap_or(false) {
                                if let (Ok(pos), Ok(size)) = (panel.outer_position(), panel.outer_size()) {
                                    let scale = panel.scale_factor().unwrap_or(1.0);
                                    let px = pos.x as f64 / scale;
                                    let py = pos.y as f64 / scale;
                                    let pw = size.width as f64 / scale;
                                    let ph = size.height as f64 / scale;

                                    let in_panel = loc.x >= px && loc.x <= px + pw
                                                && loc.y >= py && loc.y <= py + ph;

                                    if !in_panel && loc.y > 32.0 {
                                        consume_click = true;
                                    }
                                }
                            }
                        }

                        if consume_click {
                            let handle_clone = ctx.handle.clone();
                            let _ = ctx.handle.run_on_main_thread(move || {
                                if let Some(p) = handle_clone.get_webview_window("panel") {
                                    let _ = p.hide();
                                }
                            });
                            return std::ptr::null_mut();
                        }
                    }
                }
                K_CG_EVENT_LEFT_MOUSE_DRAGGED => {
                    let was_down_in = *ctx.down_in_pet.lock().unwrap();
                    if !was_down_in { return event; }
                    let mut drag = ctx.drag.lock().unwrap();
                    if !drag.dragging {
                        drag.dragging = true;
                        if let Some(w) = ctx.handle.get_webview_window("pet") { let _ = w.eval("window.__onPetDragStart()"); }
                    }
                    let new_x = loc.x - drag.offset_x; let new_y = loc.y - drag.offset_y;
                    drop(drag);
                    { let mut r = ctx.rect.lock().unwrap(); r.x = new_x; r.y = new_y; }
                    if let Some(w) = ctx.handle.get_webview_window("pet") {
                        let js = format!("window.__onPetDrag({:.1},{:.1})", new_x, new_y);
                        let _ = w.eval(&js);
                    }
                    return std::ptr::null_mut();
                }
                K_CG_EVENT_LEFT_MOUSE_UP => {
                    let was_down_in = *ctx.down_in_pet.lock().unwrap();
                    *ctx.down_in_pet.lock().unwrap() = false;
                    if was_down_in {
                        let dragging = ctx.drag.lock().unwrap().dragging;
                        if dragging {
                            ctx.drag.lock().unwrap().dragging = false;
                            if let Some(w) = ctx.handle.get_webview_window("pet") { let _ = w.eval("window.__onPetDragEnd()"); }
                        } else {
                            if let Some(w) = ctx.handle.get_webview_window("pet") { let _ = w.eval("window.__onPetClicked()"); }
                        }
                        return std::ptr::null_mut();
                    }
                }
                K_CG_EVENT_MOUSE_MOVED => {
                    if let Some(w) = ctx.handle.get_webview_window("pet") {
                        let js = format!("if(window.__onMouseMove) window.__onMouseMove({:.1},{:.1});", loc.x, loc.y);
                        let _ = w.eval(&js);
                    }
                }
                _ => {}
            }
            event
        }
    }

    let ctx = Box::new(TapContext {
        rect, handle, last_input, down_in_pet: Mutex::new(false),
        drag: Mutex::new(DragState { dragging: false, offset_x: 0.0, offset_y: 0.0 }),
    });
    let ctx_ptr = Box::into_raw(ctx) as *mut c_void;
    let mask: CGEventMask = (1 << K_CG_EVENT_LEFT_MOUSE_DOWN) | (1 << K_CG_EVENT_LEFT_MOUSE_UP)
                          | (1 << K_CG_EVENT_LEFT_MOUSE_DRAGGED) | (1 << K_CG_EVENT_MOUSE_MOVED)
                          | (1 << K_CG_EVENT_KEY_DOWN) | (1 << K_CG_EVENT_SCROLL_WHEEL);

    let tap = CGEventTapCreate(K_CG_SESSION_EVENT_TAP, K_CG_HEAD_INSERT_EVENT_TAP, 0, mask, event_callback, ctx_ptr);
    if tap.is_null() { return; }
    CGEventTapEnable(tap, true);
    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
    let rl = CFRunLoopGetCurrent();
    CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
    CFRunLoopRun();
}

#[tauri::command]
fn update_pet_rect(state: tauri::State<Arc<Mutex<PetRect>>>, x: f64, y: f64, w: f64, h: f64) {
    let mut r = state.lock().unwrap();
    r.x = x; r.y = y; r.w = w; r.h = h;
}

// =============================================
// Panel Logic
// =============================================

const PANEL_W: f64 = 300.0;
const PANEL_H: f64 = 430.0;

fn toggle_panel(app: &tauri::AppHandle, tray_x: f64, tray_y: f64) {
    let scale = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
    let px = (tray_x / scale - PANEL_W / 2.0).max(4.0);
    let py = tray_y / scale;

    if let Some(panel) = app.get_webview_window("panel") {
        if panel.is_visible().unwrap_or(false) {
            let _ = panel.hide();
        } else {
            let _ = panel.set_position(tauri::LogicalPosition::new(px, py));
            let _ = panel.show();
            let _ = panel.set_focus();
        }
        return;
    }

    if let Ok(panel) = WebviewWindowBuilder::new(app, "panel", WebviewUrl::App("/panel".into()))
        .title("")
        .inner_size(PANEL_W, PANEL_H)
        .position(px, py)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .shadow(false)
        .build()
    {
        let panel_clone = panel.clone();
        panel.on_window_event(move |event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = panel_clone.hide();
            }
        });

        let _ = panel.show();
        let _ = panel.set_focus();
    }
}

fn setup_tray(app: &mut App) -> tauri::Result<()> {
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&quit]).build()?;
    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .menu_on_left_click(false)
        .tooltip("Pet App")
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, position, .. } = event {
                toggle_panel(tray.app_handle(), position.x, position.y);
            }
        })
        .on_menu_event(|app, event| { if event.id.as_ref() == "quit" { app.exit(0); } })
        .build(app)?;
    Ok(())
}

fn setup_pet_window(app: &mut App) -> tauri::Result<()> {
    let (sw, sh) = if let Ok(Some(m)) = app.primary_monitor() {
        let s = m.scale_factor(); (m.size().width as f64 / s, m.size().height as f64 / s)
    } else { (1440.0, 900.0) };

    let window = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("/".into()))
        .title("").inner_size(sw, sh).position(0.0, 0.0).decorations(false).transparent(true)
        .always_on_top(true).resizable(false).shadow(false).visible_on_all_workspaces(true).build()?;
    window.show()?;
    let _ = window.set_ignore_cursor_events(true);

    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        if let Ok(ptr) = window.ns_window() {
            let ns_win = ptr as *mut AnyObject;
            let _: () = msg_send![ns_win, setLevel: 25_isize];
            let behavior: usize = 1 | 16 | 64 | 256;
            let _: () = msg_send![ns_win, setCollectionBehavior: behavior];
        }
    }
    Ok(())
}

#[tauri::command]
fn toggle_pet(app: tauri::AppHandle, visible: bool) {
    if let Some(w) = app.get_webview_window("pet") {
        if !visible {
            if let Some(state) = app.try_state::<Arc<Mutex<PetRect>>>() {
                let mut r = state.lock().unwrap(); r.x = -9999.0; r.y = -9999.0; r.w = 0.0; r.h = 0.0;
            }
            let _ = w.eval("window.__onPetHide && window.__onPetHide()");
        } else {
            let _ = w.eval("window.__onPetShow && window.__onPetShow()");
        }
    }
}

#[tauri::command] fn quit_app(app: tauri::AppHandle) { app.exit(0); }
#[tauri::command] fn get_windows() -> Vec<WindowInfo> { windows::fetch_windows() }
#[tauri::command]
fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::os::raw::c_void;
        type CFDictionaryRef = *const c_void; type CFStringRef = *const c_void; type CFTypeRef = *const c_void;
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFDictionaryCreateMutable(a: *const c_void, c: isize, kc: *const c_void, vc: *const c_void) -> CFDictionaryRef;
            fn CFDictionaryAddValue(d: CFDictionaryRef, k: CFTypeRef, v: CFTypeRef);
            fn CFRelease(cf: CFTypeRef); static kCFBooleanTrue: CFTypeRef;
        }
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(o: CFDictionaryRef) -> u8;
            static kAXTrustedCheckOptionPrompt: CFStringRef;
        }
        let dict = CFDictionaryCreateMutable(std::ptr::null(), 1, std::ptr::null(), std::ptr::null());
        CFDictionaryAddValue(dict, kAXTrustedCheckOptionPrompt, kCFBooleanTrue);
        let trusted = AXIsProcessTrustedWithOptions(dict) != 0;
        CFRelease(dict);
        trusted
    }
    #[cfg(not(target_os = "macos"))] false
}
