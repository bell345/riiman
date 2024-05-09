use crate::state::AppStateRef;
use crate::tasks::compute::{compute_thumbnails_grid, ThumbnailPosition};
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::{ThumbnailGridInfo, ThumbnailGridParams};
use crate::ui::item_cache::ItemCache;
use crate::ui::theme::get_accent_color;
use crate::ui::thumb_cache::{ThumbnailCache, ThumbnailCacheItem};
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use eframe::egui;
use poll_promise::Promise;
use std::ops::Add;
use std::path::Path;
use tracing::info;

const THUMBNAIL_CACHE_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const THUMBNAIL_LOAD_INTERVAL_MS: i64 = 50;
const THUMBNAIL_LQ_LOAD_INTERVAL_MS: i64 = 10;
const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;
const THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS: i64 = 1500;

const ROUNDING: egui::Rounding = egui::Rounding::same(4.0);
const HOVER_TINT: egui::Color32 = egui::Color32::from_rgba_premultiplied(255, 255, 255, 150);
const CHECKBOX_ALIGN: egui::Align2 = egui::Align2::RIGHT_TOP;
const CHECKBOX_SIZE: egui::Vec2 = egui::vec2(32.0, 32.0);
const CHECKBOX_PADDING: f32 = 0.0;
const CHECKBOX_INTERACT_SIZE: f32 = 16.0;
const HIGHLIGHT_PADDING: f32 = 2.0;

pub struct ThumbnailGrid {
    pub params: ThumbnailGridParams,
    info: ThumbnailGridInfo,

    cache: ThumbnailCache,
    lq_cache: ThumbnailCache,

    middle_item: Option<String>,
    scroll_cooldown: Option<DateTime<Utc>>,
    set_scroll: bool,
    last_vp: Option<egui::Rect>,
    hovering_item: Option<String>,
    checked_items: DashMap<String, bool>,
}

impl Default for ThumbnailGrid {
    fn default() -> Self {
        Self {
            params: Default::default(),
            info: Default::default(),
            cache: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LOAD_INTERVAL_MS),
                false,
            ),
            lq_cache: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LQ_LOAD_INTERVAL_MS),
                true,
            ),
            middle_item: Default::default(),
            scroll_cooldown: Default::default(),
            set_scroll: Default::default(),
            last_vp: Default::default(),
            hovering_item: Default::default(),
            checked_items: Default::default(),
        }
    }
}

impl ThumbnailGrid {
    pub fn commit(&mut self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        if params.height == crate::ui::THUMBNAIL_LOW_QUALITY_HEIGHT {
            self.lq_cache.commit(params.clone(), item.clone());
        }
        self.cache.commit(params, item);
    }

    fn resolve_thumbnail(&self, item: &ThumbnailPosition) -> ThumbnailCacheItem {
        let path: Box<Path> = Path::new(item.path.as_str()).into();
        let height = self.params.max_row_height as usize;

        let mut thumb = ThumbnailCacheItem::Loading;
        if height > THUMBNAIL_LOW_QUALITY_HEIGHT {
            thumb = self.cache.read(&ThumbnailParams {
                path: path.clone(),
                last_modified: item.last_modified,
                height,
            });
        }
        if thumb == ThumbnailCacheItem::Loading {
            thumb = self.lq_cache.read(&ThumbnailParams {
                path: path.clone(),
                last_modified: item.last_modified,
                height: THUMBNAIL_LOW_QUALITY_HEIGHT,
            });
        }

        thumb
    }

    pub fn view_selected_paths<R>(&mut self, f: impl FnOnce(Vec<&String>) -> R) -> R {
        let ro = std::mem::take(&mut self.checked_items).into_read_only();
        let res = f(ro.iter().filter(|(_, v)| **v).map(|(k, _)| k).collect());
        drop(std::mem::replace(&mut self.checked_items, ro.into_inner()));
        res
    }

