use crate::shortcut;
use crate::state::AppStateRef;
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::thumb_grid::{compute, ThumbnailPosition};
use crate::tasks::{ThumbnailGridInfo, ThumbnailGridParams};
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::item_cache::ItemCache;
use crate::ui::theme::get_accent_color;
use crate::ui::thumb_cache::{ThumbnailCache, ThumbnailCacheItem};
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use eframe::egui;
use eframe::egui::TextureHandle;
use itertools::Itertools;
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
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
    state: State,

    cache: ThumbnailCache,
    lq_cache: ThumbnailCache,

    scroll_cooldown: Option<DateTime<Utc>>,
    next_hover: Option<String>,
    next_middle: Option<String>,
    next_pressing: Option<String>,
    has_focus: bool,
    set_scroll: bool,
    last_vp: Option<egui::Rect>,

    pub double_clicked: bool,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
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
            state: Default::default(),
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
            scroll_cooldown: None,
            next_hover: None,
            next_middle: None,
            next_pressing: None,
            has_focus: false,
            set_scroll: false,
            last_vp: None,
            double_clicked: false,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct State {
    middle_item: Option<String>,
    hovering_item: Option<String>,
    pressing_item: Option<String>,
    checked_items: DashMap<String, bool>,
    select_mode: SelectMode,
}

impl CloneablePersistedState for State {}

#[derive(Debug)]
struct ViewportInfo {
    rect: egui::Rect,
    abs_min: egui::Vec2,
    scrolled: bool,
    resized: bool,
    middle: egui::Pos2,
}

#[allow(clippy::cast_sign_loss)]
#[allow(clippy::cast_possible_wrap)]
fn wrap_index(i: usize, len: usize, delta: isize) -> usize {
    ((((i as isize) + delta) + len as isize) % (len as isize)) as usize
}

impl ThumbnailGrid {
    #[allow(clippy::unused_self)]
    fn id(&self) -> egui::Id {
        "thumbnail_grid".into()
    }

    pub fn commit(&mut self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        if params.height == crate::ui::THUMBNAIL_LOW_QUALITY_HEIGHT {
            self.lq_cache.commit(params.clone(), item.clone());
        }
        self.cache.commit(params, item);
    }

    fn resolve_thumbnail(&self, item: &ThumbnailPosition) -> ThumbnailCacheItem {
        let path: Box<Path> = Path::new(item.path.as_str()).into();
        #[allow(clippy::cast_possible_truncation)]
        #[allow(clippy::cast_sign_loss)]
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

    pub fn select_mode(&self, ctx: &egui::Context) -> SelectMode {
        State::load(ctx, self.id()).unwrap_or_default().select_mode
    }

    pub fn set_select_mode(&self, ctx: &egui::Context, select_mode: SelectMode) {
        let mut state = State::load(ctx, self.id()).unwrap_or_default();
        state.select_mode = select_mode;
        state.store(ctx, self.id());
    }

    pub fn view_selected_paths<R>(&mut self, f: impl FnOnce(Vec<&String>) -> R) -> R {
        let ro = std::mem::take(&mut self.state.checked_items).into_read_only();
        let res = f(ro.iter().filter(|(_, v)| **v).map(|(k, _)| k).collect());
        drop(std::mem::replace(
            &mut self.state.checked_items,
            ro.into_inner(),
        ));
        res
    }

    pub fn get_selected_paths(&self) -> Vec<String> {
        self.state
            .checked_items
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

    fn handle_tab(&mut self, ui: &mut egui::Ui) {
        let selected_paths = self.get_selected_paths();
        let selected_id = selected_paths.first().map(egui::Id::new);

        if ui
            .memory(|r| r.focused())
            .is_some_and(|f| Some(f) == selected_id)
            && self.state.select_mode == SelectMode::Single
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
                self.state.middle_item = Some(next_path.clone());
                ui.ctx().memory_mut(|wr| {
                    wr.request_focus(egui::Id::new(next_path.clone()));
                });
                self.state.checked_items.clear();
                self.state.checked_items.insert(next_path, true);
            }
        }
    }

