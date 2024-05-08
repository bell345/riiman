use crate::state::AppStateRef;
use crate::tasks::compute::compute_thumbnails_grid;
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::{ThumbnailGridInfo, ThumbnailGridParams};
use crate::ui::item_cache::ItemCache;
use crate::ui::thumb_cache::{ThumbnailCache, ThumbnailCacheItem};
use chrono::{DateTime, TimeDelta, Utc};
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

pub struct ThumbnailGrid {
    pub params: ThumbnailGridParams,
    info: ThumbnailGridInfo,

    cache: ThumbnailCache,
    lq_cache: ThumbnailCache,

    middle_item: Option<String>,
    scroll_cooldown: Option<DateTime<Utc>>,
    set_scroll: bool,
    last_vp: Option<egui::Rect>,
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

    pub fn update(
        &mut self,
        ui: &mut egui::Ui,
        state: AppStateRef,
        item_cache: &ItemCache,
        item_cache_is_new: bool,
    ) -> anyhow::Result<()> {
        let state = state.blocking_read();
        let current_vault = state.current_vault()?;

        self.params.container_width = ui.available_width();

        let thumbnail_grid_is_new = self.info.params != self.params;
        if item_cache_is_new || thumbnail_grid_is_new {
            self.set_scroll = true;
            ui.ctx().request_repaint();
            let params = self.params.clone();

            let items = item_cache.resolve_refs(&current_vault);

            self.info = compute_thumbnails_grid(params, &items)?;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show_viewport(ui, |ui, vp| {
                let grid = &self.info;
                if grid.thumbnails.is_empty() {
                    return;
                }

                const PADDING: f32 = 8.0;
                const ROUNDING: f32 = 4.0;

                let abs_min = ui.min_rect().min.to_vec2();
                let abs_vp = vp.translate(abs_min);
                let vp_middle = (vp.min + vp.max.to_vec2()) / 2.0;
                let vp_changed = self.last_vp != Some(vp);
                let vp_scrolled =
                    vp_changed && vp.size() == self.last_vp.map(|v| v.size()).unwrap_or(vp.size());
                let vp_resized = vp_changed && !vp_scrolled;
                let max_y = grid.thumbnails.last().unwrap().bounds.max.y;
                ui.set_width(ui.available_width());
                ui.set_height(max_y);
                ui.set_clip_rect(abs_vp);

                let middle_item = self.middle_item.as_ref();
                let mut next_middle: Option<String> = None;

                for item in grid.thumbnails.iter() {
                    let abs_bounds = item.bounds.translate(abs_min);
                    let text = egui::Label::new(item.path.clone());

                    // scroll to item if resize event has occurred
                    if (self.set_scroll || vp_resized) && Some(&item.path) == middle_item {
                        info!("do scroll to {} at {:?}", &item.path, &abs_bounds);
                        info!("set_scroll = {}, resized = {}", self.set_scroll, vp_resized);
                        ui.scroll_to_rect(abs_bounds, Some(egui::Align::Center));
                        self.set_scroll = false;
                        self.scroll_cooldown = Some(Utc::now().add(TimeDelta::milliseconds(
                            THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS,
                        )));
                    }
                    // mark current item as item to scroll to when resize occurs
                    else if self.scroll_cooldown.unwrap_or(Utc::now()) <= Utc::now()
                        && vp_scrolled
                        && next_middle.is_none()
                        && item.bounds.contains(vp_middle)
                    {
                        next_middle = Some(item.path.clone());
                    }

                    if vp.intersects(item.bounds) {
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

                        match thumb {
                            ThumbnailCacheItem::Loading => {
                                ui.put(abs_bounds.shrink(PADDING), text);
                                ui.put(abs_bounds, egui::Spinner::new());
                            }
                            ThumbnailCacheItem::Loaded(hndl) => {
                                egui::Image::new(egui::ImageSource::Texture(
                                    egui::load::SizedTexture::from_handle(&hndl),
                                ))
                                .rounding(egui::epaint::Rounding::from(ROUNDING))
                                .shrink_to_fit()
                                .paint_at(ui, abs_bounds.shrink(PADDING));
                            }
                        }
                    }
                }

                if next_middle.is_some() {
                    self.middle_item = next_middle;
                }

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
            });

        Ok(())
    }
}
