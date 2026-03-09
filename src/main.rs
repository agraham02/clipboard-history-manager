use std::collections::{HashMap, VecDeque};
use std::time::{self, Duration};
use std::sync::{Arc, Mutex};
use arboard::Clipboard;
use tray_icon::menu::{MenuId, MenuItem};
use tray_icon::{MouseButtonState, TrayIcon, TrayIconEvent};
use tray_icon::{TrayIconBuilder, menu::Menu,Icon};
use winit::event_loop::{self, EventLoop};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

struct App {
    tray_icon: Option<tray_icon::TrayIcon>,
    clipboard_history: Arc<Mutex<VecDeque<String>>>,
    menuid_to_text: HashMap<MenuId, String>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let tray_menu = Menu::new();
        let icon = Icon::from_rgba(vec![255u8, 0u8, 0u8, 255u8].repeat(32 * 32), 32, 32);

        let new_tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("system-tray - tray icon library!")
        .with_icon(icon.unwrap())
        .build()
        .unwrap();

        self.tray_icon = Some(new_tray_icon);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // called when window events happen
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click { button_state, ..} => {
                    if button_state == MouseButtonState::Up {
                        println!("clicked");
                        let clipboard_history_copy = self.clipboard_history.lock().unwrap().clone();
                        let clipboard_items: Vec<_> = clipboard_history_copy
                            .iter()
                            .map(|item| {MenuItem::new(item, true, None)})
                            .collect();

                        let item_ref: Vec<&dyn tray_icon::menu::IsMenuItem> = clipboard_items
                            .iter()
                            .map(|item| item as &dyn tray_icon::menu::IsMenuItem)
                            .collect();

                        // We clear because everytime we rebuild the menu, the auto-assigned ids could be different
                        self.menuid_to_text.clear();
                        for (menu_item, text) in clipboard_items.iter().zip(clipboard_history_copy.iter()) {
                            self.menuid_to_text.insert(menu_item.id().clone(), text.to_string());
                        }

                        let new_menu = Menu::with_items(&item_ref).unwrap();
                        if let Some(tray_icon) = &mut self.tray_icon {
                            tray_icon.set_menu(Some(Box::new(new_menu)));
                        }
                    }
                },
                _ => {}
            }
        }

        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            println!("{:?}", event);
            if let Some(content) = self.menuid_to_text.get(event.id()){
                let mut os_clipboard = Clipboard::new().unwrap();
                os_clipboard.set_text(content).unwrap();
            }
        }
    }
}

fn main() {
    let interval_time = time::Duration::from_secs(2);

    println!("Hello, world!");

    // Arc let's multiple threads share ownership of the same data
    // Mutex let's only one thread access the data at a time (prevents race conditions)
    let clipboard_history: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    let clipboard_history_clone = clipboard_history.clone();
    poller_thread(clipboard_history_clone, interval_time);
    
    
    
    let event_loop = EventLoop::new().unwrap();
    let clipboard_history_clone2 = clipboard_history.clone();
    let mut app = App {tray_icon: None, clipboard_history: clipboard_history_clone2, menuid_to_text: HashMap::new()};
    event_loop.run_app(&mut app).unwrap();
     
        

        
    
    loop {
        // std::thread::sleep(interval_time);
        // println!("{:?}", clipboard_history.lock().unwrap());

        
    }


}

fn poller_thread(clipboard_history: Arc<Mutex<VecDeque<String>>>, interval_time: Duration) {
    // We clone so that the thread can always have a safe copy of the array, in case anything happens to the one in main
    std::thread::spawn(move || {
        let mut os_clipboard = Clipboard::new().unwrap();

        loop {
            std::thread::sleep(interval_time);
            
            let mut vec = clipboard_history.lock().unwrap();
            if let Ok(text) = os_clipboard.get_text() && !vec.contains(&text) {
                vec.push_front(text);
                println!("{:?}", vec);
            }
        }
    });
}

fn ui_thread() {

}