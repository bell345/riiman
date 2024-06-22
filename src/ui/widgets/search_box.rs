use std::ops::Add;
use std::sync::{Arc, Mutex};

use eframe::{egui, epaint};
use eframe::egui::{Align2, Color32, CursorIcon, Event, EventFilter, FontSelection, Galley, Key, Margin, Modifiers, NumExt, Rect, Response, Sense, Shape, TextBuffer, Ui, vec2, Vec2, Widget, WidgetInfo};
use eframe::egui::os::OperatingSystem;
use eframe::egui::output::{IMEOutput, OutputEvent};
use eframe::egui::text::{CCursor, CCursorRange, CursorRange};
use eframe::egui::text_edit::TextCursorState;
use eframe::egui::text_selection::text_cursor_state::{ccursor_next_word, cursor_rect};
use eframe::egui::text_selection::visuals::{paint_cursor, paint_text_selection};
use eframe::egui::util::undoer::Undoer;
use eframe::epaint::FontFamily;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::data::{FieldDefinition, FilterExpression, Vault};
use crate::data::parse::{FilterExpressionParseNode, FilterExpressionParseResult, FilterExpressionTextSection, ReplacementStringConversion};
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::{DUMMY_TAG_REPLACEMENT_FAMILY, widgets};

pub struct SearchBox<'a> {
    id: egui::Id,
    text: &'a mut String,
    desired_width: f32,
    tags: Option<&'a Vec<FieldDefinition>>,
    margin: Margin,
    state: State,
    vault: Arc<Vault>
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    cursor: TextCursorState,
    undoer: Arc<Mutex<Undoer<(CCursorRange, String)>>>,
    has_ime: bool,
    ime_cursor_range: CursorRange,
    singleline_offset: f32,
}

impl CloneablePersistedState for State {}

pub struct SearchResponse {
    pub response: Response,
    pub expression: Option<FilterExpressionParseResult>,
}

