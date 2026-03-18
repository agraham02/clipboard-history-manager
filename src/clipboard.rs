use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::{Clipboard, ImageData};

use crate::history::ClipboardHistory;

/// A single clipboard entry — either copied text or an image.
#[derive(Clone)]
pub enum ClipboardEntry {
    Text(String),
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl PartialEq for ClipboardEntry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Image { rgba: a, width: aw, height: ah }, Self::Image { rgba: b, width: bw, height: bh }) => {
                aw == bw && ah == bh && a == b
            }
            _ => false,
        }
    }
}

impl fmt::Display for ClipboardEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(s) => write!(f, "{s}"),
            Self::Image { width, height, .. } => write!(f, "[Image {width}×{height}]"),
        }
    }
}

impl ClipboardEntry {
    /// Returns text content for searching, or a placeholder for images.
    pub fn searchable_text(&self) -> &str {
        match self {
            Self::Text(s) => s.as_str(),
            Self::Image { .. } => "(image)",
        }
    }

    /// Returns a single-line summary truncated to `max_chars`.
    pub fn label(&self, max_chars: usize) -> String {
        match self {
            Self::Text(s) => {
                let single_line: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
                if single_line.chars().count() > max_chars {
                    let truncated: String = single_line.chars().take(max_chars).collect();
                    format!("{truncated}…")
                } else if single_line.is_empty() {
                    "(empty)".to_string()
                } else {
                    single_line
                }
            }
            Self::Image { width, height, .. } => format!("Image {width}×{height}"),
        }
    }

    /// Write this entry back to the system clipboard.
    pub fn copy_to_clipboard(&self) -> Result<(), String> {
        let mut cb = Clipboard::new().map_err(|e| format!("Clipboard access failed: {e}"))?;
        match self {
            Self::Text(s) => cb.set_text(s.clone()).map_err(|e| format!("Failed to set text: {e}")),
            Self::Image { rgba, width, height } => {
                let img = ImageData {
                    bytes: rgba.clone().into(),
                    width: *width as usize,
                    height: *height as usize,
                };
                cb.set_image(img).map_err(|e| format!("Failed to set image: {e}"))
            }
        }
    }
}

const MAX_IMAGE_DIM: u32 = 4096;

/// Downscale an image if either dimension exceeds MAX_IMAGE_DIM, preserving aspect ratio.
/// Normal screenshots are stored at full resolution; only absurdly large images are capped.
fn downscale_if_needed(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    if width <= MAX_IMAGE_DIM && height <= MAX_IMAGE_DIM {
        return (rgba.to_vec(), width, height);
    }

    let scale = f64::min(
        MAX_IMAGE_DIM as f64 / width as f64,
        MAX_IMAGE_DIM as f64 / height as f64,
    );
    let new_w = (width as f64 * scale).round().max(1.0) as u32;
    let new_h = (height as f64 * scale).round().max(1.0) as u32;

    let mut out = vec![0u8; (new_w * new_h * 4) as usize];
    for y in 0..new_h {
        for x in 0..new_w {
            let src_x = ((x as f64 / new_w as f64) * width as f64).min((width - 1) as f64) as u32;
            let src_y = ((y as f64 / new_h as f64) * height as f64).min((height - 1) as f64) as u32;
            let si = (src_y * width + src_x) as usize * 4;
            let di = (y * new_w + x) as usize * 4;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    (out, new_w, new_h)
}

/// Spawns a background thread that polls the system clipboard for new entries.
pub fn spawn_poller(
    history: Arc<Mutex<ClipboardHistory>>,
    dirty_flag: Arc<AtomicBool>,
    interval: Duration,
) {
    std::thread::spawn(move || {
        let mut last_text: Option<String> = None;
        let mut last_image_hash: Option<u64> = None;
        loop {
            std::thread::sleep(interval);

            let mut cb = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Clipboard unavailable: {e}");
                    continue;
                }
            };

            // Try text first
            if let Ok(text) = cb.get_text() {
                if !text.is_empty() && last_text.as_deref() != Some(&text) {
                    last_text = Some(text.clone());
                    if let Ok(mut h) = history.lock() {
                        h.push(ClipboardEntry::Text(text));
                        dirty_flag.store(true, Ordering::Release);
                    }
                    continue;
                }
            }

            // Then try image
            if let Ok(img) = cb.get_image() {
                // Simple hash to detect change without storing full previous image.
                let hash = quick_hash(&img.bytes);
                if last_image_hash != Some(hash) {
                    last_image_hash = Some(hash);
                    let (rgba, w, h) = downscale_if_needed(
                        &img.bytes,
                        img.width as u32,
                        img.height as u32,
                    );
                    if let Ok(mut hist) = history.lock() {
                        hist.push(ClipboardEntry::Image { rgba, width: w, height: h });
                        dirty_flag.store(true, Ordering::Release);
                    }
                }
            }
        }
    });
}

fn quick_hash(data: &[u8]) -> u64 {
    // FNV-1a-ish hash — not cryptographic, just for change detection.
    let mut h: u64 = 0xcbf29ce484222325;
    // Sample every 128th byte for speed on large images.
    let step = (data.len() / 1024).max(1);
    for &b in data.iter().step_by(step) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