    fn compute_viewport_info(&self, ui: &mut egui::Ui, rect: egui::Rect) -> ViewportInfo {
        let size = rect.size();
        let abs_min = ui.min_rect().min.to_vec2();
        let abs_vp = rect.translate(abs_min);
        let middle = (rect.min + rect.max.to_vec2()) / 2.0;
        let changed = self.last_vp.as_ref() != Some(&rect);
        let scrolled =
            changed && (size - self.last_vp.map_or(size, |v| v.size())).length_sq() < 1.0;
        let resized = changed && !scrolled;

        ui.set_width(ui.available_width());
        ui.set_clip_rect(abs_vp);

        ViewportInfo {
            rect,
            abs_min,
            scrolled,
            resized,
            middle,
        }
    }

    fn update_item(&mut self, ui: &mut egui::Ui, item: &ThumbnailPosition, vp: &ViewportInfo) {
        let outer_bounds = item.outer_bounds.translate(vp.abs_min);
        let inner_bounds = item.inner_bounds.translate(vp.abs_min);
        let text = egui::Label::new(item.path.clone());

        // scroll to item if resize event has occurred
        if (self.set_scroll || vp.resized) && Some(&item.path) == self.state.middle_item.as_ref() {
            info!("do scroll to {} at {:?}", &item.path, &outer_bounds);
            info!("set_scroll = {}, resized = {}", self.set_scroll, vp.resized);
            ui.scroll_to_rect(outer_bounds, Some(egui::Align::Center));
            self.set_scroll = false;
            self.scroll_cooldown = Some(Utc::now().add(TimeDelta::milliseconds(
                THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS,
            )));
        }
        // mark current item as item to scroll to when resize occurs
        else if self.scroll_cooldown.unwrap_or(Utc::now()) <= Utc::now()
            && vp.scrolled
            && self.next_middle.is_none()
            && item.outer_bounds.contains(vp.middle)
        {
            self.next_middle = Some(item.path.clone());
        }

        if vp.rect.intersects(item.outer_bounds) {
            let thumb = self.resolve_thumbnail(item);

            if let ThumbnailCacheItem::Loaded(hndl) = thumb {
                self.render_thumbnail(ui, vp, item, &hndl);
            } else {
                ui.put(inner_bounds, text);
                ui.put(inner_bounds, egui::Spinner::new());
            }
        }
    }