impl<'a> SearchBox<'a> {
    pub fn new(widget_id: impl std::hash::Hash, text: &'a mut String, vault: Arc<Vault>) -> Self {
        Self {
            id: egui::Id::new(widget_id),
            text,
            desired_width: 200.0,
            tags: None,
            margin: Margin::symmetric(4.0, 2.0),
            state: Default::default(),
            vault
        }
    }

    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = desired_width;
        self
    }

    pub fn tags(mut self, tags: &'a Vec<FieldDefinition>) -> Self {
        self.tags = Some(tags);
        self
    }

    //noinspection DuplicatedCode
    #[allow(clippy::too_many_lines)]
    fn events(&mut self, ui: &mut Ui, galley: &mut Arc<Galley>, layouter: impl Fn(&Ui, &str, f32) -> Arc<Galley>, expr: &Option<FilterExpressionParseResult>, wrap_width: f32) -> (bool, CursorRange) {
        let default_cursor_range = CursorRange::one(galley.end());
        let os = ui.ctx().os();

        let mut galley_range = self.state.cursor.range(galley).unwrap_or(default_cursor_range);
        let mut text_range = expr.replacement_range_to_text_range(galley_range);

        // We feed state to the undoer both before and after handling input
        // so that the undoer creates automatic saves even when there are no events for a while.
        self.state.undoer.lock().unwrap().feed_state(
            ui.input(|i| i.time),
            &(galley_range.as_ccursor_range(), self.text.as_str().to_owned()),
        );

        let mut any_change = false;

        let events = ui.input(|i| i.filtered_events(&EventFilter {
            horizontal_arrows: true,
            vertical_arrows: true,
            tab: false,
            ..Default::default()
        }));
        for event in &events {
            let did_mutate_text = match event {
                // First handle events that only changes the selection cursor, not the text:
                event if galley_range.on_event(os, event, galley, self.id) => {
                    text_range = expr.replacement_range_to_text_range(galley_range);
                    None
                },

                Event::Copy => {
                    if galley_range.is_empty() {
                        ui.ctx().copy_text(self.text.as_str().to_owned());
                    } else {
                        ui.ctx().copy_text(text_range.slice_str(self.text.as_str()).to_owned());
                    }
                    None
                }
                Event::Cut => {
                    if galley_range.is_empty() {
                        ui.ctx().copy_text(self.text.take());
                        Some(CCursorRange::default())
                    } else {
                        ui.ctx().copy_text(text_range.slice_str(self.text.as_str()).to_owned());
                        Some(CCursorRange::one(expr.text_ccursor_to_replacement_ccursor(self.text.delete_selected(&text_range))))
                    }
                }
                Event::Paste(text_to_insert) => { 
                    if text_to_insert.is_empty() {
                        None
                    } else {
                        let mut ccursor = self.text.delete_selected(&text_range);

                        self.text.insert_text_at(&mut ccursor, text_to_insert, usize::MAX);

                        Some(CCursorRange::one(expr.text_ccursor_to_replacement_ccursor(ccursor)))
                    }
                }
                Event::Text(text_to_insert) => {
                    // Newlines are handled by `Key::Enter`.
                    if !text_to_insert.is_empty() && text_to_insert != "\n" && text_to_insert != "\r" {
                        let mut ccursor = self.text.delete_selected(&text_range);

                        self.text.insert_text_at(&mut ccursor, text_to_insert, usize::MAX);

                        Some(CCursorRange::one(expr.text_ccursor_to_replacement_ccursor(ccursor)))
                    } else {
                        None
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if *key == Key::Enter
                    && modifiers.matches_logically(Modifiers::NONE) =>
                    {
                        ui.memory_mut(|mem| mem.surrender_focus(self.id)); // End input with enter
                        break;
                    }
                Event::Key {
                    key: Key::Z,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.matches_logically(Modifiers::COMMAND) => {
                    if let Some((undo_ccursor_range, undo_txt)) = self.state
                        .undoer
                        .lock()
                        .unwrap()
                        .undo(&(galley_range.as_ccursor_range(), self.text.as_str().to_owned()))
                    {
                        self.text.replace_with(undo_txt);
                        Some(*undo_ccursor_range)
                    } else {
                        None
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if (modifiers.matches_logically(Modifiers::COMMAND) && *key == Key::Y)
                    || (modifiers.matches_logically(Modifiers::SHIFT | Modifiers::COMMAND)
                    && *key == Key::Z) =>
                    {
                        if let Some((redo_ccursor_range, redo_txt)) = self.state
                            .undoer
                            .lock()
                            .unwrap()
                            .redo(&(galley_range.as_ccursor_range(), self.text.as_str().to_owned()))
                        {
                            self.text.replace_with(redo_txt);
                            Some(*redo_ccursor_range)
                        } else {
                            None
                        }
                    }

                Event::Key {
                    modifiers,
                    key,
                    pressed: true,
                    ..
                } => {
                    self.check_for_mutating_key_press(os, &galley_range, &text_range, galley, &expr, *modifiers, *key).map(|cur| expr.text_ccursor_range_to_replacement_ccursor_range(cur))
                }

                Event::CompositionStart => {
                    self.state.has_ime = true;
                    None
                }

                Event::CompositionUpdate(text_mark) => {
                    // empty prediction can be produced when user press backspace
                    // or escape during ime. We should clear current text.
                    if text_mark != "\n" && text_mark != "\r" && self.state.has_ime {
                        let mut ccursor = self.text.delete_selected(&text_range);
                        let start_cursor = ccursor;
                        if !text_mark.is_empty() {
                            self.text.insert_text_at(&mut ccursor, text_mark, usize::MAX);
                        }
                        self.state.ime_cursor_range = galley_range;
                        Some(expr.text_ccursor_range_to_replacement_ccursor_range(CCursorRange::two(start_cursor, ccursor)))
                    } else {
                        None
                    }
                }

                Event::CompositionEnd(prediction) => {
                    // CompositionEnd only characters may be typed into TextEdit without trigger CompositionStart first,
                    // so do not check `state.has_ime = true` in the following statement.
                    if prediction != "\n" && prediction != "\r" {
                        self.state.has_ime = false;
                        let mut ccursor;
                        if !prediction.is_empty()
                            && galley_range.secondary.ccursor.index
                            == self.state.ime_cursor_range.secondary.ccursor.index
                        {
                            ccursor = self.text.delete_selected(&text_range);
                            self.text.insert_text_at(&mut ccursor, prediction, usize::MAX);
                        } else {
                            ccursor = galley_range.primary.ccursor;
                        }
                        Some(CCursorRange::one(expr.text_ccursor_to_replacement_ccursor(ccursor)))
                    } else {
                        None
                    }
                }

                _ => None,
            };

            if let Some(new_ccursor_range) = did_mutate_text {
                any_change = true;

                // Layout again to avoid frame delay, and to keep `text` and `galley` in sync.
                *galley = layouter(ui, self.text.as_str(), wrap_width);

                // Set cursor_range using new galley:
                galley_range = CursorRange {
                    primary: galley.from_ccursor(new_ccursor_range.primary),
                    secondary: galley.from_ccursor(new_ccursor_range.secondary),
                };
                text_range = expr.replacement_range_to_text_range(galley_range);
            }
        }

        self.state.cursor.set_range(Some(galley_range));

        self.state.undoer.lock().unwrap().feed_state(
            ui.input(|i| i.time),
            &(galley_range.as_ccursor_range(), self.text.as_str().to_owned()),
        );

        (any_change, galley_range)
    }

    /// Returns `Some(new_cursor)` if we did mutate `text`.
    fn check_for_mutating_key_press(
        &mut self,
        os: OperatingSystem,
        galley_range: &CursorRange,
        text_range: &CursorRange,
        galley: &Galley,
        expr: &Option<FilterExpressionParseResult>,
        modifiers: Modifiers,
        key: Key,
    ) -> Option<CCursorRange> {
        match key {
            Key::Backspace => {
                let ccursor = if let Some(cursor) = galley_range.single() {
                    if modifiers.alt || modifiers.ctrl {
                        // alt on mac, ctrl on windows
                        self.text.delete_previous_word(expr.replacement_ccursor_to_text_ccursor(cursor.ccursor))
                    } else if cursor.ccursor.index > 0 {
                        self.text.delete_selected_ccursor_range([
                            expr.replacement_ccursor_to_text_ccursor(cursor.ccursor - 1),
                            expr.replacement_ccursor_to_text_ccursor(cursor.ccursor)
                        ])
                    } else {
                        cursor.ccursor
                    }
                } else {
                    self.text.delete_selected(text_range)
                };
                Some(CCursorRange::one(ccursor))
            }

            Key::Delete if !modifiers.shift || os != OperatingSystem::Windows => {
                let ccursor = if let Some(cursor) = galley_range.single() {
                    if modifiers.alt || modifiers.ctrl {
                        // alt on mac, ctrl on windows
                        self.text.delete_next_word(expr.replacement_ccursor_to_text_ccursor(cursor.ccursor))
                    } else {
                        self.text.delete_selected_ccursor_range([
                            expr.replacement_ccursor_to_text_ccursor(cursor.ccursor),
                            expr.replacement_ccursor_to_text_ccursor(cursor.ccursor + 1)
                        ])
                    }
                } else {
                    self.text.delete_selected(text_range)
                };
                let ccursor = CCursor {
                    prefer_next_row: true,
                    ..ccursor
                };
                Some(CCursorRange::one(ccursor))
            }

            Key::H if modifiers.ctrl => {
                let ccursor = galley_range.primary.ccursor;
                let ccursor = if ccursor.index > 0 {
                    self.text.delete_selected_ccursor_range([
                        expr.replacement_ccursor_to_text_ccursor(ccursor - 1),
                        expr.replacement_ccursor_to_text_ccursor(ccursor)
                    ])
                } else {
                    expr.replacement_ccursor_to_text_ccursor(ccursor)
                };
                Some(CCursorRange::one(ccursor))
            }

            Key::K if modifiers.ctrl => {
                let ccursor = self.text.delete_paragraph_after_cursor(galley, text_range);
                Some(CCursorRange::one(ccursor))
            }

            Key::U if modifiers.ctrl => {
                let ccursor = self.text.delete_paragraph_before_cursor(galley, text_range);
                Some(CCursorRange::one(ccursor))
            }

            Key::W if modifiers.ctrl => {
                let ccursor = if let Some(cursor) = text_range.single() {
                    self.text.delete_previous_word(cursor.ccursor)
                } else {
                    self.text.delete_selected(text_range)
                };
                Some(CCursorRange::one(ccursor))
            }

            _ => None,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn show_content(
        &mut self,
        ui: &mut Ui,
        reserved_left: f32,
    ) -> (Response, Arc<Galley>) {
        const MIN_WIDTH: f32 = 24.0; // Never make a [`TextEdit`] more narrow than this.
        
        let event_filter = EventFilter {
            horizontal_arrows: true,
            vertical_arrows: true,
            tab: false,
            ..Default::default()
        };
        let text_color = ui.visuals().override_text_color
            // .unwrap_or_else(|| ui.style().interact(&response).text_color()); // too bright
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());

        let prev_text = self.text.as_str().to_owned();

        let font_id = FontSelection::default().resolve(ui.style());
        let row_height = ui.fonts(|f| f.row_height(&font_id));
        let available_width = ui.available_width().at_least(MIN_WIDTH);
        let wrap_width = if ui.layout().horizontal_justify() {
            available_width
        } else {
            self.desired_width.min(available_width)
        };

        let vault = Arc::clone(&self.vault);
        let layouter = move |ui: &Ui, text: &str, _wrap_width: f32| -> Arc<Galley> {
            let mut job = egui::text::LayoutJob::default();
            let style = ui.style();
            
            let normal_fmt = egui::TextFormat::simple(font_id.clone(), style.visuals.text_color());
            let dummy_font = egui::FontId { size: 1.0, family: FontFamily::Name(DUMMY_TAG_REPLACEMENT_FAMILY.into()) };
            let repl_fmt = egui::TextFormat::simple(dummy_font, Color32::TRANSPARENT);
            
            job.append("", reserved_left, normal_fmt.clone());
            
            if let Ok(expr) = text.parse::<FilterExpressionParseResult>() {
                for section in expr.sections() {
                    match section {
                        FilterExpressionTextSection::Normal(start, end) => job.append(
                            &text[start..end],
                            0.0,
                            normal_fmt.clone()
                        ),
                        FilterExpressionTextSection::Replacement(_, node) => job.append(
                            &String::from(node.replacement_char(ui, &vault).unwrap_or(char::REPLACEMENT_CHARACTER)),
                            0.0,
                            repl_fmt.clone()
                        )
                    }
                }
                
            } else {
                job.append(text, 0.0, normal_fmt);
            }

            ui.fonts(|f| f.layout_job(job))
        };

        let mut galley = layouter(ui, self.text.as_str(), wrap_width);

        let desired_width = wrap_width;
        let desired_height = row_height;
        let desired_size =
            vec2(desired_width, galley.size().y.max(desired_height));

        let (_auto_id, rect) = ui.allocate_space(desired_size);

        // On touch screens (e.g. mobile in `eframe` web), should
        // dragging select text, or scroll the enclosing [`ScrollArea`] (if any)?
        // Since currently copying selected text in not supported on `eframe` web,
        // we prioritize touch-scrolling:
        let allow_drag_to_select =
            ui.input(|i| !i.has_touch_screen()) || ui.memory(|mem| mem.has_focus(self.id));

        let sense = if allow_drag_to_select { Sense::click_and_drag() } else { Sense::click() };
        let mut response = ui.interact(rect, self.id, sense);
        let text_clip_rect = rect;
        let painter = ui.painter_at(text_clip_rect.expand(1.0)); // expand to avoid clipping cursor

        if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
            if response.hovered() {
                ui.output_mut(|o| o.mutable_text_under_cursor = true);
            }

            let singleline_offset = vec2(self.state.singleline_offset, 0.0);
            let cursor_at_pointer =
                galley.cursor_from_pos(pointer_pos - response.rect.min + singleline_offset);

            if ui.visuals().text_cursor_preview
                && response.hovered()
                && ui.input(|i| i.pointer.is_moving())
            {
                // preview:
                let cursor_rect =
                    cursor_rect(response.rect.min, &galley, &cursor_at_pointer, row_height);
                paint_cursor(&painter, ui.visuals(), cursor_rect);
            }

            let is_being_dragged = ui.ctx().is_being_dragged(response.id);
            let did_interact = self.state.cursor.pointer_interaction(
                ui,
                &response,
                cursor_at_pointer,
                &galley,
                is_being_dragged,
            );

            if did_interact {
                ui.memory_mut(|mem| mem.request_focus(response.id));
            }
        }

        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }

        let mut cursor_range = None;
        let prev_cursor_range = self.state.cursor.range(&galley);
        if ui.memory(|mem| mem.has_focus(self.id)) {
            ui.memory_mut(|mem| mem.set_focus_lock_filter(self.id, event_filter));

            let expr = self.text.parse::<FilterExpressionParseResult>().ok();
            let (changed, new_cursor_range) = self.events(
                ui,
                &mut galley,
                layouter,
                &expr,
                wrap_width,
            );

            if changed {
                response.mark_changed();
            }
            cursor_range = Some(new_cursor_range);
        }

        let mut galley_pos = Align2::LEFT_TOP
            .align_size_within_rect(galley.size(), response.rect)
            .intersect(response.rect) // limit pos to the response rect area
            .min;
        let align_offset = response.rect.left() - galley_pos.x;

        // Visual clipping for singleline text editor with text larger than width
        if align_offset == 0.0 {
            let cursor_pos = match (cursor_range, ui.memory(|mem| mem.has_focus(self.id))) {
                (Some(cursor_range), true) => galley.pos_from_cursor(&cursor_range.primary).min.x,
                _ => 0.0,
            };

            let mut offset_x = self.state.singleline_offset;
            let visible_range = offset_x..=offset_x + desired_size.x;

            if !visible_range.contains(&cursor_pos) {
                if cursor_pos < *visible_range.start() {
                    offset_x = cursor_pos;
                } else {
                    offset_x = cursor_pos - desired_size.x;
                }
            }

            offset_x = offset_x
                .at_most(galley.size().x - desired_size.x)
                .at_least(0.0);

            self.state.singleline_offset = offset_x;
            galley_pos -= vec2(offset_x, 0.0);
        } else {
            self.state.singleline_offset = align_offset;
        }

        let selection_changed = if let (Some(cursor_range), Some(prev_cursor_range)) =
            (cursor_range, prev_cursor_range)
        {
            prev_cursor_range.as_ccursor_range() != cursor_range.as_ccursor_range()
        } else {
            false
        };

        if ui.is_rect_visible(rect) {
            painter.galley(galley_pos, galley.clone(), text_color);

            if ui.memory(|mem| mem.has_focus(self.id)) {
                if let Some(cursor_range) = self.state.cursor.range(&galley) {
                    // We paint the cursor on top of the text, in case
                    // the text galley has backgrounds (as e.g. `code` snippets in markup do).
                    paint_text_selection(
                        &painter,
                        ui.visuals(),
                        galley_pos,
                        &galley,
                        &cursor_range,
                        None,
                    );

                    let primary_cursor_rect =
                        cursor_rect(galley_pos, &galley, &cursor_range.primary, row_height);

                    let is_fully_visible = ui.clip_rect().contains_rect(rect);
                    if (response.changed || selection_changed) && !is_fully_visible {
                        // Scroll to keep primary cursor in view:
                        ui.scroll_to_rect(primary_cursor_rect, None);
                    }

                    paint_cursor(&painter, ui.visuals(), primary_cursor_rect);

                    // For IME, so only set it when text is editable and visible!
                    ui.ctx().output_mut(|o| {
                        o.ime = Some(IMEOutput {
                            rect,
                            cursor_rect: primary_cursor_rect,
                        });
                    });
                }
            }
        }

        if response.changed {
            response.widget_info(|| {
                WidgetInfo::text_edit(
                    prev_text.as_str(),
                    self.text.as_str(),
                )
            });
        } else if selection_changed {
            let cursor_range = cursor_range.unwrap();
            let char_range =
                cursor_range.primary.ccursor.index..=cursor_range.secondary.ccursor.index;
            let info = WidgetInfo::text_selection_changed(
                char_range,
                self.text.as_str(),
            );
            response.output_event(OutputEvent::TextSelectionChanged(info));
        } else {
            response.widget_info(|| {
                WidgetInfo::text_edit(
                    prev_text.as_str(),
                    self.text.as_str()
                )
            });
        }

        (response, galley)
    }

    fn show_edit_field(&mut self, ui: &mut Ui, reserved_left: f32) -> (Response, Arc<Galley>) {
        let is_mutable = self.text.is_mutable();
        let where_to_put_background = ui.painter().add(Shape::Noop);

        let margin = self.margin;
        let available = ui.available_rect_before_wrap();
        let max_rect = margin.shrink_rect(available);
        let mut content_ui = ui.child_ui(max_rect, *ui.layout());

        let (mut res, galley) = self.show_content(&mut content_ui, reserved_left);

        let id = res.id;
        let frame_rect = margin.expand_rect(res.rect);
        ui.allocate_space(frame_rect.size());

        res |= ui.interact(frame_rect, id, Sense::click());
        if res.clicked() && !res.lost_focus() {
            ui.memory_mut(|mem| mem.request_focus(res.id));
        }

        let visuals = ui.style().interact(&res);
        let frame_rect = frame_rect.expand(visuals.expansion);
        let shape = if is_mutable {
            if res.has_focus() {
                epaint::RectShape::new(
                    frame_rect,
                    visuals.rounding,
                    ui.visuals().extreme_bg_color,
                    ui.visuals().selection.stroke,
                )
            } else {
                epaint::RectShape::new(
                    frame_rect,
                    visuals.rounding,
                    ui.visuals().extreme_bg_color,
                    visuals.bg_stroke
                )
            }
        } else {
            let visuals = &ui.style().visuals.widgets.inactive;
            epaint::RectShape::stroke(
                frame_rect,
                visuals.rounding,
                visuals.bg_stroke
            )
        };

        ui.painter().set(where_to_put_background, shape);

        (res, galley)
    }

    pub fn show(mut self, ui: &mut Ui) -> SearchResponse {
        const TAG_PADDING: Vec2 = vec2(2.0, 0.0);
        
        self.state = State::load(ui.ctx(), self.id).unwrap_or_default();

        let empty_vec = vec![];
        let tags = self.tags.unwrap_or(&empty_vec);
        let tag_sizes: Vec<_> = tags
            .iter()
            .map(|t| widgets::Tag::new(t).small(true).size(ui))
            .collect();

        let style = ui.style();
        let icon_galley = ui.fonts(|f| {
            f.layout_job(egui::text::LayoutJob::simple_singleline(
                "\u{1f50d}".into(),
                egui::TextStyle::Button.resolve(style),
                style.visuals.strong_text_color(),
            ))
        });

        let icon_reserved_width = icon_galley.rect.width() + style.spacing.icon_spacing;

        #[allow(clippy::cast_precision_loss)]
        let reserved_width = icon_reserved_width
            + tag_sizes.iter().map(|s| s.x).sum::<f32>()
            + style.spacing.item_spacing.x * tag_sizes.len().saturating_sub(1) as f32;

        let (output, galley)= self.show_edit_field(ui, reserved_width);

        let style = ui.style();
        let mut tag_location = output.rect.left_center() + vec2(icon_reserved_width, 0.0);
        for (def, size) in tags.iter().zip(tag_sizes) {
            let rect = Rect::from_min_size(tag_location - vec2(0.0, size.y / 2.0), size);
            widgets::Tag::new(def).small(true).paint(ui, rect, None);
            tag_location += vec2(size.x + style.spacing.item_spacing.x, 0.0);
        }

        let style = ui.style();
        let painter = ui.painter_at(output.rect);

        painter.galley(
            output.rect.min.add(vec2(
                style.spacing.button_padding.x,
                (output.rect.height() / 2.0) - (icon_galley.rect.height() / 2.0),
            )),
            icon_galley,
            Color32::TRANSPARENT,
        );

        if self.text.is_empty() {
            painter.text(
                output.rect.min.add(vec2(
                    reserved_width + style.spacing.icon_spacing,
                    output.rect.size().y / 2.0,
                )),
                Align2::LEFT_CENTER,
                "Search...",
                egui::TextStyle::Body.resolve(style),
                style.visuals.weak_text_color(),
            );
        } else {
            let rect = Rect::from_min_size(
                output.rect.right_top() - vec2(output.rect.size().y, 0.0),
                Vec2::splat(output.rect.size().y),
            );
            if ui
                .put(rect, egui::Button::new("\u{274c}").frame(false))
                .on_hover_cursor(CursorIcon::Default)
                .clicked()
            {
                self.text.clear();
            }
        }
        
        let expr = self.text.parse::<FilterExpressionParseResult>().ok();

        if let Some(expr) = expr.as_ref() {
            for section in expr.sections() {
                if let FilterExpressionTextSection::Replacement(index, node) = section {
                    let cur = galley.pos_from_ccursor(CCursor { index, prefer_next_row: true }).translate(output.rect.min.to_vec2()).translate(TAG_PADDING);
                    let size = node.replacement_size(ui, &self.vault).expect("replacement size to exist for replacement node");
                    let clip_rect = Align2::LEFT_TOP.align_size_within_rect(size, cur);
                    info!("output.rect = {:?}, cur = {cur:?}, size = {size:?}, clip_rect = {clip_rect:?}", output.rect);
                    match node.expr {
                        FilterExpression::TagMatch(id) | FilterExpression::FieldMatch(id, _) => {
                            if let Some(def) = self.vault.get_definition(&id) {
                                let tag = widgets::Tag::new(&def).small(true);
                                let tag_size = tag.size(ui);
                                info!("clip_rect size = {:?}, tag size = {tag_size:?}", clip_rect.size());
                                ui.put(Align2::LEFT_CENTER.align_size_within_rect(tag_size, clip_rect), tag);
                            }
                        },
                        _ => continue
                    }
                }
            }
        }

        std::mem::take(&mut self.state).store(ui.ctx(), self.id);

        SearchResponse {
            response: output,
            expression: expr
        }
    }
}

impl<'a> Widget for SearchBox<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}