    pub fn update(
        &mut self,
        ui: &mut egui::Ui,
        state: AppStateRef,
        item_cache: &ItemCache,
        item_cache_is_new: bool,
    ) -> anyhow::Result<Option<egui::scroll_area::ScrollAreaOutput<()>>> {
        let state = state.blocking_read();
        let current_vault = state.current_vault()?;

        self.params.container_width = ui.available_width();

        let thumbnail_grid_is_new = self.info.params != self.params;
        if item_cache_is_new || thumbnail_grid_is_new {
            self.set_scroll = true;
            ui.ctx().request_repaint();
            let params = self.params.clone();

            let items = item_cache.resolve_all_refs(&current_vault);

            self.info = compute_thumbnails_grid(params, &items)?;
        }

        if item_cache_is_new {
            let included_paths = item_cache.item_path_set();
            let mut to_remove = vec![];
            for item in self.checked_items.iter() {
                if !included_paths.contains(item.key()) {
                    to_remove.push(item.key().clone());
                }
            }
            for path in to_remove {
                self.checked_items.remove(&path);
            }
        }

        Ok(Some(
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .animated(false)
                .show_viewport(ui, |ui, vp| {
                    let grid = &self.info;
                    if grid.thumbnails.is_empty() {
                        return;
                    }

                    let abs_min = ui.min_rect().min.to_vec2();
                    let abs_vp = vp.translate(abs_min);
                    let vp_middle = (vp.min + vp.max.to_vec2()) / 2.0;
                    let vp_changed = self.last_vp != Some(vp);
                    let vp_scrolled = vp_changed
                        && (vp.size() - self.last_vp.map(|v| v.size()).unwrap_or(vp.size()))
                            .length_sq()
                            < 1.0;
                    let vp_resized = vp_changed && !vp_scrolled;
                    let max_y = grid.thumbnails.last().unwrap().outer_bounds.max.y;
                    ui.set_width(ui.available_width());
                    ui.set_height(max_y);
                    ui.set_clip_rect(abs_vp);

                    let middle_item = self.middle_item.as_ref();
                    let hover_item = self.hovering_item.as_ref();
                    let mut next_middle: Option<String> = None;
                    let mut next_hover: Option<String> = None;

                    for item in grid.thumbnails.iter() {
                        let outer_bounds = item.outer_bounds.translate(abs_min);
                        let inner_bounds = item.inner_bounds.translate(abs_min);
                        let text = egui::Label::new(item.path.clone());

                        let is_hover = |r: &egui::Response| {
                            if let Some(hover_pos) = r.hover_pos() {
                                if outer_bounds.contains(hover_pos) {
                                    return true;
                                }
                            }
                            false
                        };

                        // scroll to item if resize event has occurred
                        if (self.set_scroll || vp_resized) && Some(&item.path) == middle_item {
                            info!("do scroll to {} at {:?}", &item.path, &outer_bounds);
                            info!("set_scroll = {}, resized = {}", self.set_scroll, vp_resized);
                            ui.scroll_to_rect(outer_bounds, Some(egui::Align::Center));
                            self.set_scroll = false;
                            self.scroll_cooldown = Some(Utc::now().add(TimeDelta::milliseconds(
                                THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS,
                            )));
                        }
                        // mark current item as item to scroll to when resize occurs
                        else if self.scroll_cooldown.unwrap_or(Utc::now()) <= Utc::now()
                            && vp_scrolled
                            && next_middle.is_none()
                            && item.outer_bounds.contains(vp_middle)
                        {
                            next_middle = Some(item.path.clone());
                        }

                        if vp.intersects(item.outer_bounds) {
                            let thumb = self.resolve_thumbnail(item);

                            let ThumbnailCacheItem::Loaded(hndl) = thumb else {
                                ui.put(inner_bounds, text);
                                ui.put(inner_bounds, egui::Spinner::new());
                                continue;
                            };

                            let checked = {
                                let check_ref =
                                    self.checked_items.entry(item.path.clone()).or_default();
                                *check_ref.value()
                            };
                            if checked {
                                let highlight_tint = get_accent_color();
                                let highlight_rect = inner_bounds.expand(HIGHLIGHT_PADDING);
                                let shape = egui::epaint::RectShape::filled(
                                    highlight_rect,
                                    ROUNDING,
                                    highlight_tint,
                                );
                                ui.painter_at(highlight_rect).add(shape);
                            }

                            let img = egui::Image::new(egui::ImageSource::Texture(
                                egui::load::SizedTexture::from_handle(&hndl),
                            ))
                            .bg_fill(egui::Color32::from_gray(20))
                            .shrink_to_fit();

                            let tint = if Some(&item.path) == hover_item {
                                HOVER_TINT
                            } else {
                                egui::Color32::WHITE
                            };

                            let img_btn = egui::ImageButton::new(img)
                                .rounding(ROUNDING)
                                .tint(tint)
                                .frame(false);

                            let res = ui.put(inner_bounds, img_btn);
                            let is_clicked = res.clicked();
                            if is_clicked {
                                info!("Clicked {}!", item.path);
                            }
                            if is_hover(&res) {
                                next_hover = Some(item.path.clone());
                            }

                            ui.scope(|ui| {
                                ui.spacing_mut().interact_size =
                                    egui::Vec2::splat(CHECKBOX_INTERACT_SIZE);
                                let mut check_ref =
                                    self.checked_items.entry(item.path.clone()).or_default();
                                let checkbox = egui::Checkbox::new(check_ref.value_mut(), "");
                                let checkbox_rect = CHECKBOX_ALIGN
                                    .align_size_within_rect(CHECKBOX_SIZE, outer_bounds);
                                let res = ui.put(checkbox_rect, checkbox);
                                if is_hover(&res) {
                                    next_hover = Some(item.path.clone());
                                }
                                if !res.clicked() && is_clicked {
                                    *check_ref.value_mut() ^= true;
                                }
                            });
                        }
                    }

                    if next_middle.is_some() {
                        self.middle_item = next_middle;
                    }

                    self.hovering_item = next_hover;

                    self.last_vp = Some(vp);

                    for params in self.lq_cache.drain_requests() {
                        state.add_task(
                            format!("Load thumbnail for {}", params.path.display()),
                            Box::new(move |s, p| {
                                Promise::spawn_async(load_image_thumbnail_with_fs(s, p, params))
                            }),
                        );
                    }

                    for params in self.cache.drain_requests() {
                        state.add_task(
                            format!("Load thumbnail for {}", params.path.display()),
                            Box::new(move |s, p| {
                                Promise::spawn_async(load_image_thumbnail(s, p, params))
                            }),
                        );
                    }
                }),
        ))
    }
}
