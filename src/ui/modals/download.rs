use crate::errors::AppError;
use eframe::egui;
use eframe::egui::Color32;
use egui_extras::TableBody;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::state::AppStateRef;
use crate::tasks::download::{
    GalleryDLLogin, GalleryDLLoginDiscriminants, GalleryDLParams, GalleryDLSource,
    GalleryDLSourceDiscriminants,
};
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::modals::AppModal;

#[derive(Default)]
pub struct Download {
    modal: Option<Modal>,
    error_message: Option<String>,
    params: GalleryDLParams,
    loading_find: bool,
    find_error: Option<String>,
    opened: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    params: GalleryDLParams,
}

impl CloneablePersistedState for State {}

const ROW_HEIGHT: f32 = 18.0;

impl Download {
    fn select_gallery_dl(&mut self, app_state: AppStateRef) {
        self.params.location = None;
        self.params.version = None;
        self.loading_find = true;
        let r = app_state.blocking_read();
        r.add_task_request(self.find_request_id(), |state, p| {
            Promise::spawn_async(crate::tasks::download::select_gallery_dl(state, p))
        });
    }
    fn executable_fragment(&mut self, body: &mut TableBody, state: AppStateRef) {
        body.row(ROW_HEIGHT, |mut row| {
            row.col(|ui| {
                ui.label("gallery-dl: ");
            });
            row.col(|ui| {
                if self.loading_find {
                    ui.label(
                        egui::RichText::new("Detecting...").color(Color32::from_rgb(80, 160, 255)),
                    );
                } else if self.params.location.is_none() || self.params.version.is_none() {
                    ui.label(
                        egui::RichText::new("Not found").color(Color32::from_rgb(255, 80, 80)),
                    );
                    if ui.button("Select...").clicked() {
                        self.select_gallery_dl(state.clone());
                    }
                } else {
                    ui.label(egui::RichText::new("Found").color(Color32::from_rgb(80, 255, 80)));
                    ui.label(format!(" ({})", self.params.version.as_ref().unwrap()));
                    if ui.button("Edit...").clicked() {
                        self.select_gallery_dl(state.clone());
                    }
                }
            });
        });
    }
    fn source_form_fragment(&mut self, body: &mut TableBody) {
        let mut src_discriminant: GalleryDLSourceDiscriminants = (&self.params.source).into();

        body.row(ROW_HEIGHT, |mut row| {
            row.col(|ui| {
                ui.label("Data source: ");
            });
            row.col(|ui| {
                egui::ComboBox::new("gallery_dl_source", "")
                    .selected_text(src_discriminant.to_string())
                    .show_ui(ui, |ui| {
                        macro_rules! discriminant {
                            ($id:ident) => {
                                ui.selectable_value(
                                    &mut src_discriminant,
                                    GalleryDLSourceDiscriminants::$id,
                                    GalleryDLSourceDiscriminants::$id.to_string(),
                                );
                            };
                        }
                        discriminant!(None);
                        discriminant!(TwitterLikes);
                        discriminant!(CustomURL);
                    });
            });
        });

        if src_discriminant != (&self.params.source).into() {
            self.params.source = match src_discriminant {
                GalleryDLSourceDiscriminants::None => GalleryDLSource::None,
                GalleryDLSourceDiscriminants::TwitterLikes => GalleryDLSource::TwitterLikes {
                    username: String::new(),
                },
                GalleryDLSourceDiscriminants::CustomURL => {
                    GalleryDLSource::CustomURL { url: String::new() }
                }
            };
        }

        match &mut self.params.source {
            GalleryDLSource::None => {}
            GalleryDLSource::TwitterLikes { username } => {
                body.row(ROW_HEIGHT, |mut row| {
                    row.col(|ui| {
                        ui.label("Twitter username: ");
                    });
                    row.col(|ui| {
                        ui.text_edit_singleline(username);
                    });
                });
            }
            GalleryDLSource::CustomURL { url } => {
                body.row(ROW_HEIGHT, |mut row| {
                    row.col(|ui| {
                        ui.label("Custom URL: ");
                    });
                    row.col(|ui| {
                        ui.text_edit_singleline(url);
                    });
                });
            }
        }
    }

    fn login_form_fragment(&mut self, body: &mut TableBody) {
        let mut login_discriminant: GalleryDLLoginDiscriminants = (&self.params.login).into();

        body.row(ROW_HEIGHT, |mut row| {
            row.col(|ui| {
                ui.label("Data source: ");
            });
            row.col(|ui| {
                egui::ComboBox::new("gallery_dl_login_combo", "")
                    .selected_text(login_discriminant.to_string())
                    .show_ui(ui, |ui| {
                        macro_rules! discriminant {
                            ($id:ident) => {
                                ui.selectable_value(
                                    &mut login_discriminant,
                                    GalleryDLLoginDiscriminants::$id,
                                    GalleryDLLoginDiscriminants::$id.to_string(),
                                );
                            };
                        }
                        discriminant!(None);
                        discriminant!(ChromeCookies);
                        discriminant!(FirefoxCookies);
                        discriminant!(UsernamePassword);
                    });
            });
        });

        if login_discriminant != (&self.params.login).into() {
            self.params.login = match login_discriminant {
                GalleryDLLoginDiscriminants::None => GalleryDLLogin::None,
                GalleryDLLoginDiscriminants::ChromeCookies => GalleryDLLogin::ChromeCookies,
                GalleryDLLoginDiscriminants::FirefoxCookies => GalleryDLLogin::FirefoxCookies,
                GalleryDLLoginDiscriminants::UsernamePassword => GalleryDLLogin::UsernamePassword {
                    username: String::new(),
                    password: String::new(),
                },
            };
        }

        match &mut self.params.login {
            GalleryDLLogin::None => {}
            GalleryDLLogin::FirefoxCookies => {}
            GalleryDLLogin::ChromeCookies => {}
            GalleryDLLogin::UsernamePassword { username, password } => {
                body.row(ROW_HEIGHT, |mut row| {
                    row.col(|ui| {
                        ui.label("Username: ");
                    });
                    row.col(|ui| {
                        ui.text_edit_singleline(username);
                    });
                });
                body.row(ROW_HEIGHT, |mut row| {
                    row.col(|ui| {
                        ui.label("Password: ");
                    });
                    row.col(|ui| {
                        ui.add(egui::TextEdit::singleline(password).password(true));
                    });
                });
            }
        }
    }

