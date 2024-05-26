use std::collections::HashMap;
use std::sync::OnceLock;

use eframe::egui;
use eframe::egui::{FontData, FontDefinitions, FontTweak};
use eframe::epaint::FontFamily;
use poll_promise::Promise;
use tracing::info;
use uuid::Uuid;

use crate::errors::AppError;

use crate::state::{AppState, AppStateRef};
use crate::tasks::filter::FilterExpression;
use crate::tasks::AsyncTaskResult::{ImportComplete, ThumbnailLoaded, VaultLoaded, VaultSaved};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskState};

use crate::tasks::sort::{SortDirection, SortExpression, SortType};
use crate::ui::item_cache::ItemCache;
use crate::ui::item_panel::ItemPanel;
use crate::ui::stepwise_range::StepwiseRange;
use crate::ui::thumb_cache::ThumbnailCacheItem;
use crate::ui::thumb_grid::{SelectMode, ThumbnailGrid};

mod cloneable_state;
mod input;
mod item_cache;
mod item_panel;
mod modals;
mod stepwise_range;
mod theme;
mod thumb_cache;
mod thumb_grid;
mod widgets;

pub use crate::ui::modals::AppModal;
pub use crate::ui::modals::MessageDialog;
use crate::ui::modals::{EditTagDialog, NewVaultDialog};

static THUMBNAIL_SLIDER_RANGE: OnceLock<StepwiseRange> = OnceLock::new();

const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;
const MAX_RUNNING_TASKS: usize = 16;

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    modal_dialogs: HashMap<egui::Id, Box<dyn AppModal>>,

    item_list_cache: ItemCache,
    thumbnail_grid: ThumbnailGrid,

    sort_type: SortType,
    sort_field_id: Option<Uuid>,
    sort_direction: SortDirection,
    search_text: String,

    expand_right_panel: bool,
    focused: Option<egui::Id>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AppStorage {
    vault_name_to_file_paths: HashMap<String, String>,
    current_vault_name: Option<String>,
    thumbnail_row_height: f32,
    checked_items: Vec<String>,
    sorts: Vec<SortExpression>,
    filter: FilterExpression,
}

impl AppStorage {
    const KEY: &'static str = "AppStorage";
}

impl App {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    fn add_task(
        &mut self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.state.blocking_read().add_task(name, task_factory);
    }

    fn add_queued_tasks(&mut self) {
        let capacity = MAX_RUNNING_TASKS - self.tasks.running_tasks_count();
        for (name, task_factory, is_request) in self.state.blocking_write().drain_tasks(capacity) {
            match is_request {
                true => self
                    .tasks
                    .add_request(name, |tx| task_factory(self.state.clone(), tx)),
                false => self
                    .tasks
                    .add(name, |tx| task_factory(self.state.clone(), tx)),
            }
        }
    }

    fn add_modal_dialog(&mut self, dialog: impl AppModal + 'static) {
        let b = Box::new(dialog);
        self.modal_dialogs.insert(b.id(), b);
    }

    fn error(&mut self, message: String) {
        self.add_modal_dialog(MessageDialog::error(message));
    }

    fn success(&mut self, title: String, message: String) {
        self.add_modal_dialog(MessageDialog::success(message).with_title(title));
    }

    fn state(&self) -> tokio::sync::RwLockReadGuard<AppState> {
        self.state.blocking_read()
    }

    fn load_persistent_state(&mut self, storage: Option<&dyn eframe::Storage>) -> Option<()> {
        let stored_state: AppStorage =
            serde_json::from_str(&storage?.get_string(AppStorage::KEY)?).ok()?;

        for (name, path) in stored_state.vault_name_to_file_paths.into_iter() {
            let set_as_current = stored_state.current_vault_name.as_ref() == Some(&path);
            if set_as_current {
                self.state.blocking_read().set_vault_loading();
            }

            self.add_task(format!("Load vault {name}"), move |s, p| {
                Promise::spawn_async(crate::tasks::vault::load_vault_from_path(
                    path,
                    s,
                    p,
                    set_as_current,
                ))
            });
        }

        self.thumbnail_grid.params.max_row_height = stored_state.thumbnail_row_height;
        self.thumbnail_grid
            .set_selected_paths(&stored_state.checked_items);

        {
            let mut wr = self.state.blocking_write();
            wr.filter = stored_state.filter;
            wr.sorts = stored_state.sorts;
        }

        Some(())
    }

