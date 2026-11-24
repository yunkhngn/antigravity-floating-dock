use tauri::WebviewWindow;

#[cfg(target_os = "macos")]
fn configure_macos_window_mask_and_focus(window: &WebviewWindow, radius: f64) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use std::ffi::c_void;

    // Suppress rectangular key-window shadow/border and clip content/webview layers to match the rounded shell.
    if let Ok(ns_win_ptr) = window.ns_window() {
        let ns_win = ns_win_ptr as *mut AnyObject;
        if !ns_win.is_null() {
            unsafe {
                let clear_cls = objc2::runtime::AnyClass::get(c"NSColor");
                if let Some(cls) = clear_cls {
                    let clear_color: *mut AnyObject = msg_send![cls, clearColor];
                    let _: () = msg_send![ns_win, setOpaque: false];
                    let _: () = msg_send![ns_win, setBackgroundColor: clear_color];
                }
                let _: () = msg_send![ns_win, setHasShadow: false];
            }
        }
    }

    let apply_view_clip = move |view_ptr: *mut c_void| {
        let view = view_ptr as *mut AnyObject;
        if view.is_null() {
            return;
        }
        unsafe {
            // NSFocusRingTypeNone = 1
            let _: () = msg_send![view, setFocusRingType: 1usize];
            let _: () = msg_send![view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
                let _: () = msg_send![layer, setBorderWidth: 0.0f64];
            }
        }
    };

    if let Ok(ns_view_ptr) = window.ns_view() {
        apply_view_clip(ns_view_ptr);
    }

    let _ = window.with_webview(move |webview| {
        apply_view_clip(webview.inner());
    });
}

pub fn apply_native_surface(window: &WebviewWindow, radius: f64, preferred_style: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{
            apply_liquid_glass, apply_vibrancy, clear_liquid_glass, clear_vibrancy,
            LiquidGlassOptions, NSGlassEffectViewStyle, NSVisualEffectMaterial,
        };

        let _ = clear_liquid_glass(window);
        let _ = clear_vibrancy(window);
        configure_macos_window_mask_and_focus(window, radius);

        let normalized = preferred_style.trim().to_ascii_lowercase();
        if normalized == "vibrancy" {
            if apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(radius)).is_ok()
            {
                return "vibrancy".to_string();
            }
        } else {
            let style = if normalized == "clear" {
                NSGlassEffectViewStyle::Clear
            } else {
                NSGlassEffectViewStyle::Regular
            };
            let glass_opts = LiquidGlassOptions::new(style)
                .radius(radius)
                .interactive(true);

            if apply_liquid_glass(window, glass_opts).is_ok() {
                return if normalized == "clear" {
                    "liquid-glass-clear".to_string()
                } else {
                    "liquid-glass".to_string()
                };
            }

            if apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(radius)).is_ok()
            {
                return "vibrancy".to_string();
            }
        }
    }

    let _ = (window, radius, preferred_style);
    "css-fallback".to_string()
}
