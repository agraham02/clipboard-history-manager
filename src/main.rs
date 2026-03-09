use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::Clipboard;
use tray_icon::menu::{MenuId, MenuItem};
use tray_icon::{menu::Menu, Icon, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoop;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

const POLL_INTERVAL_SECONDS: u64 = 2;
const MAX_HISTORY_ITEMS: usize = 30;
const MAX_MENU_LABEL_CHARS: usize = 70;

// Tray menu labels should be short and single-line, so this normalizes and truncates text.
fn menu_label_for(text: &str) -> String {
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
        "(empty)".to_string()
    } else {
        result
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
    // Maps runtime-generated menu IDs back to full clipboard text.
    menuid_to_text: HashMap<MenuId, String>,
    clear_menu_id: Option<MenuId>,
    quit_menu_id: Option<MenuId>,
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
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
        // called when window events happen
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Non-blocking drain of tray events so UI stays responsive.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { button_state, .. } => {
                    if button_state == MouseButtonState::Up {
                        let clipboard_history_copy = match self.clipboard_history.lock() {
                            Ok(lock) => lock.clone(),
                            Err(err) => {
                                eprintln!("Clipboard history lock poisoned: {err}");
                                continue;
                            }
                        };

                        let clipboard_items: Vec<_> = clipboard_history_copy
                            .iter()
                            .map(|item| MenuItem::new(menu_label_for(item), true, None))
                            .collect();

                        let clear_item = MenuItem::new("Clear History", true, None);
                        let quit_item = MenuItem::new("Quit", true, None);

                        self.clear_menu_id = Some(clear_item.id().clone());
                        self.quit_menu_id = Some(quit_item.id().clone());

                        let mut item_ref: Vec<&dyn tray_icon::menu::IsMenuItem> = clipboard_items
                            .iter()
                            .map(|item| item as &dyn tray_icon::menu::IsMenuItem)
                            .collect();

                        item_ref.push(&clear_item);
                        item_ref.push(&quit_item);

                        // IDs are regenerated on rebuild, so old mappings become stale.
                        self.menuid_to_text.clear();
                        for (menu_item, text) in clipboard_items.iter().zip(clipboard_history_copy.iter()) {
                            self.menuid_to_text.insert(menu_item.id().clone(), text.to_string());
                        }

                        let Ok(new_menu) = Menu::with_items(&item_ref) else {
                            eprintln!("Failed to build tray menu");
                            continue;
                        };

                        if let Some(tray_icon) = &mut self.tray_icon {
                            tray_icon.set_menu(Some(Box::new(new_menu)));
                        }
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

    let clipboard_history_clone = clipboard_history.clone();
    poller_thread(clipboard_history_clone, interval_time);

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
        menuid_to_text: HashMap::new(),
        clear_menu_id: None,
        quit_menu_id: None,
    };

    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("Application event loop exited with error: {err}");
    }
}

fn poller_thread(clipboard_history: Arc<Mutex<VecDeque<String>>>, interval_time: Duration) {
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
        }
    });
}