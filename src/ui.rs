use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::clipboard::ClipboardEntry;
use crate::history::ClipboardHistory;

const MAX_LABEL_CHARS: usize = 100;

// -- Color palette --
const BG_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x1B, 0x1E);
const BG_SURFACE: egui::Color32 = egui::Color32::from_rgb(0x22, 0x23, 0x27);
const BG_HOVER: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2D, 0x33);
const BG_SELECTED: egui::Color32 = egui::Color32::from_rgb(0x1A, 0x4A, 0x8A);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0xE4, 0xE6, 0xEB);
const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x9A, 0xA0, 0xAC);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x64, 0x6C, 0x7A);
const ACCENT_BLUE: egui::Color32 = egui::Color32::from_rgb(0x58, 0xA6, 0xFF);
const BORDER_SUBTLE: egui::Color32 = egui::Color32::from_rgb(0x30, 0x33, 0x3A);
const DELETE_HOVER: egui::Color32 = egui::Color32::from_rgb(0xE5, 0x53, 0x4B);
const BADGE_BG: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2D, 0x33);
const SEARCH_BG: egui::Color32 = egui::Color32::from_rgb(0x28, 0x2A, 0x30);
const SEARCH_BORDER: egui::Color32 = egui::Color32::from_rgb(0x3A, 0x3D, 0x45);
const SEARCH_FOCUS_BORDER: egui::Color32 = egui::Color32::from_rgb(0x58, 0xA6, 0xFF);
const KBD_BG: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x2D, 0x33);
const KBD_BORDER: egui::Color32 = egui::Color32::from_rgb(0x40, 0x44, 0x4D);

/// Persistent state for the popup picker UI.
pub struct PickerState {
    pub search_query: String,
    pub selected_index: usize,
    pub should_close: bool,
    pub paste_on_close: bool,
    first_frame: bool,
    image_textures: HashMap<usize, egui::TextureHandle>,
    prev_search_query: String,
    /// Index to delete (set by UI, consumed by render_picker after dropping history lock).
    delete_index: Option<usize>,
    /// List index hovered last frame (used to set bg fill before drawing).
    hovered_list_index: Option<usize>,
}

impl PickerState {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            selected_index: 0,
            should_close: false,
            paste_on_close: false,
            first_frame: true,
            image_textures: HashMap::new(),
            prev_search_query: String::new(),
            delete_index: None,
            hovered_list_index: None,
        }
    }

    pub fn reset(&mut self) {
        self.search_query.clear();
        self.selected_index = 0;
        self.should_close = false;
        self.paste_on_close = false;
        self.first_frame = true;
        self.image_textures.clear();
        self.prev_search_query.clear();
        self.delete_index = None;
        self.hovered_list_index = None;
    }
}

