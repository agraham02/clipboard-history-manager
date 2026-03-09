use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::clipboard::ClipboardEntry;
use crate::history::ClipboardHistory;

const MAX_LABEL_CHARS: usize = 90;

/// Persistent state for the popup picker UI.
pub struct PickerState {
    pub search_query: String,
    pub selected_index: usize,
    pub should_close: bool,
    image_textures: HashMap<usize, egui::TextureHandle>,
}

impl PickerState {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            selected_index: 0,
            should_close: false,
            image_textures: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.search_query.clear();
        self.selected_index = 0;
        self.should_close = false;
        self.image_textures.clear();
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
                .fill(egui::Color32::from_rgb(0x1E, 0x1E, 0x1E))
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            let hist = match history.lock() {
                Ok(h) => h,
                Err(_) => return,
            };

            // -- Header --
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Clipboard History")
                        .size(16.0)
                        .color(egui::Color32::from_rgb(0xE8, 0xEE, 0xF2))
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} items", hist.len()))
                            .size(12.0)
                            .color(egui::Color32::from_rgb(0x90, 0xA0, 0xB0)),
                    );
                });
            });

            ui.add_space(8.0);

            // -- Search bar --
            let search_resp = ui.add(
                egui::TextEdit::singleline(&mut state.search_query)
                    .hint_text("Search…")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Body),
            );
            // Auto-focus search bar on first frame.
            if search_resp.gained_focus() || ctx.input(|i| i.key_pressed(egui::Key::Slash)) {
                search_resp.request_focus();
            }
            // Make sure it gets focus when popup opens.
            if state.search_query.is_empty() && !search_resp.has_focus() {
                search_resp.request_focus();
            }

            ui.add_space(6.0);

            // -- Filtered results --
            let results = hist.search(&state.search_query);
            let result_count = results.len();

            // Clamp selection.
            if result_count > 0 && state.selected_index >= result_count {
                state.selected_index = result_count - 1;
            }

            // -- Keyboard navigation (consume before rendering list) --
            let mut enter_pressed = false;
            ctx.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) {
                    if result_count > 0 && state.selected_index + 1 < result_count {
                        state.selected_index += 1;
                    }
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    if state.selected_index > 0 {
                        state.selected_index -= 1;
                    }
                }
                if i.key_pressed(egui::Key::Enter) {
                    enter_pressed = true;
                }
                if i.key_pressed(egui::Key::Escape) {
                    state.should_close = true;
                }
            });

            // -- Results list --
            let available_height = ui.available_height() - 28.0; // reserve footer space
            egui::ScrollArea::vertical()
                .max_height(available_height)
                .auto_shrink(false)
                .show(ui, |ui| {
                    if results.is_empty() {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new(if state.search_query.is_empty() {
                                "No clipboard history yet"
                            } else {
                                "No matches"
                            })
                            .color(egui::Color32::from_rgb(0xB0, 0xBA, 0xC5))
                            .size(13.0),
                        );
                    } else {
                        for (list_idx, (orig_idx, entry, matched_indices)) in results.iter().enumerate() {
                            let is_selected = list_idx == state.selected_index;

                            let bg = if is_selected {
                                egui::Color32::from_rgb(0x0A, 0x54, 0xA0)
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let frame = egui::Frame::new()
                                .fill(bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 4));

                            let resp = frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Index number
                                    ui.label(
                                        egui::RichText::new(format!("{}.", orig_idx + 1))
                                            .color(egui::Color32::from_rgb(0x80, 0x8A, 0x94))
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
                                            let thumb_h = 32.0;
                                            let aspect = *width as f32 / *height as f32;
                                            let thumb_w = thumb_h * aspect;
                                            ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(thumb_w, thumb_h)));

                                            ui.label(
                                                egui::RichText::new(format!("{width}×{height}"))
                                                    .color(egui::Color32::from_rgb(0xB0, 0xBA, 0xC5))
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
                                                            egui::Color32::from_rgb(0xD3, 0xD9, 0xE0)
                                                        })
                                                        .size(13.0),
                                                );
                                            } else {
                                                // Highlight matched characters.
                                                let mut job = egui::text::LayoutJob::default();
                                                let normal_color = if is_selected {
                                                    egui::Color32::from_rgb(0xD0, 0xD8, 0xE0)
                                                } else {
                                                    egui::Color32::from_rgb(0xB0, 0xBA, 0xC5)
                                                };
                                                let highlight_color = egui::Color32::from_rgb(0x58, 0xC4, 0xFF);

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
                                                            font_id: egui::FontId::proportional(13.0),
                                                            color,
                                                            ..Default::default()
                                                        },
                                                    );
                                                }
                                                ui.label(job);
                                            }
                                        }
                                    }
                                });
                            });

                            // Click to select + copy
                            if resp.response.interact(egui::Sense::click()).clicked() {
                                copied_index = Some(*orig_idx);
                            }

                            // Scroll selected item into view
                            if is_selected {
                                resp.response.scroll_to_me(Some(egui::Align::Center));
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

            // Drop the history lock before footer.
            drop(hist);

            // -- Footer --
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.label(
                    egui::RichText::new("↑↓ Navigate  •  Enter Copy  •  Esc Close")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(0x70, 0x7A, 0x84)),
                );
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
    }

    copied_index
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
    visuals.window_fill = egui::Color32::from_rgb(0x1E, 0x1E, 0x1E);
    visuals.extreme_bg_color = egui::Color32::from_rgb(0x2A, 0x2A, 0x2A);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(0x2D, 0x2D, 0x2D);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(0x3A, 0x3A, 0x3A);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0x0A, 0x84, 0xFF);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x0A, 0x84, 0xFF);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    ctx.set_visuals(visuals);
}
