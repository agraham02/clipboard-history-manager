use std::time;
use std::sync::{Arc, Mutex};

fn main() {
    let interval_time = time::Duration::from_secs(2);

    println!("Hello, world!");

    // Arc let's multiple threads share ownership of the same data
    // Mutex let's only one thread access the data at a time (prevents race conditions)
    let clipboard_history: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // We clone so that the thread can always have a safe copy of the array, in case anything happens to the one in main
    let clipboard_history_clone = clipboard_history.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval_time);
    
            let mut vec = clipboard_history_clone.lock().unwrap();
            let str: String = "value".to_string();
            vec.push(str);
        }
    });
    
    loop {
        std::thread::sleep(time::Duration::from_secs(2));
        println!("{:?}", clipboard_history.lock().unwrap());
    }
}
