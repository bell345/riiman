use eframe::egui;
use eframe::egui::KeyboardShortcut;

use crate::data::{FieldType, ShortcutAction, ShortcutBehaviour};
use crate::state::AppStateRef;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::AppModal;
use crate::ui::{buttons, widgets};

#[derive(Default)]
pub struct TagShortcuts {
    widget_state: State,
    opened: bool,
    updated: bool,
}

#[derive(Clone)]
struct State {
    opened: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { opened: true }
    }
}

impl CloneableTempState for State {}

impl TagShortcuts {
    fn table_row(
        &mut self,
        shortcut: KeyboardShortcut,
        behaviour: &mut ShortcutBehaviour,
        row: &mut egui_extras::Strip,
        state: AppStateRef,
    ) {
        let id = self.id().with(shortcut);

        row.cell(|ui| {
            ui.label(shortcut.format(&egui::ModifierNames::NAMES, false));
        });

        row.cell(|ui| {
            let Ok(vault) = state.current_vault_catch() else {
                return;
            };
            match behaviour.action {
                ShortcutAction::None => {
                    let mut tag_id_opt = None;
                    ui.add(
                        widgets::FindTag::new(id.with("find_tag"), &mut tag_id_opt, vault)
                            .filter_types(&[FieldType::Tag]),
                    );
                    if let Some(new_tag_id) = tag_id_opt {
                        behaviour.action = ShortcutAction::ToggleTag(new_tag_id);
                    }
                }
                ShortcutAction::ToggleTag(tag_id) => {
                    let mut tag_id_opt = Some(tag_id);
                    ui.add(
                        widgets::FindTag::new(id.with("find_tag"), &mut tag_id_opt, vault)
                            .show_tag(true)
                            .filter_types(&[FieldType::Tag])
                            .exclude_ids(&[tag_id]),
                    );
                    match tag_id_opt {
                        Some(new_tag_id) if new_tag_id != tag_id => {
                            behaviour.action = ShortcutAction::ToggleTag(new_tag_id);
                        }
                        _ => {}
                    }
                }
            }
        });

        row.cell(|ui| {
            if !matches!(behaviour.action, ShortcutAction::None) && ui.button("Clear").clicked() {
                behaviour.action = ShortcutAction::None;
            }
        });

        row.cell(|ui| {
            ui.checkbox(&mut behaviour.move_next, "Move next?");
        });
    }
    //noinspection DuplicatedCode
    fn edit_ui(&mut self, ui: &mut egui::Ui, state: AppStateRef) {
        let shortcuts = state.shortcuts();

        egui::ScrollArea::vertical().show_viewport(ui, |ui, _vp| {
            ui.group(|ui| {
                ui.vertical_centered_justified(|ui| {
                    egui_extras::StripBuilder::new(ui)
                        .sizes(egui_extras::Size::exact(24.0), shortcuts.len())
                        .vertical(|mut strip| {
                            for (shortcut, mut behaviour) in shortcuts {
                                strip.strip(|builder| {
                                    builder
                                        .size(egui_extras::Size::exact(100.0))
                                        .size(egui_extras::Size::remainder())
                                        .size(egui_extras::Size::exact(100.0))
                                        .size(egui_extras::Size::exact(100.0))
                                        .horizontal(|mut strip| {
                                            let old_behaviour = behaviour.clone();
                                            self.table_row(
                                                shortcut,
                                                &mut behaviour,
                                                &mut strip,
                                                state.clone(),
                                            );
                                            if behaviour != old_behaviour {
                                                state.set_shortcut(shortcut, behaviour);
                                            }
                                        });
                                });
                            }
                        })
                });
            });
        });
    }
}

impl AppModal for TagShortcuts {
    fn id(&self) -> egui::Id {
        "tag_shortcuts_window".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) {
        self.widget_state = State::load(ctx, self.id()).unwrap_or_default();
        let prev_updated = self.updated;
        let mut opened = self.widget_state.opened;

        let mut do_close = false;

        egui::Window::new("Tag shortcuts")
            .id(self.id())
            .open(&mut opened)
            .min_width(500.0)
            .show(ctx, |ui| {
                buttons(self.id(), ui, |ui| {
                    if ui.button("Close").clicked() {
                        do_close = true;
                    }
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    self.edit_ui(ui, app_state.clone());
                });
            });

        if prev_updated && self.updated {
            self.updated = false;
        }

        if do_close {
            opened = false;
        }

        self.widget_state.opened = opened;
        self.opened = self.widget_state.opened;
        std::mem::take(&mut self.widget_state).store(ctx, self.id());
    }

    fn dispose(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        State::dispose(ctx, self.id());
    }

    fn is_open(&self) -> bool {
        self.opened
    }
}
