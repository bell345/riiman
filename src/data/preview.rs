use chrono::{DateTime, Utc};
use eframe::egui;
use eframe::egui::ViewportClass;
use std::fmt::{Debug, Formatter};
use std::ops::Deref;

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

#[derive(Default, Clone, Copy, Hash, PartialEq, Eq)]
pub struct DebugViewportClass(ViewportClass);

impl Debug for DebugViewportClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            ViewportClass::Root => write!(f, "ViewportClass::Root"),
            ViewportClass::Deferred => write!(f, "ViewportClass::Deferred"),
            ViewportClass::Immediate => write!(f, "ViewportClass::Immediate"),
            ViewportClass::Embedded => write!(f, "ViewportClass::Embedded"),
        }
    }
}

impl From<DebugViewportClass> for ViewportClass {
    fn from(value: DebugViewportClass) -> Self {
        value.0
    }
}

impl From<ViewportClass> for DebugViewportClass {
    fn from(value: ViewportClass) -> Self {
        Self(value)
    }
}

impl Deref for DebugViewportClass {
    type Target = ViewportClass;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
