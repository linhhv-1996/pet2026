use crate::WindowInfo;

pub fn fetch_windows() -> Vec<WindowInfo> {
    #[cfg(target_os = "macos")]
    {
        fetch_windows_macos()
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![]
    }
}

#[cfg(target_os = "macos")]
fn fetch_windows_macos() -> Vec<WindowInfo> {
    use std::os::raw::c_void;

    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFTypeRef = *const c_void;
    type CFIndex = isize;
    type CGWindowID = u32;

    const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
    const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;
    const kCGNullWindowID: CGWindowID = 0;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: CGWindowID) -> CFArrayRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(arr: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFStringRef) -> CFTypeRef;
        fn CFStringGetCStringPtr(s: CFStringRef, enc: u32) -> *const std::os::raw::c_char;
        fn CFNumberGetValue(number: CFTypeRef, theType: i32, valuePtr: *mut c_void) -> bool;
        fn CFRelease(cf: CFTypeRef);
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            cstr: *const std::os::raw::c_char,
            enc: u32,
        ) -> CFStringRef;
    }

    const kCFStringEncodingUTF8: u32 = 0x08000100;
    const K_CFNUMBER_FLOAT64_TYPE: i32 = 13;

    unsafe fn cf_string(s: &str) -> CFStringRef {
        let c = std::ffi::CString::new(s).unwrap();
        CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), kCFStringEncodingUTF8)
    }

    unsafe fn dict_string(dict: CFDictionaryRef, key: &str) -> Option<String> {
        let k = cf_string(key);
        let val = CFDictionaryGetValue(dict, k);
        CFRelease(k);
        if val.is_null() {
            return None;
        }
        let ptr = CFStringGetCStringPtr(val, kCFStringEncodingUTF8);
        if ptr.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }

    unsafe fn dict_f64(dict: CFDictionaryRef, key: &str) -> f64 {
        let k = cf_string(key);
        let val = CFDictionaryGetValue(dict, k);
        CFRelease(k);
        if val.is_null() {
            return 0.0;
        }
        let mut out: f64 = 0.0;
        CFNumberGetValue(val, K_CFNUMBER_FLOAT64_TYPE, &mut out as *mut f64 as *mut c_void);
        out
    }

    unsafe {
        let list = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        );
        if list.is_null() {
            return vec![];
        }

        let count = CFArrayGetCount(list);
        let mut result = Vec::new();

        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(list, i) as CFDictionaryRef;

            // Skip windows without a title
            let title = match dict_string(dict, "kCGWindowName") {
                Some(t) if !t.is_empty() => t,
                _ => continue,
            };

            // Get bounds sub-dictionary
            let bounds_key = cf_string("kCGWindowBounds");
            let bounds = CFDictionaryGetValue(dict, bounds_key) as CFDictionaryRef;
            CFRelease(bounds_key);

            let (x, y, width, height) = if !bounds.is_null() {
                (
                    dict_f64(bounds, "X"),
                    dict_f64(bounds, "Y"),
                    dict_f64(bounds, "Width"),
                    dict_f64(bounds, "Height"),
                )
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };

            result.push(WindowInfo {
                title,
                x,
                y,
                width,
                height,
            });
        }

        CFRelease(list);
        result
    }
}
