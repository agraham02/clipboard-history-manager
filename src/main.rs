use std::collections::VecDeque;
use std::time;
use std::sync::{Arc, Mutex};
use arboard::Clipboard;

fn main() {
    let interval_time = time::Duration::from_secs(2);

    println!("Hello, world!");

    // Arc let's multiple threads share ownership of the same data
    // Mutex let's only one thread access the data at a time (prevents race conditions)
    let clipboard_history: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    // We clone so that the thread can always have a safe copy of the array, in case anything happens to the one in main
    let clipboard_history_clone = clipboard_history.clone();
    std::thread::spawn(move || {
        let mut my_clipboard = Clipboard::new().unwrap();

        loop {
            std::thread::sleep(interval_time);
            
            let mut vec = clipboard_history_clone.lock().unwrap();
            if let Ok(text) = my_clipboard.get_text() && !vec.contains(&text) {
                vec.push_front(text);
            }
        }
    });
    
    loop {
        std::thread::sleep(time::Duration::from_secs(2));
        println!("{:?}", clipboard_history.lock().unwrap());
    }
}
