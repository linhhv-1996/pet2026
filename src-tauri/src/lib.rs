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

            // Thread 2: CGEventTap — intercept + consume clicks trên vùng pet
            // Đây là cách duy nhất để "ăn" click không cho macOS xử lý
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
// CGEventTap: intercept mouse events ở system level
// Nếu click nằm trong pet rect → consume (không forward)
//                               → eval __onPetClicked()
// Nếu không → forward bình thường
// =============================================
#[cfg(target_os = "macos")]
unsafe fn run_event_tap(
    handle: tauri::AppHandle,
    rect: Arc<Mutex<PetRect>>,
) {
    use std::os::raw::c_void;

    type CFMachPortRef   = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFRunLoopRef    = *mut c_void;
    type CGEventRef      = *mut c_void;
    type CGEventMask     = u64;
    type CGEventType     = u32;
    type CGEventTapLocation  = u32;
    type CGEventTapPlacement = u32;
    type CGEventTapOptions   = u32;

    // Event types
    const K_CG_EVENT_LEFT_MOUSE_DOWN: CGEventType = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP:   CGEventType = 2;

    // kCGHeadInsertEventTap = 0 — intercept TRƯỚC khi hệ thống xử lý
    const K_CG_HEAD_INSERT_EVENT_TAP: CGEventTapPlacement = 0;
    // kCGSessionEventTap = 1
    const K_CG_SESSION_EVENT_TAP: CGEventTapLocation = 1;
    // kCGEventTapOptionDefault = 0 — có thể modify/drop event
    const K_CG_EVENT_TAP_OPTION_DEFAULT: CGEventTapOptions = 0;

    #[repr(C)]
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

        fn CFMachPortCreateRunLoopSource(
            alloc: *const c_void,
            tap: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;

        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: *const c_void;
    }

    // UserInfo truyền vào callback
    struct TapContext {
        rect:   Arc<Mutex<PetRect>>,
        handle: tauri::AppHandle,
        // Track mouse down position để detect click (down+up cùng vùng)
        down_in_pet: Mutex<bool>,
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
            let r   = ctx.rect.lock().unwrap();

            let in_pet = loc.x >= r.x && loc.x <= r.x + r.w
                      && loc.y >= r.y && loc.y <= r.y + r.h;
            drop(r);

            match event_type {
                K_CG_EVENT_LEFT_MOUSE_DOWN => {
                    *ctx.down_in_pet.lock().unwrap() = in_pet;
                    if in_pet {
                        println!("[PET] 🖱️  tap DOWN in pet — consuming");
                        // Trả về null = drop event, macOS không nhận
                        return std::ptr::null_mut();
                    }
                }
                K_CG_EVENT_LEFT_MOUSE_UP => {
                    let was_down_in = *ctx.down_in_pet.lock().unwrap();
                    *ctx.down_in_pet.lock().unwrap() = false;
                    if was_down_in && in_pet {
                        println!("[PET] ✅ tap UP in pet — fire click, consuming");
                        if let Some(w) = ctx.handle.get_webview_window("pet") {
                            let _ = w.eval("window.__onPetClicked()");
                        }
                        return std::ptr::null_mut();
                    }
                }
                _ => {}
            }

            // Forward event bình thường
            event
        }
    }

    // Box context lên heap, leak để giữ lifetime suốt app
    let ctx = Box::new(TapContext {
        rect:        rect,
        handle:      handle,
        down_in_pet: Mutex::new(false),
    });
    let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

    // Mask: chỉ listen LEFT_MOUSE_DOWN và LEFT_MOUSE_UP
    let mask: CGEventMask = (1 << K_CG_EVENT_LEFT_MOUSE_DOWN)
                          | (1 << K_CG_EVENT_LEFT_MOUSE_UP);

    let tap = CGEventTapCreate(
        K_CG_SESSION_EVENT_TAP,
        K_CG_HEAD_INSERT_EVENT_TAP,
        K_CG_EVENT_TAP_OPTION_DEFAULT,
        mask,
        event_callback,
        ctx_ptr,
    );

    if tap.is_null() {
        println!("[PET] ❌ CGEventTapCreate failed — cần quyền Accessibility!");
        println!("[PET]    Vào System Settings → Privacy → Accessibility → bật app");
        return;
    }

    println!("[PET] ✅ CGEventTap created successfully");
    CGEventTapEnable(tap, true);

    let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
    let rl = CFRunLoopGetCurrent();
    CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);

    println!("[PET] 🔄 CGEventTap run loop running...");
    CFRunLoopRun(); // Block thread này, chạy event loop
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

    let window = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("/".into()))
        .title("")
        .inner_size(sw, sh)
        .position(0.0, 0.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .resizable(false)
        .shadow(false)
        .visible_on_all_workspaces(true)
        .build()?;

    window.show()?;
    let _ = window.set_ignore_cursor_events(true);
    println!("[PET] ✅ window ready, ignore_cursor=true");
    Ok(())
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
