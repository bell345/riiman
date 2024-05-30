use eframe::egui::Color32;
use std::sync::OnceLock;

static ACCENT_COLOR: OnceLock<Color32> = OnceLock::new();

pub fn get_accent_color() -> Color32 {
    *ACCENT_COLOR.get_or_init(|| {
        #[cfg(windows)]
        {
            use windows::UI::ViewManagement::{UIColorType, UISettings};
            let settings = UISettings::new().expect("create UISettings");
            let value = settings
                .GetColorValue(UIColorType::Accent)
                .expect("get color value");
            Color32::from_rgb(value.R, value.G, value.B)
        }

        #[cfg(not(windows))]
        {
            tracing::warn!(
                "Getting user preference color is not supported on your platform ({}). \
                Defaulting to #00FFFF.",
                std::env::consts::OS
            );
            Color32::from_rgba(0, 255, 255);
        }
    })
}

pub const ERROR_TEXT: Color32 = Color32::from_rgb(255, 80, 80);
pub const SUCCESS_TEXT: Color32 = Color32::from_rgb(80, 255, 80);
pub const PROGRESS_TEXT: Color32 = Color32::from_rgb(80, 160, 255);
pub const WHITE_TEXT: Color32 = Color32::from_rgb(240, 240, 240);
pub const BLACK_TEXT: Color32 = Color32::from_rgb(20, 20, 20);
