use chrono::{DateTime, Utc};
use eframe::egui;
use std::fmt::{Debug, Formatter};

#[derive(Clone)]
pub struct PreviewOptions {
    texture: Option<egui::TextureHandle>,
    pub cursor_position: Option<egui::Pos2>,
    pub lens_magnification: f32,
    pub lens_size: f32,
    pub fullscreen: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            texture: None,
            cursor_position: None,
            lens_magnification: 2.0,
            lens_size: 256.0,
            fullscreen: false,
        }
    }
}

impl PreviewOptions {
    pub fn texture_handle(&self) -> Option<egui::TextureHandle> {
        self.texture.clone()
    }

    pub fn set_texture(&mut self, texture: egui::TextureHandle) {
        self.texture = Some(texture);
    }

    pub fn clear(&mut self) {
        self.texture = None;
    }
}
