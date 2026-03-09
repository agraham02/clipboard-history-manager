use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

/// Register Cmd+Shift+V as the global hotkey for opening the clipboard picker.
pub fn setup_hotkey() -> Option<(GlobalHotKeyManager, HotKey)> {
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to initialize global hotkey manager: {e}");
            return None;
        }
    };

    let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
    if let Err(e) = manager.register(hotkey) {
        eprintln!("Failed to register Cmd+Shift+V hotkey: {e}");
        return None;
    }

    Some((manager, hotkey))
}