    fn setup(&mut self, ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
        ctx.style_mut(|style| style.animation_time = 0.0);
        self.load_persistent_state(storage);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(f) = self.focused.take() {
            ctx.memory_mut(|m| m.request_focus(f));
        }

        let errors = self.state.blocking_write().drain_errors();
        for error in errors {
            self.error(format!("{}", error));
        }

        for new_dialog in self.state.blocking_write().drain_dialogs() {
            if let Some(mut old_dialog) = self.modal_dialogs.remove(&new_dialog.id()) {
                old_dialog.dispose(ctx, self.state.clone());
            }
            self.modal_dialogs.insert(new_dialog.id(), new_dialog);
        }

        self.modal_dialogs
            .retain(|_, dialog| dialog.update_or_dispose(ctx, self.state.clone()));

        self.add_queued_tasks();

        let (results, request_results) = self.tasks.iter_ready();
        for result in results {
            match result {
                Ok(AsyncTaskResult::None) => {}
                Ok(VaultLoaded {
                    name,
                    set_as_current,
                }) if set_as_current => {
                    let r = self.state.blocking_read();
                    if r.set_current_vault_name(name.clone()).is_err() {
                        r.reset_vault_loading();
                        drop(r);
                        self.error(format!(
                            "Failed to set current vault with name '{name}' \
                             as it could not be found"
                        ));
                    }
                }
                Ok(VaultLoaded { .. }) => {}
                Ok(VaultSaved(_)) => self.state.blocking_read().reset_vault_loading(),
                Ok(ImportComplete { path, results }) => {
                    let total = results.len();
                    let success = results.iter().filter(|r| r.is_ok()).count();
                    let body = format!(
                        "Import of {} complete. {success}/{total} images imported successfully.",
                        path.display()
                    );
                    // update thumbnail grid
                    self.thumbnail_grid.params.container_width = 0.0;
                    self.success("Import complete".to_string(), body);
                }
                Ok(ThumbnailLoaded { params, image }) => {
                    let hndl = ctx.load_texture(params.tex_name(), image, Default::default());
                    self.thumbnail_grid
                        .commit(params, ThumbnailCacheItem::Loaded(hndl));
                }
                Ok(AsyncTaskResult::FoundGalleryDl { .. }) => {}
                Err(e) if AppError::NotImplemented.is_err(&e) => {
                    self.error("Not implemented".to_string())
                }
                Err(e) => self.error(format!("{e:#}")),
            }
            ctx.request_repaint();
        }

        self.state
            .blocking_read()
            .push_request_results(request_results);

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Vault", |ui| {
                    let vault_loading = self.state.blocking_read().vault_loading();
                    if ui
                        .add_enabled(!vault_loading, egui::Button::new("New..."))
                        .clicked()
                    {
                        info!("New vault clicked!");

                        self.add_modal_dialog(NewVaultDialog::default());

                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!vault_loading, egui::Button::new("Open..."))
                        .clicked()
                    {
                        info!("Open vault clicked!");

                        self.state.blocking_read().set_vault_loading();
                        self.add_task("Load vault".into(), |s, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(
                                s, p, true,
                            ))
                        });