    fn render_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        vp: &ViewportInfo,
        item: &ThumbnailPosition,
        hndl: &TextureHandle,
    ) {
        let id = egui::Id::new(item.path.clone());
        let outer_bounds = item.outer_bounds.translate(vp.abs_min);
        let inner_bounds = item.inner_bounds.translate(vp.abs_min);

        let checked = {
            let check_ref = self
                .state
                .checked_items
                .entry(item.path.clone())
                .or_default();
            *check_ref.value()
        };
        if checked {
            let highlight_tint = get_accent_color();
            let highlight_rect = inner_bounds.expand(HIGHLIGHT_PADDING);
            let shape = egui::epaint::RectShape::filled(highlight_rect, ROUNDING, highlight_tint);
            ui.painter_at(highlight_rect).add(shape);
        }

        let img = egui::Image::new(egui::ImageSource::Texture(
            egui::load::SizedTexture::from_handle(hndl),
        ))
        .bg_fill(egui::Color32::from_gray(20))
        .shrink_to_fit();

        let tint = if Some(&item.path) == self.state.hovering_item.as_ref() {
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

        if res.hover_pos().map_or(false, |p| outer_bounds.contains(p)) {
            self.next_hover = Some(item.path.clone());
        }
        if res.is_pointer_button_down_on() {
            self.next_pressing = Some(item.path.clone());
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

        if Some(&item.path) == self.state.hovering_item.as_ref()
            || Some(&item.path) == self.state.pressing_item.as_ref()
            || res.is_pointer_button_down_on()
            || checked
        {
            self.render_checkbox(ui, item, &res);
        }
    }

    fn render_checkbox(
        &mut self,
        ui: &mut egui::Ui,
        item: &ThumbnailPosition,
        image_res: &egui::Response,
    ) {
        ui.scope(|ui| {
            ui.spacing_mut().interact_size = egui::Vec2::splat(CHECKBOX_INTERACT_SIZE);

            let mut check_ref = self
                .state
                .checked_items
                .entry(item.path.clone())
                .or_default();

            let checkbox = egui::Checkbox::new(check_ref.value_mut(), "");
            let checkbox_rect =
                CHECKBOX_ALIGN.align_size_within_rect(CHECKBOX_SIZE, image_res.rect);
            let checkbox_res = ui.put(checkbox_rect, checkbox);

            if checkbox_res
                .hover_pos()
                .map_or(false, |p| checkbox_res.rect.contains(p))
            {
                self.next_hover = Some(item.path.clone());
            }
            if checkbox_res.is_pointer_button_down_on() {
                self.next_pressing = Some(item.path.clone());
            }

            // clicked on image but not on checkbox -> select only this image
            if image_res.clicked() && !checkbox_res.clicked() {
                if self.state.select_mode == SelectMode::Single && !ui.input(|i| i.modifiers.ctrl) {
                    drop(check_ref);
                    self.request_exclusive_focus(ui, item);
                } else {
                    *check_ref.value_mut() ^= true;
                }
            }
        });
    }

    fn request_exclusive_focus(&mut self, ui: &mut egui::Ui, item: &ThumbnailPosition) {
        let id = egui::Id::new(item.path.clone());

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

        self.state.checked_items.clear();
        self.state.checked_items.insert(item.path.clone(), true);
    }

    pub fn update(
        &mut self,
        ui: &mut egui::Ui,
        state: AppStateRef,
        item_cache: &ItemCache,
        item_cache_is_new: bool,
        vault_is_new: bool,
    ) -> Option<egui::scroll_area::ScrollAreaOutput<()>> {
        self.state = State::load(ui.ctx(), self.id()).unwrap_or_default();

        self.params.container_width = ui.available_width().floor();

        let thumbnail_grid_is_new = self.info.params != self.params;
        if item_cache_is_new || thumbnail_grid_is_new {
            self.set_scroll = true;
            ui.ctx().request_repaint();
            let params = self.params.clone();

            let r = state.blocking_read();
            let vault = r.catch(|| "thumb grid".into(), || r.current_vault()).ok()?;
            let items = item_cache.resolve_all_refs(&vault);

            self.info = r
                .catch(|| "thumb grid".into(), || compute(params, &items))
                .ok()?;
        }

        if item_cache_is_new && item_cache.len_items() != 0 {
            let included_paths = item_cache.item_path_set();
            let mut to_remove = vec![];
            for item in &self.state.checked_items {
                if !included_paths.contains(item.key()) {
                    to_remove.push(item.key().clone());
                }
            }
            for path in to_remove {
                self.state.checked_items.remove(&path);
            }
        }

        if vault_is_new {
            self.cache.clear();
            self.lq_cache.clear();
        }

        let res = egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .animated(false)
            .show_viewport(ui, |ui, vp_rect| {
                if self.info.thumbnails.is_empty() {
                    return;
                }

                let vp = self.compute_viewport_info(ui, vp_rect);

                let grid = std::mem::take(&mut self.info);

                let max_y = grid.thumbnails.last().unwrap().outer_bounds.max.y;
                ui.set_height(max_y);

                self.handle_tab(ui);

                self.double_clicked = false;
                self.has_focus = false;

                for item in &grid.thumbnails {
                    self.update_item(ui, item, &vp);
                }

                self.info = grid;

                let selected_path = self.view_selected_paths(|paths| {
                    if paths.len() == 1 {
                        Some((*paths.first().unwrap()).to_string())
                    } else {
                        None
                    }
                });
                let next_middle = std::mem::take(&mut self.next_middle);

                if let Some(middle_item) = selected_path.or(next_middle) {
                    self.state.middle_item = Some(middle_item);
                }

                self.state.hovering_item = std::mem::take(&mut self.next_hover);
                self.state.pressing_item = std::mem::take(&mut self.next_pressing);

                self.last_vp = Some(vp.rect);
            });

        let r = state.blocking_read();
        for params in self.lq_cache.drain_requests() {
            r.add_task(
                format!("Load thumbnail for {}", params.path.display()),
                Box::new(move |s, p| {
                    Promise::spawn_async(load_image_thumbnail_with_fs(s, p, params))
                }),
            );
        }

        for params in self.cache.drain_requests() {
            r.add_task(
                format!("Load thumbnail for {}", params.path.display()),
                Box::new(move |s, p| Promise::spawn_async(load_image_thumbnail(s, p, params))),
            );
        }

        self.state.clone().store(ui.ctx(), self.id());

        Some(res)
    }
}