    fn configuration_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Config: ");
        });

        let theme = egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx());
        let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
            let mut layout_job =
                egui_extras::syntax_highlighting::highlight(ui.ctx(), &theme, string, "json");
            layout_job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(layout_job))
        };

        ui.add(
            egui::TextEdit::multiline(&mut self.params.json_config)
                .code_editor()
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter),
        );
    }

    fn find_request_id(&self) -> String {
        format!("Find gallery-dl ({})", self.id().value())
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.params.location.is_none() || self.params.version.is_none() {
            return Err("A valid gallery-dl executable is required.");
        }

        match &self.params.source {
            GalleryDLSource::None => return Err("Please select an image source."),
            GalleryDLSource::TwitterLikes { username } if username.is_empty() => {
                return Err("Please enter your Twitter username.")
            }
            GalleryDLSource::CustomURL { url } if Url::parse(url.as_str()).is_err() => {
                return Err("Please enter a valid URL.")
            }
            _ => {}
        }

        match &self.params.login {
            GalleryDLLogin::None => return Err("Please select a login method."),
            GalleryDLLogin::UsernamePassword { username, password }
                if username.is_empty() || password.is_empty() =>
            {
                return Err("Please enter a username and password.")
            }
            _ => {}
        }

        let Ok(config_val) =
            serde_json::from_str::<serde_json::Value>(self.params.json_config.as_str())
        else {
            return Err("Configuration must be a valid JSON object.");
        };

        if !config_val.is_object() {
            return Err("Configuration must be a valid JSON object.");
        }

        Ok(())
    }
}

impl AppModal for Download {
    fn id(&self) -> eframe::egui::Id {
        "download_modal".into()
    }

    fn update(&mut self, ctx: &eframe::egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, self.id().value()).with_style(&ModalStyle {
            default_width: Some(800.0),
            ..Default::default()
        });
        let mut state = State::load(ctx, self.id()).unwrap_or_default();
        self.params = std::mem::take(&mut state.params);

        if !self.opened {
            let mut log_file = std::env::temp_dir();
            log_file.push(format!("{}.txt", Uuid::new_v4()));
            self.params.log_file = Some(log_file.to_str().unwrap().to_string());

            let r = app_state.blocking_read();
            r.add_task_request(self.find_request_id(), |state, p| {
                Promise::spawn_async(crate::tasks::download::find_gallery_dl(state, p))
            });
            self.loading_find = true;
        }

        match app_state
            .blocking_read()
            .try_take_request_result(&self.find_request_id())
        {
            None => {}
            Some(res) => {
                self.loading_find = false;
                match res {
                    Ok(AsyncTaskResult::FoundGalleryDl { path, version }) => {
                        self.params.location = Some(path);
                        self.params.version = Some(version);
                    }
                    Err(e) if AppError::UserCancelled.is_err(&e) => {}
                    Err(e) => self.find_error = Some(e.to_string()),
                    _ => {}
                }
            }
        }

        modal.show(|ui| {
            modal.title(ui, "Download");
            modal.frame(ui, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::auto())
                    .column(egui_extras::Column::remainder())
                    .auto_shrink([false, true])
                    .vscroll(false)
                    .body(|mut body| {
                        self.executable_fragment(&mut body, app_state.clone());
                        self.source_form_fragment(&mut body);
                        self.login_form_fragment(&mut body);

                        body.row(ROW_HEIGHT, |mut row| {
                            row.col(|ui| {
                                ui.label("Arguments:");
                            });
                            row.col(|ui| {
                                ui.text_edit_singleline(&mut self.params.cli_arguments);
                            });
                        });
                    });

                self.configuration_ui(ui);

                if let Some(msg) = &self.error_message {
                    ui.colored_label(Color32::RED, msg);
                }
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Download").clicked() {
                    if let Err(e) = self.validate() {
                        self.error_message = e.to_string().into();
                        modal.open();
                    } else {
                        let params = self.params.clone();
                        app_state.blocking_read().add_task(
                            self.params.task_name(),
                            |state, progress| {
                                Promise::spawn_async(
                                    crate::tasks::download::perform_gallery_dl_download(
                                        state, progress, params,
                                    ),
                                )
                            },
                        );
                    }
                }
                if modal.button(ui, "Cancel").clicked() {}
            });
        });

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        state.params = std::mem::take(&mut self.params);
        state.store(ctx, self.id());

        self.modal = Some(modal);
        self
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
