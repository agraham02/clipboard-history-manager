use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use softbuffer::Surface;
use tray_icon::menu::{MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{menu::Menu, Icon, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoop;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const POLL_INTERVAL_SECONDS: u64 = 2;
const HOTKEY_DEBOUNCE_MS: u64 = 250;
const MAX_HISTORY_ITEMS: usize = 50;
const MAX_MENU_LABEL_CHARS: usize = 70;
const POPUP_WIDTH: usize = 760;
const POPUP_HEIGHT: usize = 420;
const POPUP_VISIBLE_ITEMS: usize = 12;
const POPUP_LABEL_CHARS: usize = 95;

// Tray menu labels should be short and single-line, so this normalizes and truncates text.
fn menu_label_for(index: usize, text: &str) -> String {
    let single_line = text.replace('\n', " ").replace('\r', " ");
    let mut result = String::new();
    for (idx, ch) in single_line.chars().enumerate() {
        if idx >= MAX_MENU_LABEL_CHARS {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }

    if result.is_empty() {
        format!("{}. (empty)", index + 1)
    } else {
        format!("{}. {}", index + 1, result)
    }
}

// Keep history unique and bounded: newest first, older duplicates removed.
fn push_history(history: &mut VecDeque<String>, text: String) {
    if let Some(pos) = history.iter().position(|item| item == &text) {
        history.remove(pos);
    }

    history.push_front(text);
    while history.len() > MAX_HISTORY_ITEMS {
        history.pop_back();
    }
}

struct App {
    tray_icon: Option<TrayIcon>,
    // Shared with the poller thread; guarded by Mutex for safe mutable access.
    clipboard_history: Arc<Mutex<VecDeque<String>>>,
    menu_dirty: Arc<AtomicBool>,
    // Maps runtime-generated menu IDs back to full clipboard text.
    menuid_to_text: HashMap<MenuId, String>,
    clear_menu_id: Option<MenuId>,
    quit_menu_id: Option<MenuId>,
    _hotkey_manager: GlobalHotKeyManager,
    open_picker_hotkey: HotKey,
    last_hotkey_triggered_at: Option<Instant>,
    popup_window: Option<Arc<Window>>,
    popup_surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    popup_selected_index: usize,
    popup_scroll_offset: usize,
}

struct BuiltMenu {
    menu: Menu,
    menuid_to_text: HashMap<MenuId, String>,
    clear_menu_id: MenuId,
    quit_menu_id: MenuId,
}

fn refresh_tray_menu(app: &mut App) {
    let clipboard_history_copy = match app.clipboard_history.lock() {
        Ok(lock) => lock.clone(),
        Err(err) => {
            eprintln!("Clipboard history lock poisoned: {err}");
            return;
        }
    };

    let Some(built_menu) = build_tray_menu(&clipboard_history_copy) else {
        return;
    };

    app.menuid_to_text = built_menu.menuid_to_text;
    app.clear_menu_id = Some(built_menu.clear_menu_id);
    app.quit_menu_id = Some(built_menu.quit_menu_id);

    if let Some(tray_icon) = &mut app.tray_icon {
        tray_icon.set_menu(Some(Box::new(built_menu.menu)));
    }
}

fn popup_label_for(index: usize, text: &str) -> String {
    let single_line = text.replace('\n', " ").replace('\r', " ");
    let mut result = String::new();
    for (idx, ch) in single_line.chars().enumerate() {
        if idx >= POPUP_LABEL_CHARS {
            result.push_str("...");
            break;
        }
        result.push(ch);
    }

    if result.is_empty() {
        format!("{}. (empty)", index + 1)
    } else {
        format!("{}. {}", index + 1, result)
    }
}

fn draw_rect(buffer: &mut [u32], width: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    let max_y = (y + h).min(POPUP_HEIGHT);
    let max_x = (x + w).min(width);
    for py in y..max_y {
        let row = py * width;
        for px in x..max_x {
            buffer[row + px] = color;
        }
    }
}

fn draw_text(buffer: &mut [u32], width: usize, x: usize, y: usize, color: u32, text: &str) {
    for (idx, ch) in text.chars().enumerate() {
        let x_offset = x + idx * 8;
        if x_offset + 8 > width {
            break;
        }

        let glyph = font8x8::UnicodeFonts::get(&font8x8::BASIC_FONTS, ch)
            .or_else(|| font8x8::UnicodeFonts::get(&font8x8::BASIC_FONTS, '?'));

        let Some(bitmap) = glyph else {
            continue;
        };

        for (row, byte) in bitmap.iter().enumerate() {
            let py = y + row;
            if py >= POPUP_HEIGHT {
                continue;
            }
            let row_base = py * width;
            for bit in 0..8 {
                if (byte >> bit) & 1 == 1 {
                    let px = x_offset + bit;
                    if px < width {
                        buffer[row_base + px] = color;
                    }
                }
            }
        }
    }
}

fn render_popup(
    surface: &mut Surface<Arc<Window>, Arc<Window>>,
    clipboard_history: &Arc<Mutex<VecDeque<String>>>,
    selected_index: usize,
    scroll_offset: usize,
) {
    let history_snapshot = match clipboard_history.lock() {
        Ok(lock) => lock.clone(),
        Err(err) => {
            eprintln!("Clipboard history lock poisoned: {err}");
            return;
        }
    };

    let history_len = history_snapshot.len();

    let mut buffer = surface.buffer_mut().unwrap();
    let buffer_slice = buffer.as_mut();

    buffer_slice.fill(0x171a20);
    draw_rect(buffer_slice, POPUP_WIDTH, 0, 0, POPUP_WIDTH, 34, 0x222831);
    draw_text(
        buffer_slice,
        POPUP_WIDTH,
        12,
        12,
        0xE8EEF2,
        "Clipboard History  -  Up/Down: navigate, Enter: copy, Esc: close",
    );

    if history_len == 0 {
        draw_text(buffer_slice, POPUP_WIDTH, 16, 56, 0xB0BAC5, "No clipboard history yet");
    } else {
        for visible_idx in 0..POPUP_VISIBLE_ITEMS {
            let history_idx = scroll_offset + visible_idx;
            if history_idx >= history_len {
                break;
            }

            let y = 48 + visible_idx * 28;
            let is_selected = history_idx == selected_index;
            if is_selected {
                draw_rect(buffer_slice, POPUP_WIDTH, 8, y - 4, POPUP_WIDTH - 16, 24, 0x2D425A);
            }

            let label = popup_label_for(history_idx, &history_snapshot[history_idx]);
            let color = if is_selected { 0xFFFFFF } else { 0xD3D9E0 };
            draw_text(buffer_slice, POPUP_WIDTH, 16, y, color, &label);
        }

        let footer = format!("{} items total", history_len);
        draw_text(buffer_slice, POPUP_WIDTH, 16, POPUP_HEIGHT - 24, 0x90A0B0, &footer);
    }

    let _ = buffer.present();
}

fn build_tray_menu(history: &VecDeque<String>) -> Option<BuiltMenu> {
    let title_item = MenuItem::new("Clipboard History", false, None);
    let divider_top = PredefinedMenuItem::separator();
    let divider_bottom = PredefinedMenuItem::separator();

    let mut history_items = Vec::new();
    let mut menuid_to_text = HashMap::new();

    if history.is_empty() {
        history_items.push(MenuItem::new("No clipboard history yet", false, None));
    } else {
        for (index, text) in history.iter().enumerate() {
            let item = MenuItem::new(menu_label_for(index, text), true, None);
            menuid_to_text.insert(item.id().clone(), text.clone());
            history_items.push(item);
        }
    }

    let clear_item = MenuItem::new("Clear History", !history.is_empty(), None);
    let quit_item = MenuItem::new("Quit", true, None);

    let mut item_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = Vec::new();
    item_refs.push(&title_item);
    item_refs.push(&divider_top);
    for item in &history_items {
        item_refs.push(item);
    }
    item_refs.push(&divider_bottom);
    item_refs.push(&clear_item);
    item_refs.push(&quit_item);

    let menu = match Menu::with_items(&item_refs) {
        Ok(menu) => menu,
        Err(err) => {
            eprintln!("Failed to build tray menu: {err}");
            return None;
        }
    };

    Some(BuiltMenu {
        menu,
        menuid_to_text,
        clear_menu_id: clear_item.id().clone(),
        quit_menu_id: quit_item.id().clone(),
    })
}

impl App {
    fn open_popup_window(&mut self, event_loop: &ActiveEventLoop) {
        let window_attrs = Window::default_attributes()
            .with_title("Clipboard History Picker")
            .with_inner_size(LogicalSize::new(POPUP_WIDTH as u32, POPUP_HEIGHT as u32))
            .with_resizable(false);

        let window = match event_loop.create_window(window_attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("Failed to create popup window: {err}");
                return;
            }
        };

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();
        surface
            .resize(
                NonZeroU32::new(POPUP_WIDTH as u32).unwrap(),
                NonZeroU32::new(POPUP_HEIGHT as u32).unwrap(),
            )
            .unwrap();

        self.popup_selected_index = 0;
        self.popup_scroll_offset = 0;

        render_popup(
            &mut surface,
            &self.clipboard_history,
            self.popup_selected_index,
            self.popup_scroll_offset,
        );

        self.popup_window = Some(window);
        self.popup_surface = Some(surface);
    }

    fn update_popup_scroll(&mut self) {
        if self.popup_selected_index < self.popup_scroll_offset {
            self.popup_scroll_offset = self.popup_selected_index;
        }
        if self.popup_selected_index >= self.popup_scroll_offset + POPUP_VISIBLE_ITEMS {
            self.popup_scroll_offset = self.popup_selected_index + 1 - POPUP_VISIBLE_ITEMS;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        let tray_menu = Menu::new();
        let icon = Icon::from_rgba(vec![255u8, 0u8, 0u8, 255u8].repeat(32 * 32), 32, 32);

        let Ok(icon) = icon else {
            eprintln!("Failed to build tray icon image");
            return;
        };

        let new_tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("Clipboard History Manager")
            .with_icon(icon)
            .build();

        let Ok(new_tray_icon) = new_tray_icon else {
            eprintln!("Failed to create tray icon");
            return;
        };

        self.tray_icon = Some(new_tray_icon);
        refresh_tray_menu(self);
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.popup_window.as_ref().is_some_and(|w| w.id() == id) {
            match event {
                WindowEvent::CloseRequested => {
                    self.popup_window = None;
                    self.popup_surface = None;
                }
                WindowEvent::KeyboardInput { event: key_event, .. } => {
                    if key_event.state == ElementState::Pressed {
                        let history_len = self.clipboard_history.lock().ok().map(|h| h.len()).unwrap_or(0);
                        let mut should_render = false;
                        let mut should_close = false;
                        let mut should_copy = false;

                        match key_event.physical_key {
                            PhysicalKey::Code(KeyCode::ArrowUp) if history_len > 0 => {
                                if self.popup_selected_index > 0 {
                                    self.popup_selected_index -= 1;
                                    self.update_popup_scroll();
                                    should_render = true;
                                }
                            }
                            PhysicalKey::Code(KeyCode::ArrowDown) if history_len > 0 => {
                                if self.popup_selected_index + 1 < history_len {
                                    self.popup_selected_index += 1;
                                    self.update_popup_scroll();
                                    should_render = true;
                                }
                            }
                            PhysicalKey::Code(KeyCode::Enter) if history_len > 0 => {
                                should_copy = true;
                                should_close = true;
                            }
                            PhysicalKey::Code(KeyCode::Escape) => {
                                should_close = true;
                            }
                            _ => {}
                        }

                        if should_copy {
                            if let Ok(history) = self.clipboard_history.lock() {
                                if let Some(selected_text) = history.get(self.popup_selected_index).cloned() {
                                    drop(history);
                                    if let Ok(mut clipboard) = Clipboard::new() {
                                        let _ = clipboard.set_text(selected_text.clone());
                                        if let Ok(mut history) = self.clipboard_history.lock() {
                                            push_history(&mut history, selected_text);
                                            self.menu_dirty.store(true, Ordering::Release);
                                        }
                                    }
                                }
                            }
                        }

                        if should_render {
                            if let Some(surface) = &mut self.popup_surface {
                                render_popup(
                                    surface,
                                    &self.clipboard_history,
                                    self.popup_selected_index,
                                    self.popup_scroll_offset,
                                );
                            }
                        }

                        if should_close {
                            self.popup_window = None;
                            self.popup_surface = None;
                        }
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let Some(surface) = &mut self.popup_surface {
                        render_popup(
                            surface,
                            &self.clipboard_history,
                            self.popup_selected_index,
                            self.popup_scroll_offset,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.menu_dirty.swap(false, Ordering::AcqRel) {
            refresh_tray_menu(self);
        }

        if self.popup_window.is_some() {
            let history_len = self.clipboard_history.lock().ok().map(|h| h.len()).unwrap_or(0);
            if history_len > 0 && self.popup_selected_index >= history_len {
                self.popup_selected_index = history_len - 1;
            }
            if let Some(window) = &self.popup_window {
                window.request_redraw();
            }
        }

        while let Ok(hotkey_event) = GlobalHotKeyEvent::receiver().try_recv() {
            if hotkey_event.id == self.open_picker_hotkey.id()
                && hotkey_event.state == HotKeyState::Pressed
            {
                let should_skip = self
                    .last_hotkey_triggered_at
                    .is_some_and(|last| last.elapsed().as_millis() < HOTKEY_DEBOUNCE_MS as u128);
                if should_skip {
                    continue;
                }
                self.last_hotkey_triggered_at = Some(Instant::now());

                if self.popup_window.is_none() {
                    self.open_popup_window(event_loop);
                }
            }
        }

        // Non-blocking drain of tray events so UI stays responsive.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { button_state, .. } => {
                    if button_state == MouseButtonState::Down {
                        refresh_tray_menu(self);
                    }
                }
                _ => {}
            }
        }

        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if self.quit_menu_id.as_ref().is_some_and(|id| id == event.id()) {
                event_loop.exit();
                continue;
            }

            if self.clear_menu_id.as_ref().is_some_and(|id| id == event.id()) {
                if let Ok(mut history) = self.clipboard_history.lock() {
                    history.clear();
                    self.menu_dirty.store(true, Ordering::Release);
                } else {
                    eprintln!("Failed to clear history due to poisoned lock");
                }
                continue;
            }

            if let Some(content) = self.menuid_to_text.get(event.id()) {
                let mut os_clipboard = match Clipboard::new() {
                    Ok(clipboard) => clipboard,
                    Err(err) => {
                        eprintln!("Failed to access clipboard: {err}");
                        continue;
                    }
                };

                if let Err(err) = os_clipboard.set_text(content.clone()) {
                    eprintln!("Failed to write clipboard text: {err}");
                }
            }
        }
    }
}

fn main() {
    let interval_time = Duration::from_secs(POLL_INTERVAL_SECONDS);

    // One background thread writes history while the UI thread reads/clears it.
    // Arc shares ownership across threads; Mutex serializes mutation.
    let clipboard_history: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let menu_dirty = Arc::new(AtomicBool::new(true));

    let clipboard_history_clone = clipboard_history.clone();
    let menu_dirty_clone = menu_dirty.clone();
    poller_thread(clipboard_history_clone, menu_dirty_clone, interval_time);

    let hotkey_manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            eprintln!("Failed to initialize global hotkey manager: {err}");
            return;
        }
    };

    let open_picker_hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
    if let Err(err) = hotkey_manager.register(open_picker_hotkey) {
        eprintln!("Failed to register Cmd+Shift+V hotkey: {err}");
    }

    let event_loop = match EventLoop::new() {
        Ok(loop_) => loop_,
        Err(err) => {
            eprintln!("Failed to create event loop: {err}");
            return;
        }
    };

    let clipboard_history_clone2 = clipboard_history.clone();
    let mut app = App {
        tray_icon: None,
        clipboard_history: clipboard_history_clone2,
        menu_dirty,
        menuid_to_text: HashMap::new(),
        clear_menu_id: None,
        quit_menu_id: None,
        _hotkey_manager: hotkey_manager,
        open_picker_hotkey,
        last_hotkey_triggered_at: None,
        popup_window: None,
        popup_surface: None,
        popup_selected_index: 0,
        popup_scroll_offset: 0,
    };

    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("Application event loop exited with error: {err}");
    }
}

fn poller_thread(
    clipboard_history: Arc<Mutex<VecDeque<String>>>,
    menu_dirty: Arc<AtomicBool>,
    interval_time: Duration,
) {
    // Spawn a detached worker that periodically snapshots OS clipboard text.
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval_time);

            let mut os_clipboard = match Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(err) => {
                    eprintln!("Clipboard unavailable: {err}");
                    continue;
                }
            };

            let text = match os_clipboard.get_text() {
                Ok(text) => text,
                Err(_) => continue,
            };

            let mut history = match clipboard_history.lock() {
                Ok(lock) => lock,
                Err(err) => {
                    eprintln!("Clipboard history lock poisoned: {err}");
                    continue;
                }
            };

            if history.front().is_some_and(|top| top == &text) {
                continue;
            }

            push_history(&mut history, text);
            menu_dirty.store(true, Ordering::Release);
        }
    });
}