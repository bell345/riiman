use crate::shortcut;
use crate::state::AppStateRef;
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::thumb_grid::{compute, ThumbnailPosition};
use crate::tasks::{ThumbnailGridInfo, ThumbnailGridParams};
use crate::ui::item_cache::ItemCache;
use crate::ui::theme::get_accent_color;
use crate::ui::thumb_cache::{ThumbnailCache, ThumbnailCacheItem};
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use eframe::egui;
use itertools::Itertools;
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
const CHECKBOX_INTERACT_SIZE: f32 = 16.0;
const HIGHLIGHT_PADDING: f32 = 2.0;

pub struct ThumbnailGrid {
    pub params: ThumbnailGridParams,
    info: ThumbnailGridInfo,

    cache: ThumbnailCache,
    lq_cache: ThumbnailCache,

    middle_item: Option<String>,
    scroll_cooldown: Option<DateTime<Utc>>,
    has_focus: bool,
    set_scroll: bool,
    last_vp: Option<egui::Rect>,
    hovering_item: Option<String>,
    pressing_item: Option<String>,
    checked_items: DashMap<String, bool>,

    pub double_clicked: bool,
    pub select_mode: SelectMode,
}

#[derive(Default, PartialEq, Eq)]
pub enum SelectMode {
    #[default]
    Single,
    Multiple,
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
            has_focus: Default::default(),
            set_scroll: Default::default(),
            last_vp: Default::default(),
            hovering_item: Default::default(),
            pressing_item: Default::default(),
            checked_items: Default::default(),
            double_clicked: Default::default(),
            select_mode: Default::default(),
        }
    }
}