/// Render the full picker popup UI. Returns `Some(entry_index)` if user selected an item.
pub fn render_picker(
    ctx: &egui::Context,
    history: &Arc<Mutex<ClipboardHistory>>,
    dirty_flag: &Arc<AtomicBool>,
    state: &mut PickerState,
) -> Option<usize> {
    let mut copied_index: Option<usize> = None;

    apply_theme(ctx);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(BG_PRIMARY)
                .inner_margin(egui::Margin::symmetric(16, 14)),
        )
        .show(ctx, |ui| {
            let hist = match history.lock() {
                Ok(h) => h,
                Err(_) => return,
            };

            // -- Header --
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Clipboard icon
                ui.label(
                    egui::RichText::new("📋")
                        .size(18.0),
                );
                ui.label(
                    egui::RichText::new("Clipboard History")
                        .size(17.0)
                        .color(TEXT_PRIMARY)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Item count badge
                    let count_text = format!("{}", hist.len());
                    let badge_frame = egui::Frame::new()
                        .fill(BADGE_BG)
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::symmetric(8, 2));
                    badge_frame.show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(count_text)
                                .size(11.0)
                                .color(TEXT_SECONDARY)
                                .strong(),
                        );
                    });
                });
            });

            ui.add_space(10.0);

            // -- Filtered results (compute early so keyboard nav knows bounds) --
            let results = hist.search(&state.search_query);
            let result_count = results.len();

            // Reset hover tracking for this frame (will be set if pointer is over an item).
            let prev_hovered = state.hovered_list_index.take();

            // Reset selection when search query changes.
            if state.search_query != state.prev_search_query {
                state.selected_index = 0;
                state.prev_search_query = state.search_query.clone();
            }

            // Clamp selection.
            if result_count > 0 && state.selected_index >= result_count {
                state.selected_index = result_count - 1;
            }

            // -- Keyboard navigation (read raw events BEFORE TextEdit consumes them) --
            let mut enter_pressed = false;
            let mut keyboard_nav = false;
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    if result_count > 0 && state.selected_index + 1 < result_count {
                        state.selected_index += 1;
                        keyboard_nav = true;
                    }
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    if state.selected_index > 0 {
                        state.selected_index -= 1;
                        keyboard_nav = true;
                    }
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                    enter_pressed = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    state.should_close = true;
                }
                // Delete/Backspace with Cmd to remove selected item
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace)
                {
                    if result_count > 0 {
                        if let Some((orig_idx, _, _)) = results.get(state.selected_index) {
                            state.delete_index = Some(*orig_idx);
                        }
                    }
                }
            });

            // -- Search bar --
            ui.horizontal(|ui| {
                let search_frame = egui::Frame::new()
                    .fill(SEARCH_BG)
                    .corner_radius(8.0)
                    .stroke(egui::Stroke::new(1.0, SEARCH_BORDER))
                    .inner_margin(egui::Margin::symmetric(10, 6));

                search_frame.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("🔍")
                                .size(14.0),
                        );
                        let search_resp = ui.add(
                            egui::TextEdit::singleline(&mut state.search_query)
                                .hint_text(
                                    egui::RichText::new("Search clipboard history…")
                                        .color(TEXT_DIM)
                                        .size(13.5),
                                )
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::proportional(13.5))
                                .frame(false),
                        );
                        // Focus the search bar only on the first frame after popup opens.
                        if state.first_frame {
                            search_resp.request_focus();
                            state.first_frame = false;
                        }
                        // Update border color on focus
                        if search_resp.has_focus() {
                            let rect = ui.min_rect().expand(6.0);
                            ui.painter().rect_stroke(
                                rect,
                                8.0,
                                egui::Stroke::new(1.5, SEARCH_FOCUS_BORDER),
                                egui::StrokeKind::Outside,
                            );
                        }
                    });
                });
            });

            ui.add_space(8.0);

            // -- Results list --
            let available_height = ui.available_height() - 32.0; // reserve footer space
            egui::ScrollArea::vertical()
                .max_height(available_height)
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;

                    if results.is_empty() {
                        // Centered empty state
                        ui.add_space(available_height / 3.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("📭")
                                    .size(32.0),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(if state.search_query.is_empty() {
                                    "No clipboard history yet"
                                } else {
                                    "No matches found"
                                })
                                .color(TEXT_SECONDARY)
                                .size(14.0),
                            );
                            if !state.search_query.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("Try a different search term")
                                        .color(TEXT_DIM)
                                        .size(12.0),
                                );
                            }
                        });
                    } else {
                        for (list_idx, (orig_idx, entry, matched_indices)) in results.iter().enumerate() {
                            let is_selected = list_idx == state.selected_index;
                            let item_id = egui::Id::new(("clip_item", list_idx));

                            let is_hovered_last_frame = prev_hovered == Some(list_idx);

                            let bg = if is_selected {
                                BG_SELECTED
                            } else if is_hovered_last_frame {
                                BG_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let frame = egui::Frame::new()
                                .fill(bg)
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(10, 7));

                            let resp = frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;

                                    // Index number with fixed width
                                    let idx_text = format!("{}.", list_idx + 1);
                                    ui.label(
                                        egui::RichText::new(idx_text)
                                            .color(TEXT_DIM)
                                            .size(12.0)
                                            .monospace(),
                                    );

                                    match entry {
                                        ClipboardEntry::Image { rgba, width, height, .. } => {
                                            // Show thumbnail
                                            let tex = state.image_textures.entry(*orig_idx).or_insert_with(|| {
                                                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                                                    [*width as usize, *height as usize],
                                                    rgba,
                                                );
                                                ctx.load_texture(
                                                    format!("clip_img_{orig_idx}"),
                                                    color_image,
                                                    egui::TextureOptions::LINEAR,
                                                )
                                            });
                                            let thumb_h = 36.0;
                                            let aspect = *width as f32 / *height as f32;
                                            let thumb_w = thumb_h * aspect;
                                            // Thumbnail with rounded corners
                                            let img_rect = ui.allocate_exact_size(
                                                egui::vec2(thumb_w, thumb_h),
                                                egui::Sense::hover(),
                                            ).0;
                                            let uv = egui::Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            );
                                            ui.painter().image(
                                                tex.id(), img_rect, uv, egui::Color32::WHITE,
                                            );

                                            ui.label(
                                                egui::RichText::new(format!("{width}×{height}"))
                                                    .color(TEXT_SECONDARY)
                                                    .size(12.0),
                                            );
                                        }
                                        ClipboardEntry::Text(_) => {
                                            let label_text = entry.label(MAX_LABEL_CHARS);
                                            if matched_indices.is_empty() {
                                                ui.label(
                                                    egui::RichText::new(&label_text)
                                                        .color(if is_selected {
                                                            egui::Color32::WHITE
                                                        } else {
                                                            TEXT_PRIMARY
                                                        })
                                                        .size(13.5),
                                                );
                                            } else {
                                                // Highlight matched characters.
                                                let mut job = egui::text::LayoutJob::default();
                                                let normal_color = if is_selected {
                                                    egui::Color32::from_rgb(0xD0, 0xD8, 0xE0)
                                                } else {
                                                    TEXT_SECONDARY
                                                };
                                                let highlight_color = ACCENT_BLUE;

                                                for (ci, ch) in label_text.chars().enumerate() {
                                                    let color = if matched_indices.contains(&ci) {
                                                        highlight_color
                                                    } else {
                                                        normal_color
                                                    };
                                                    let mut s = String::new();
                                                    s.push(ch);
                                                    job.append(
                                                        &s,
                                                        0.0,
                                                        egui::TextFormat {
                                                            font_id: egui::FontId::proportional(13.5),
                                                            color,
                                                            ..Default::default()
                                                        },
                                                    );
                                                }
                                                ui.label(job);
                                            }
                                        }
                                    }

                                    // Spacer to push delete button right
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        // Only show delete button on hover or selection
                                        let show_delete = is_selected;
                                        if show_delete {
                                            let del_resp = ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("✕")
                                                        .size(11.0)
                                                        .color(TEXT_DIM),
                                                )
                                                .frame(false),
                                            );
                                            if del_resp.clicked() {
                                                state.delete_index = Some(*orig_idx);
                                            }
                                            if del_resp.hovered() {
                                                ui.painter().text(
                                                    del_resp.rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    "✕",
                                                    egui::FontId::proportional(11.0),
                                                    DELETE_HOVER,
                                                );
                                            }
                                        }
                                    });
                                });
                            });

                            // Track hover for next frame's bg fill
                            let item_rect = resp.response.rect;
                            if ui.rect_contains_pointer(item_rect) {
                                state.hovered_list_index = Some(list_idx);
                            }

                            // Click to select + copy — use a dedicated interact rect
                            let click_resp = ui.interact(item_rect, item_id, egui::Sense::click());
                            if click_resp.clicked() {
                                copied_index = Some(*orig_idx);
                            }
                            // Scroll selected item into view only on keyboard nav
                            if is_selected && keyboard_nav {
                                resp.response.scroll_to_me(Some(egui::Align::Center));
                            }

                            // Subtle separator between items
                            if list_idx + 1 < result_count {
                                let sep_rect = egui::Rect::from_min_size(
                                    egui::pos2(item_rect.left() + 10.0, item_rect.bottom() + 1.0),
                                    egui::vec2(item_rect.width() - 20.0, 1.0),
                                );
                                ui.painter().rect_filled(sep_rect, 0.0, BORDER_SUBTLE);
                            }
                        }
                    }
                });

            // Handle Enter press
            if enter_pressed && result_count > 0 {
                if let Some((orig_idx, _, _)) = results.get(state.selected_index) {
                    copied_index = Some(*orig_idx);
                }
            }

            // Drop the history lock before footer / delete.
            drop(hist);

            // Handle delete
            if let Some(del_idx) = state.delete_index.take() {
                if let Ok(mut hist) = history.lock() {
                    hist.remove(del_idx);
                    dirty_flag.store(true, Ordering::Release);
                    state.image_textures.clear();
                }
            }

            // -- Footer --
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    render_kbd(ui, "↑↓");
                    ui.label(egui::RichText::new("Navigate").size(11.0).color(TEXT_DIM));

                    ui.add_space(8.0);

                    render_kbd(ui, "Enter");
                    ui.label(egui::RichText::new("Paste").size(11.0).color(TEXT_DIM));

                    ui.add_space(8.0);

                    render_kbd(ui, "Del");
                    ui.label(egui::RichText::new("Delete").size(11.0).color(TEXT_DIM));

                    ui.add_space(8.0);

                    render_kbd(ui, "Esc");
                    ui.label(egui::RichText::new("Close").size(11.0).color(TEXT_DIM));
                });
            });
        });

    // Perform the copy outside the history lock.
    if let Some(idx) = copied_index {
        if let Ok(mut hist) = history.lock() {
            if let Some(entry) = hist.get(idx).cloned() {
                if let Err(e) = entry.copy_to_clipboard() {
                    eprintln!("Copy failed: {e}");
                } else {
                    hist.promote(idx);
                    dirty_flag.store(true, Ordering::Release);
                }
            }
        }
        state.should_close = true;
        state.paste_on_close = true;
    }

    copied_index
}

/// Render a keyboard shortcut badge.
fn render_kbd(ui: &mut egui::Ui, text: &str) {
    let frame = egui::Frame::new()
        .fill(KBD_BG)
        .corner_radius(4.0)
        .stroke(egui::Stroke::new(0.5, KBD_BORDER))
        .inner_margin(egui::Margin::symmetric(5, 1));
    frame.show(ui, |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(10.5)
                .color(TEXT_SECONDARY)
                .monospace(),
        );
    });
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG_PRIMARY;
    visuals.window_fill = BG_PRIMARY;
    visuals.extreme_bg_color = BG_SURFACE;
    visuals.widgets.inactive.bg_fill = BG_SURFACE;
    visuals.widgets.hovered.bg_fill = BG_HOVER;
    visuals.widgets.active.bg_fill = ACCENT_BLUE;
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x1A, 0x4A, 0x8A);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_BLUE);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    // Softer widget borders
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, BORDER_SUBTLE);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    ctx.set_visuals(visuals);
}
