use tauri::WebviewWindow;

pub fn apply_native_surface(window: &WebviewWindow, radius: f64) -> String {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{
            apply_liquid_glass, apply_vibrancy, LiquidGlassOptions, NSGlassEffectViewStyle,
            NSVisualEffectMaterial,
        };

        let glass_opts = LiquidGlassOptions::new(NSGlassEffectViewStyle::Clear)
            .radius(radius)
            .interactive(true);

        if apply_liquid_glass(window, glass_opts).is_ok() {
            return "liquid-glass".to_string();
        }

        if apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(radius)).is_ok() {
            return "vibrancy".to_string();
        }
    }

    let _ = (window, radius);
    "css-fallback".to_string()
}
