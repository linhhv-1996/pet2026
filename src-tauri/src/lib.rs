mod windows;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
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

pub fn run() {
    let pet_rect = Arc::new(Mutex::new(PetRect::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            update_pet_rect,
            get_windows,
            request_accessibility
        ])
        .setup(move |app| {
            setup_tray(app)?;
            setup_pet_window(app)?;

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Thread 1: poll windows mỗi 500ms
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

            // Thread 2: CGEventTap — handle click + drag
            let handle2 = app.handle().clone();
            let rect2 = Arc::clone(&pet_rect);
            thread::spawn(move || {
                #[cfg(target_os = "macos")]
                unsafe {
                    run_event_tap(handle2, rect2);
                }
            });

            app.manage(Arc::clone(&pet_rect));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// =============================================
// CGEventTap
// =============================================
#[cfg(target_os = "macos")]
unsafe fn run_event_tap(
    handle: tauri::AppHandle,
    rect: Arc<Mutex<PetRect>>,
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

    const K_CG_HEAD_INSERT_EVENT_TAP:    CGEventTapPlacement = 0;
    const K_CG_SESSION_EVENT_TAP:        CGEventTapLocation  = 1;
    const K_CG_EVENT_TAP_OPTION_DEFAULT: CGEventTapOptions   = 0;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint { x: f64, y: f64 }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: CGEventTapLocation,
            place: CGEventTapPlacement,
            options: CGEventTapOptions,
            events_of_interest: CGEventMask,
            callback: extern "C" fn(
                proxy: *mut c_void,
                event_type: CGEventType,
                event: CGEventRef,
                user_info: *mut c_void,
            ) -> CGEventRef,
            user_info: *mut c_void,
        ) -> CFMachPortRef;

        fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CFMachPortCreateRunLoopSource(
            alloc: *const c_void,
            tap: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    // State của drag
    struct DragState {
        // Đang drag pet không
        dragging: bool,
        // Offset từ góc trái pet đến điểm grab
        offset_x: f64,
        offset_y: f64,
    }

    struct TapContext {
        rect:       Arc<Mutex<PetRect>>,
        handle:     tauri::AppHandle,
        down_in_pet: Mutex<bool>,
        drag:       Mutex<DragState>,
    }

    extern "C" fn event_callback(
        _proxy: *mut c_void,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        unsafe {
            let ctx = &*(user_info as *const TapContext);
            let loc = CGEventGetLocation(event);

            match event_type {
                // ── MOUSE DOWN ──────────────────────────────────
                K_CG_EVENT_LEFT_MOUSE_DOWN => {
                    let r = ctx.rect.lock().unwrap();
                    let in_pet = loc.x >= r.x && loc.x <= r.x + r.w
                              && loc.y >= r.y && loc.y <= r.y + r.h;

                    *ctx.down_in_pet.lock().unwrap() = in_pet;

                    if in_pet {
                        // Lưu offset để drag mượt (grab point relative to pet corner)
                        let mut drag = ctx.drag.lock().unwrap();
                        drag.dragging = false; // chưa drag, chờ xem có move không
                        drag.offset_x = loc.x - r.x;
                        drag.offset_y = loc.y - r.y;
                        drop(drag);
                        drop(r);

                        println!("[PET] 🖱️  DOWN in pet at ({:.0},{:.0})", loc.x, loc.y);
                        return std::ptr::null_mut(); // consume
                    }
                }

                // ── MOUSE DRAGGED ────────────────────────────────
                K_CG_EVENT_LEFT_MOUSE_DRAGGED => {
                    let was_down_in = *ctx.down_in_pet.lock().unwrap();
                    if !was_down_in { return event; }

                    let mut drag = ctx.drag.lock().unwrap();

                    // Lần đầu move → bắt đầu drag
                    if !drag.dragging {
                        drag.dragging = true;
                        // Báo frontend bắt đầu drag (tạm dừng AI behavior)
                        if let Some(w) = ctx.handle.get_webview_window("pet") {
                            let _ = w.eval("window.__onPetDragStart()");
                        }
                    }

                    // Tính vị trí pet mới: cursor - offset
                    let new_x = loc.x - drag.offset_x;
                    let new_y = loc.y - drag.offset_y;
                    drop(drag);

                    // Cập nhật rect trong Rust
                    {
                        let mut r = ctx.rect.lock().unwrap();
                        r.x = new_x;
                        r.y = new_y;
                    }

                    // Cập nhật vị trí trên frontend
                    if let Some(w) = ctx.handle.get_webview_window("pet") {
                        let js = format!(
                            "window.__onPetDrag({:.1},{:.1})",
                            new_x, new_y
                        );
                        let _ = w.eval(&js);
                    }

                    return std::ptr::null_mut(); // consume drag event
                }

                // ── MOUSE UP ─────────────────────────────────────
                K_CG_EVENT_LEFT_MOUSE_UP => {
                    let was_down_in = *ctx.down_in_pet.lock().unwrap();
                    *ctx.down_in_pet.lock().unwrap() = false;

                    if !was_down_in { return event; }

                    let mut drag = ctx.drag.lock().unwrap();
                    let was_dragging = drag.dragging;
                    drag.dragging = false;
                    drop(drag);

                    if was_dragging {
                        // Thả ra sau drag → báo frontend kết thúc drag
                        println!("[PET] 🖱️  drag END at ({:.0},{:.0})", loc.x, loc.y);
                        if let Some(w) = ctx.handle.get_webview_window("pet") {
                            let _ = w.eval("window.__onPetDragEnd()");
                        }
                    } else {
                        // Không drag → là click thuần
                        println!("[PET] ✅ click in pet");
                        if let Some(w) = ctx.handle.get_webview_window("pet") {
                            let _ = w.eval("window.__onPetClicked()");
                        }
                    }

                    return std::ptr::null_mut(); // consume
                }

                _ => {}
            }

            event // forward
        }
    }

    let ctx = Box::new(TapContext {
        rect:        rect,
        handle:      handle,
        down_in_pet: Mutex::new(false),
        drag: Mutex::new(DragState {
            dragging: false,
            offset_x: 0.0,
            offset_y: 0.0,
        }),
    });
    let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

    let mask: CGEventMask = (1 << K_CG_EVENT_LEFT_MOUSE_DOWN)
                          | (1 << K_CG_EVENT_LEFT_MOUSE_UP)
                          | (1 << K_CG_EVENT_LEFT_MOUSE_DRAGGED);

    let tap = CGEventTapCreate(
        K_CG_SESSION_EVENT_TAP,
        K_CG_HEAD_INSERT_EVENT_TAP,
        K_CG_EVENT_TAP_OPTION_DEFAULT,
        mask,
        event_callback,
        ctx_ptr,
    );

    if tap.is_null() {
        println!("[PET] ❌ CGEventTapCreate failed — cần Accessibility permission!");
        return;
    }

    println!("[PET] ✅ CGEventTap ready (click + drag)");
    CGEventTapEnable(tap, true);

    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
    let rl = CFRunLoopGetCurrent();
    CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
    CFRunLoopRun();
}

#[tauri::command]
fn update_pet_rect(
    state: tauri::State<Arc<Mutex<PetRect>>>,
    x: f64, y: f64, w: f64, h: f64,
) {
    let mut r = state.lock().unwrap();
    let changed = (r.x - x).abs() > 1.0 || (r.y - y).abs() > 1.0;
    if changed {
        println!("[PET] 📦 rect => ({:.0},{:.0} {}x{})", x, y, w as i32, h as i32);
    }
    r.x = x; r.y = y; r.w = w; r.h = h;
}

fn setup_tray(app: &mut App) -> tauri::Result<()> {
    let quit   = MenuItemBuilder::with_id("quit",   "Quit").build(app)?;
    let toggle = MenuItemBuilder::with_id("toggle", "Show/Hide").build(app)?;
    let menu   = MenuBuilder::new(app).items(&[&toggle, &quit]).build()?;
    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Pet App")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit"   => app.exit(0),
            "toggle" => {
                if let Some(w) = app.get_webview_window("pet") {
                    if w.is_visible().unwrap_or(false) { let _ = w.hide(); }
                    else                               { let _ = w.show(); }
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn setup_pet_window(app: &mut App) -> tauri::Result<()> {
    let (sw, sh) = if let Ok(Some(m)) = app.primary_monitor() {
        let s = m.scale_factor();
        println!("[PET] monitor scale={} logical={}x{}",
            s, m.size().width as f64 / s, m.size().height as f64 / s);
        (m.size().width as f64 / s, m.size().height as f64 / s)
    } else {
        println!("[PET] no monitor, fallback 1440x900");
        (1440.0, 900.0)
    };

    // Bỏ always_on_top — sẽ set level thủ công bên dưới
    let window = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("/".into()))
        .title("")
        .inner_size(sw, sh)
        .position(0.0, 0.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(false)
        .resizable(false)
        .shadow(false)
        .visible_on_all_workspaces(true)
        .build()?;

    window.show()?;
    let _ = window.set_ignore_cursor_events(true);

    // Set NSWindowLevel cao hơn fullscreen
    #[cfg(target_os = "macos")]
    unsafe {
        set_window_above_fullscreen(&window);
    }

    println!("[PET] ✅ window ready (above fullscreen)");
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe fn set_window_above_fullscreen(window: &tauri::WebviewWindow) {
    use std::ffi::c_void;

    // Lấy NSWindow raw pointer
    let ns_win = match window.ns_window() {
        Ok(ptr) => ptr as *mut c_void,
        Err(e) => {
            println!("[PET] ❌ ns_window() failed: {:?}", e);
            return;
        }
    };

    // NSWindowLevel constants:
    // NSNormalWindowLevel      =  0
    // NSFloatingWindowLevel    =  3   (floating panels)
    // NSSubmenuWindowLevel     =  3
    // NSTornOffMenuWindowLevel =  3
    // NSModalPanelWindowLevel  =  8
    // NSMainMenuWindowLevel    = 24
    // NSStatusWindowLevel      = 25
    // NSPopUpMenuWindowLevel   = 101
    // NSScreenSaverWindowLevel = 1000  ← đè lên fullscreen app
    //
    // Fullscreen app chạy ở level ~500-999 tuỳ macOS version
    // Dùng 1000 (kCGScreenSaverWindowLevel) là safe nhất

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        // id objc_msgSend(id self, SEL op, ...)
        fn objc_msgSend(receiver: *mut c_void, sel: *const c_void, ...) -> *mut c_void;
        fn sel_registerName(name: *const i8) -> *const c_void;
    }

    let sel_set_level = {
        let name = b"setLevel:\0";
        sel_registerName(name.as_ptr() as *const i8)
    };
    let sel_set_collection_behavior = {
        let name = b"setCollectionBehavior:\0";
        sel_registerName(name.as_ptr() as *const i8)
    };

    // setLevel: 1000 (kCGScreenSaverWindowLevel) — trên fullscreen
    objc_msgSend(ns_win, sel_set_level, 1000i64);

    // NSWindowCollectionBehavior flags:
    // NSWindowCollectionBehaviorCanJoinAllSpaces    = 1 << 0  (hiện ở mọi Space)
    // NSWindowCollectionBehaviorStationary          = 1 << 4  (không bị Exposé ẩn)
    // NSWindowCollectionBehaviorIgnoresCycle        = 1 << 6  (không xuất hiện khi Cmd+Tab)
    // NSWindowCollectionBehaviorFullScreenAuxiliary = 1 << 8  (hiện khi app khác fullscreen)
    //
    // Combine: CanJoinAllSpaces | Stationary | IgnoresCycle | FullScreenAuxiliary
    let behavior: u64 = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
    objc_msgSend(ns_win, sel_set_collection_behavior, behavior);

    println!("[PET] 🪟 NSWindowLevel=1000, CollectionBehavior={:#b}", behavior);
}

#[tauri::command]
fn get_windows() -> Vec<WindowInfo> {
    windows::fetch_windows()
}

#[tauri::command]
fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::os::raw::c_void;
        type CFDictionaryRef = *const c_void;
        type CFStringRef     = *const c_void;
        type CFTypeRef       = *const c_void;
        type Boolean         = u8;

        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFDictionaryCreateMutable(
                alloc: *const c_void, cap: isize,
                kc: *const c_void, vc: *const c_void,
            ) -> CFDictionaryRef;
            fn CFDictionaryAddValue(dict: CFDictionaryRef, key: CFTypeRef, val: CFTypeRef);
            fn CFRelease(cf: CFTypeRef);
            static kCFBooleanTrue: CFTypeRef;
        }
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
            static kAXTrustedCheckOptionPrompt: CFStringRef;
        }

        let dict = CFDictionaryCreateMutable(
            std::ptr::null(), 1, std::ptr::null(), std::ptr::null(),
        );
        CFDictionaryAddValue(dict, kAXTrustedCheckOptionPrompt, kCFBooleanTrue);
        let trusted = AXIsProcessTrustedWithOptions(dict) != 0;
        CFRelease(dict);
        println!("[PET] accessibility trusted={}", trusted);
        trusted
    }
    #[cfg(not(target_os = "macos"))]
    false
}