                        ui.close_menu();
                    }

                    if self.state().current_vault().is_ok()
                        && ui
                            .add_enabled(!vault_loading, egui::Button::new("Save"))
                            .clicked()
                    {
                        info!("Save vault clicked!");

                        self.state.blocking_read().save_current_vault();

                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                if self.state().current_vault().is_ok() {
                    ui.menu_button("Import", |ui| {
                        if ui.button("Import...").clicked() {
                            self.add_task("Import one".into(), |state, p| {
                                Promise::spawn_async(crate::tasks::import::select_and_import_one(
                                    state, p,
                                ))
                            });

                            ui.close_menu();
                        }

                        if ui.button("Import all files").clicked() {
                            info!("Import all clicked!");

                            self.add_task("Import to vault".into(), |state, p| {
                                Promise::spawn_async(
                                    crate::tasks::import::import_images_recursively(state, p),
                                )
                            });

                            ui.close_menu();
                        }

                        if ui.button("Download...").clicked() {
                            self.add_modal_dialog(modals::Download::default());

                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Tags", |ui| {
                        if ui.button("New...").clicked() {
                            self.add_modal_dialog(EditTagDialog::create());
                            ui.close_menu();
                        }
                        if ui.button("Edit...").clicked() {
                            self.add_modal_dialog(EditTagDialog::select());
                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Link", |ui| {
                        if ui.button("Other Vault...").clicked() {}
                        if ui.button("Sidecars...").clicked() {
                            self.add_task("Link sidecars".into(), |state, p| {
                                Promise::spawn_async(crate::tasks::link::link_sidecars(state, p))
                            });
                            ui.close_menu();
                        }
                    });
                }

                ui.add_space(16.0);
            });
        });

        egui::TopBottomPanel::top("search_panel")
            .max_height(24.0)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let slider_range = THUMBNAIL_SLIDER_RANGE.get_or_init(|| {
                        StepwiseRange::new(&[0.0, 1.0, 2.0, 3.0], &[128.0, 256.0, 512.0, 1024.0])
                    });
                    let mut slider_value =
                        slider_range.lerp_in(self.thumbnail_grid.params.max_row_height);

                    ui.add(
                        egui::widgets::Slider::new(&mut slider_value, slider_range.input_range())
                            .step_by(1.0)
                            .show_value(false),
                    );

                    self.thumbnail_grid.params.max_row_height = slider_range.lerp_out(slider_value);

                    // square four corners
                    ui.label("\u{26f6}");

                    ui.add_space(16.0);

                    if ui
                        .add(egui::Button::new(self.sort_direction.to_icon()).frame(false))
                        .clicked()
                    {
                        self.sort_direction = !self.sort_direction;
                    }

                    if self.sort_type == SortType::Field {
                        let state = self.state.blocking_read();
                        if let Some(vault) = state.current_vault_opt() {
                            ui.add(
                                widgets::FindTag::new(
                                    "sort_field",
                                    &mut self.sort_field_id,
                                    &vault,
                                )
                                .show_tag(true),
                            );
                        };
                    }

                    egui::ComboBox::from_label("Sort by")
                        .selected_text(self.sort_type.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.sort_type,
                                SortType::Path,
                                SortType::Path.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.sort_type,
                                SortType::Field,
                                SortType::Field.to_string(),
                            );

                            ui.style_mut().visuals.widgets.inactive.rounding.ne = 0.0;
                            ui.style_mut().visuals.widgets.inactive.rounding.se = 0.0;
                        });

                    ui.add(
                        widgets::SearchBox::new(&mut self.search_text).desired_width(f32::INFINITY),
                    );

                    {
                        let mut wr = self.state.blocking_write();
                        wr.sorts = match self.sort_type {
                            SortType::Path => vec![SortExpression::Path(self.sort_direction)],
                            SortType::Field => {
                                if let Some(field_id) = self.sort_field_id {
                                    vec![SortExpression::Field(field_id, self.sort_direction)]
                                } else {
                                    vec![]
                                }
                            }
                        };
                        wr.filter = FilterExpression::TextSearch(self.search_text.clone().into());
                    }
                });
            });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} {} ({}) compiled {}",
                    crate::built_info::PKG_NAME,
                    crate::built_info::PKG_VERSION,
                    crate::built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("?"),
                    crate::built_info::built_time()
                ));

                let progresses = self.tasks.iter_progress();
                match &progresses[..] {
                    [] => {}
                    [(name, ProgressState::NotStarted), ..]
                    | [(name, ProgressState::Indeterminate), ..] => {
                        ui.add(egui::ProgressBar::new(0.0).text(name).animate(true));
                    }
                    [(name, ProgressState::Determinate(progress)), ..] => {
                        ui.add(
                            egui::ProgressBar::new(*progress)
                                .text(format!("{}% {name}", (progress * 100.0).floor() as u32)),
                        );
                    }
                    [(name, ProgressState::DeterminateWithMessage(progress, msg)), ..] => {
                        ui.add(egui::ProgressBar::new(*progress).text(format!(
                            "{}% {name}: {msg}",
                            (progress * 100.0).floor() as u32
                        )));
                    }
                    [(name, ProgressState::Completed), ..] => {
                        ui.add(egui::ProgressBar::new(1.0).text(name));
                    }
                };
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::SidePanel::right("right_panel").show_animated_inside(
                ui,
                self.expand_right_panel,
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .max_width(350.0)
                        .show_viewport(ui, |ui, _vp| {
                            ui.horizontal(|ui| {
                                ui.label("Select: ");
                                ui.selectable_value(
                                    &mut self.thumbnail_grid.select_mode,
                                    SelectMode::Single,
                                    "Single",
                                );
                                ui.selectable_value(
                                    &mut self.thumbnail_grid.select_mode,
                                    SelectMode::Multiple,
                                    "Multiple",
                                );
                            });

                            let r = self.state.blocking_read();
                            let Some(vault) = r.current_vault_opt() else {
                                return;
                            };

                            let items = self.thumbnail_grid.view_selected_paths(|paths| {
                                self.item_list_cache.resolve_refs(&vault, paths)
                            });

                            ui.add(ItemPanel::new(
                                "item_panel",
                                &items,
                                &vault,
                                self.state.clone(),
                            ));
                        });
                },
            );

            let scroll_area_rect = egui::CentralPanel::default()
                .show_inside(ui, |ui| {
                    let mut update =
                        || -> anyhow::Result<Option<egui::scroll_area::ScrollAreaOutput<()>>> {
                            let is_new_item_list =
                                self.item_list_cache.update(self.state.clone())?;
                            self.thumbnail_grid.update(
                                ui,
                                self.state.clone(),
                                &self.item_list_cache,
                                is_new_item_list,
                            )
                        };

                    let mut scroll_area_rect: Option<egui::Rect> = None;
                    match update() {
                        Ok(Some(egui::scroll_area::ScrollAreaOutput { inner_rect, .. })) => {
                            scroll_area_rect = Some(inner_rect);
                        }
                        Ok(_) => {}
                        Err(e) if AppError::NoCurrentVault.is_err(&e) => {}
                        Err(e) => self.error(format!("{e:?}")),
                    }
                    scroll_area_rect
                })
                .inner;

            if self.thumbnail_grid.double_clicked {
                self.expand_right_panel ^= true;
            }

            const EXPAND_BTN_SIZE: egui::Vec2 = egui::vec2(32.0, 32.0);
            const EXPAND_BTN_ROUNDING: egui::Rounding = egui::Rounding {
                nw: 0.0,
                ne: 0.0,
                sw: 8.0,
                se: 8.0,
            };
            const EXPAND_BTN_MARGIN: egui::Vec2 = egui::vec2(16.0, 0.0);

            let btn_text = if self.expand_right_panel {
                // right pointing triangle
                "\u{25b6}"
            } else {
                // left pointing triangle
                "\u{25c0}"
            };
            let expand_btn = egui::Button::new(
                egui::RichText::new(btn_text).text_style(egui::TextStyle::Heading),
            )
            .rounding(EXPAND_BTN_ROUNDING)
            .min_size(EXPAND_BTN_SIZE);

            let btn_rect = egui::Align2::RIGHT_TOP.align_size_within_rect(
                EXPAND_BTN_SIZE,
                scroll_area_rect
                    .unwrap_or(ui.min_rect())
                    .shrink2(EXPAND_BTN_MARGIN),
            );

            if ui.put(btn_rect, expand_btn).clicked() {
                self.expand_right_panel ^= true;
            }
        });

        self.focused = ctx.memory(|m| m.focused());
        self.thumbnail_grid.view_selected_paths(|paths| {
            if self.focused.is_none() && paths.len() == 1 {
                self.focused = Some(egui::Id::new(paths.first().unwrap()));
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = self.state.blocking_read();
        let stored_state = AppStorage {
            current_vault_name: state.current_vault_name().map(|s| s.to_string()),
            vault_name_to_file_paths: state.vault_name_to_file_paths(),
            thumbnail_row_height: self.thumbnail_grid.params.max_row_height,
            checked_items: self
                .thumbnail_grid
                .view_selected_paths(|paths| paths.into_iter().cloned().collect()),
            sorts: state.sorts.clone(),
            filter: state.filter.clone(),
        };

        storage.set_string(
            AppStorage::KEY,
            serde_json::to_string(&stored_state).expect("state to serialise properly"),
        );
    }
}

impl App {
    pub(crate) fn run(mut self) -> Result<(), eframe::Error> {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 800.0]),
            ..Default::default()
        };

        eframe::run_native(
            "riiman",
            options,
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);

                let mut fonts = FontDefinitions::default();

                const M_PLUS_ROUNDED: &str = "MPLUSRounded1c-Regular";
                const JETBRAINS_MONO: &str = "JetBrainsMono-Regular";
                const INTER: &str = "Inter-Regular";
                const NOTO_SANS: &str = "NotoSans-Regular";
                const NOTO_SANS_SC: &str = "NotoSansSC-Regular";
                const NOTO_SANS_TC: &str = "NotoSansTC-Regular";
                const NOTO_SANS_KR: &str = "NotoSansKR-Regular";
                const NOTO_SANS_SYMBOLS: &str = "NotoSansSymbols-Regular";
                const NOTO_SANS_SYMBOLS_2: &str = "NotoSansSymbols2-Regular";
                const NOTO_SANS_MATH: &str = "NotoSansMath-Regular";
                const NOTO_EMOJI: &str = "NotoEmoji-Regular";

                fonts.font_data.insert(
                    M_PLUS_ROUNDED.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/MPLUSRounded1c/MPLUSRounded1c-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    JETBRAINS_MONO.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/JetBrainsMono/JetBrainsMono-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    INTER.to_owned(),
                    FontData::from_static(include_bytes!("../res/font/Inter/Inter-Regular.ttf")),
                );
                fonts.font_data.insert(
                    NOTO_SANS.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSans/NotoSans-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_SC.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansSC/NotoSansSC-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_TC.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansTC/NotoSansTC-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_KR.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansKR/NotoSansKR-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_SYMBOLS.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansSymbols/NotoSansSymbols-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_SYMBOLS_2.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansSymbols2/NotoSansSymbols2-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_SANS_MATH.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoSansMath/NotoSansMath-Regular.ttf"
                    )),
                );
                fonts.font_data.insert(
                    NOTO_EMOJI.to_owned(),
                    FontData::from_static(include_bytes!(
                        "../res/font/NotoEmoji/NotoEmoji-Regular.ttf"
                    )),
                );

                {
                    let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
                    // pushing to front (so highest priority is at the end here)
                    prop.insert(0, NOTO_SANS_KR.to_owned());
                    prop.insert(0, NOTO_SANS_TC.to_owned());
                    prop.insert(0, NOTO_SANS_SC.to_owned());
                    prop.insert(0, M_PLUS_ROUNDED.to_owned());
                    prop.insert(0, INTER.to_owned());

                    // fallback symbols
                    prop.push(NOTO_SANS_SYMBOLS.to_owned());
                    prop.push(NOTO_SANS_SYMBOLS_2.to_owned());
                    prop.push(NOTO_SANS_MATH.to_owned());
                    prop.push(NOTO_EMOJI.to_owned());
                }

                {
                    let mono = fonts.families.get_mut(&FontFamily::Monospace).unwrap();
                    mono.insert(0, JETBRAINS_MONO.to_owned());
                }

                cc.egui_ctx.set_fonts(fonts);

                self.setup(&cc.egui_ctx, cc.storage);
                Box::new(self)
            }),
        )
    }
}
