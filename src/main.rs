mod app;
mod clipboard;
mod history;
mod hotkey;
mod tray;
mod ui;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use winit::event_loop::EventLoop;

use crate::app::App;
use crate::history::ClipboardHistory;

const POLL_INTERVAL_SECONDS: u64 = 2;

fn main() {
    let history = Arc::new(Mutex::new(ClipboardHistory::new()));
    let dirty_flag = Arc::new(AtomicBool::new(false));

    // Spawn background clipboard poller.
    clipboard::spawn_poller(
        history.clone(),
        dirty_flag.clone(),
        Duration::from_secs(POLL_INTERVAL_SECONDS),
    );

    // Register global hotkey (Cmd+Option+V).
    let Some((_hotkey_manager, hotkey)) = hotkey::setup_hotkey() else {
        eprintln!("Cannot start without global hotkey support");
        return;
    };

    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("Failed to create event loop: {e}");
            return;
        }
    };

    let mut app = App::new(history, dirty_flag, hotkey);

    // _hotkey_manager must stay alive for the hotkey to work — move it into app.
    app.set_hotkey_manager(_hotkey_manager);

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("Event loop exited with error: {e}");
    }
}