use eframe::egui;
use eframe::egui::{pos2, vec2, ViewportClass, ViewportId};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::data::PreviewOptions;
use crate::state::AppStateRef;
use crate::take_shortcut;
use crate::ui::AppModal;

pub struct Preview {
    id: egui::Id,
    texture: egui::TextureHandle,
    viewport_class: ViewportClass,
    options: PreviewOptions,
    is_open: Arc<AtomicBool>,
}

impl Preview {
    pub fn widget(id_source: impl std::hash::Hash, texture: egui::TextureHandle) -> Self {
        Self {
            id: egui::Id::new(id_source),
            texture,
            options: Default::default(),
            viewport_class: ViewportClass::Embedded,
            is_open: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn window(id_source: impl std::hash::Hash, texture: egui::TextureHandle) -> Self {
        Self {
            id: egui::Id::new(id_source),
            texture,
            options: Default::default(),
            viewport_class: ViewportClass::Deferred,
            is_open: Arc::new(AtomicBool::new(true)),
        }
    }

    fn contents(&mut self, viewport_id: ViewportId, ui: &mut egui::Ui) {
        let PreviewOptions {
            cursor_position,
            lens_magnification,
            lens_size,
            ..
        } = self.options;

        let hndl = self.texture.clone();

        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                let img = egui::Image::from_texture(egui::load::SizedTexture::from_handle(&hndl))
                    .bg_fill(egui::Color32::from_gray(20))
                    .shrink_to_fit();

                let res = ui.add(img);

                let img_ratio = hndl.aspect_ratio();
                let win_size = res.rect.size();

                let img_size = if res.rect.aspect_ratio() >= img_ratio {
                    vec2(win_size.y * img_ratio, win_size.y)
                } else {
                    vec2(win_size.x, win_size.x / img_ratio)
                };

                let img_pos = if res.rect.aspect_ratio() >= img_ratio {
                    pos2((win_size.x - img_size.x) / 2.0, 0.0)
                } else {
                    pos2(0.0, (win_size.y - img_size.y) / 2.0)
                };

                if let Some(cursor_pos) = cursor_position {
                    let cur_uv = ((cursor_pos - img_pos) / img_size).to_pos2();
                    let size = egui::Vec2::splat(lens_size);
                    let size_uv = size / lens_magnification / img_size;

                    let lens_img =
                        egui::Image::from_texture(egui::load::SizedTexture::from_handle(&hndl))
                            .uv(egui::Rect::from_min_size(cur_uv - size_uv / 2.0, size_uv))
                            .fit_to_original_size(lens_magnification)
                            .max_size(size)
                            .maintain_aspect_ratio(false)
                            .rounding(egui::Rounding::same(lens_size))
                            .bg_fill(egui::Color32::from_gray(20));

                    ui.put(
                        egui::Rect::from_min_size(cursor_pos - size / 2.0, size),
                        lens_img,
                    );
                }

                let opts = &mut self.options;

                if ui.ui_contains_pointer() && ui.input(|i| i.pointer.primary_down()) {
                    opts.cursor_position = ui.input(|i| i.pointer.latest_pos());
                } else {
                    opts.cursor_position = None;
                }

                let double_clicked = ui.ui_contains_pointer()
                    && ui.input(|i| {
                        i.pointer
                            .button_double_clicked(egui::PointerButton::Primary)
                    });

                if take_shortcut!(ui, F11) || double_clicked {
                    opts.fullscreen ^= true;
                    ui.ctx().send_viewport_cmd_to(
                        viewport_id,
                        egui::ViewportCommand::Fullscreen(opts.fullscreen),
                    );
                }
            },
        );
    }
}

impl AppModal for Arc<RwLock<Preview>> {
    fn id(&self) -> egui::Id {
        self.read().unwrap().id
    }

    fn update(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        let (id, viewport_class, is_open) = {
            let r = self.read().unwrap();
            (r.id, r.viewport_class, Arc::clone(&r.is_open))
        };

        let min_size = vec2(600.0, 400.0);
        let vp_id = ViewportId::from_hash_of(id);
        let builder = egui::ViewportBuilder::default()
            .with_title("Preview")
            .with_min_inner_size(min_size);

        match viewport_class {
            ViewportClass::Root => panic!("Preview window is not allowed to be a root window"),
            ViewportClass::Deferred => {
                let this = Arc::clone(self);
                ctx.show_viewport_deferred(vp_id, builder, move |ctx, cls| {
                    assert!(
                        cls == ViewportClass::Deferred,
                        "This egui backend doesn't support multiple viewports"
                    );

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            this.write().unwrap().contents(vp_id, ui);
                        });

                    if ctx.input(|i| i.viewport().close_requested()) {
                        is_open.store(false, Ordering::Relaxed);
                    }
                });
            }
            ViewportClass::Immediate => {
                ctx.show_viewport_immediate(vp_id, builder, |ctx, _cls| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            self.write().unwrap().contents(vp_id, ui);
                        });

                    if ctx.input(|i| i.viewport().close_requested()) {
                        is_open.store(false, Ordering::Relaxed);
                    }
                });
            }
            ViewportClass::Embedded => {
                let mut is_open_var = is_open.load(Ordering::Relaxed);

                egui::Window::new("Preview")
                    .id(self.id())
                    .frame(egui::Frame::none())
                    .min_size(min_size)
                    .open(&mut is_open_var)
                    .show(ctx, |ui| {
                        self.write().unwrap().contents(vp_id, ui);
                    });

                is_open.store(is_open_var, Ordering::Relaxed);
            }
        }
    }

    fn is_open(&self) -> bool {
        self.read().unwrap().is_open.load(Ordering::Relaxed)
    }
}