#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_possible_wrap)]
fn wrap_index(i: usize, len: usize, delta: isize) -> usize {
    ((((i as isize) + delta) + len as isize) % (len as isize)) as usize
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
        #[allow(clippy::cast_possible_truncation)]
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

    pub fn get_selected_paths(&self) -> Vec<String> {
        self.checked_items
            .iter()
            .filter_map(|r| {
                if *r.value() {
                    Some(r.key().clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn set_selected_paths(&mut self, paths: &[String]) {
        self.checked_items.clear();
        for path in paths {
            self.checked_items.insert(path.clone(), true);
        }
    }

    pub fn update(
        &mut self,
        ui: &mut egui::Ui,
        state: AppStateRef,
        item_cache: &ItemCache,
        item_cache_is_new: bool,
        vault_is_new: bool,
    ) -> anyhow::Result<Option<egui::scroll_area::ScrollAreaOutput<()>>> {
        let state = state.blocking_read();
        let current_vault = state.current_vault()?;

        self.params.container_width = ui.available_width().floor();

        let thumbnail_grid_is_new = self.info.params != self.params;
        if item_cache_is_new || thumbnail_grid_is_new {
            self.set_scroll = true;
            ui.ctx().request_repaint();
            let params = self.params.clone();

            let items = item_cache.resolve_all_refs(&current_vault);

            self.info = compute(params, &items)?;
        }

        if item_cache_is_new {
            let included_paths = item_cache.item_path_set();
            let mut to_remove = vec![];
            for item in &self.checked_items {
                if !included_paths.contains(item.key()) {
                    to_remove.push(item.key().clone());
                }
            }
            for path in to_remove {
                self.checked_items.remove(&path);
            }
        }

        if vault_is_new {
            self.cache.clear();
            self.lq_cache.clear();
        }

        Ok(Some(
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .animated(false)
                .show_viewport(ui, |ui, vp| {
                    if self.info.thumbnails.is_empty() {
                        return;
                    }

                    let abs_min = ui.min_rect().min.to_vec2();
                    let abs_vp = vp.translate(abs_min);
                    let vp_middle = (vp.min + vp.max.to_vec2()) / 2.0;
                    let vp_changed = self.last_vp != Some(vp);
                    let vp_scrolled = vp_changed
                        && (vp.size() - self.last_vp.map_or(vp.size(), |v| v.size())).length_sq()
                            < 1.0;
                    let vp_resized = vp_changed && !vp_scrolled;
                    let max_y = self.info.thumbnails.last().unwrap().outer_bounds.max.y;
                    ui.set_width(ui.available_width());
                    ui.set_height(max_y);
                    ui.set_clip_rect(abs_vp);

                    let selected_paths = self.get_selected_paths();
                    let selected_id = selected_paths.first().map(egui::Id::new);

                    if ui
                        .memory(|r| r.focused())
                        .is_some_and(|f| Some(f) == selected_id)
                        && self.select_mode == SelectMode::Single
                        && shortcut!(ui, Tab)
                    {
                        if let Some((i, _)) = self
                            .info
                            .thumbnails
                            .iter()
                            .find_position(|pos| selected_paths.contains(&pos.path))
                        {
                            let delta = if ui.input(|i| i.modifiers.shift) {
                                -1
                            } else {
                                1
                            };
                            let next_path = self.info.thumbnails
                                [wrap_index(i, self.info.thumbnails.len(), delta)]
                            .path
                            .clone();
                            self.set_scroll = true;
                            self.middle_item = Some(next_path.clone());
                            ui.ctx().memory_mut(|wr| {
                                wr.request_focus(egui::Id::new(next_path.clone()));
                            });
                            self.checked_items.clear();
                            self.checked_items.insert(next_path, true);
                        }
                    }

                    let grid = &self.info;

                    self.double_clicked = false;
                    self.has_focus = false;

                    let middle_item = self.middle_item.as_ref();
                    let hover_item = self.hovering_item.as_ref();
                    let pressing_item = self.pressing_item.as_ref();
                    let mut next_middle: Option<String> = None;
                    let mut next_hover: Option<String> = None;
                    let mut next_pressing: Option<String> = None;

                    for item in &grid.thumbnails {
                        let id = egui::Id::new(item.path.clone());
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

                            let res = ui
                                .push_id(id, |ui| {
                                    ui.put(inner_bounds, img_btn).on_hover_text(&item.path)
                                })
                                .inner;
                            ui.ctx()
                                .check_for_id_clash(res.id, res.rect, "thumbnail image");

                            let is_clicked = res.clicked();
                            if is_hover(&res) {
                                next_hover = Some(item.path.clone());
                            }
                            if res.is_pointer_button_down_on() {
                                next_pressing = Some(item.path.clone());
                            }
                            if res.double_clicked() {
                                self.double_clicked = true;
                            }
                            if res.has_focus() {
                                info!("has focus: {}", item.path.clone());
                                self.has_focus = true;
                                ui.memory_mut(|wr| {
                                    wr.set_focus_lock_filter(
                                        id,
                                        egui::EventFilter {
                                            tab: true,
                                            horizontal_arrows: true,
                                            vertical_arrows: true,
                                            escape: false,
                                        },
                                    );
                                });
                            }

                            let mut req_ex_focus = false;
                            if Some(&item.path) == hover_item
                                || Some(&item.path) == pressing_item
                                || res.is_pointer_button_down_on()
                                || checked
                            {
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
                                    if res.is_pointer_button_down_on() {
                                        next_pressing = Some(item.path.clone());
                                    }

                                    // clicked on image but not on checkbox -> select only this imag
                                    if !res.clicked() && is_clicked {
                                        if self.select_mode == SelectMode::Single
                                            && !ui.input(|i| i.modifiers.ctrl)
                                        {
                                            req_ex_focus = true;
                                        } else {
                                            *check_ref.value_mut() ^= true;
                                        }
                                    }
                                });
                            }

                            if req_ex_focus {
                                ui.memory_mut(|wr| {
                                    wr.request_focus(id);
                                    wr.set_focus_lock_filter(
                                        id,
                                        egui::EventFilter {
                                            tab: true,
                                            horizontal_arrows: true,
                                            vertical_arrows: true,
                                            escape: false,
                                        },
                                    );
                                });
                                self.checked_items.clear();
                                self.checked_items.insert(item.path.clone(), true);
                            }
                        }
                    }

                    let selected_path = self.view_selected_paths(|paths| {
                        if paths.len() == 1 {
                            Some((*paths.first().unwrap()).to_string())
                        } else {
                            None
                        }
                    });

                    if selected_path.is_some() {
                        self.middle_item = selected_path;
                    } else if next_middle.is_some() {
                        self.middle_item = next_middle;
                    }

                    self.hovering_item = next_hover;
                    self.pressing_item = next_pressing;

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
