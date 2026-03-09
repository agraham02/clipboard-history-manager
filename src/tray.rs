use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct TrayState {
    #[allow(dead_code)]
    pub tray_icon: TrayIcon,
    pub show_menu_id: MenuId,
    pub clear_menu_id: MenuId,
    pub quit_menu_id: MenuId,
}

/// Build the tray icon and its simplified menu.
pub fn create_tray() -> Option<TrayState> {
    let show_item = MenuItem::new("Show Clipboard History (⌘⇧V)", true, None);
    let clear_item = MenuItem::new("Clear History", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let separator = PredefinedMenuItem::separator();
    let item_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = vec![
        &show_item,
        &separator,
        &clear_item,
        &quit_item,
    ];

    let menu = match Menu::with_items(&item_refs) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to build tray menu: {e}");
            return None;
        }
    };

    // 16×16 clipboard icon (simple white clipboard shape on transparent background).
    let icon = build_clipboard_icon();

    let tray_icon = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Clipboard History Manager")
        .with_icon(icon)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to create tray icon: {e}");
            return None;
        }
    };

    Some(TrayState {
        tray_icon,
        show_menu_id: show_item.id().clone(),
        clear_menu_id: clear_item.id().clone(),
        quit_menu_id: quit_item.id().clone(),
    })
}

/// Build a simple 22×22 clipboard icon programmatically.
fn build_clipboard_icon() -> Icon {
    let size: u32 = 22;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let set_pixel = |buf: &mut Vec<u8>, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8| {
        let idx = ((y * size + x) * 4) as usize;
        buf[idx] = r;
        buf[idx + 1] = g;
        buf[idx + 2] = b;
        buf[idx + 3] = a;
    };

    // Draw a rounded clipboard shape — white on transparent.
    for y in 3..20 {
        for x in 4..18 {
            set_pixel(&mut rgba, x, y, 220, 225, 230, 255);
        }
    }
    // Clip at top center
    for x in 8..14 {
        set_pixel(&mut rgba, x, 1, 200, 205, 210, 255);
        set_pixel(&mut rgba, x, 2, 200, 205, 210, 255);
    }
    // Lines on the clipboard
    for x in 6..16 {
        for &y in &[7u32, 10, 13, 16] {
            set_pixel(&mut rgba, x, y, 100, 110, 120, 255);
        }
    }

    Icon::from_rgba(rgba, size, size).unwrap_or_else(|_| {
        // Fallback: solid white square
        Icon::from_rgba(vec![255u8; (size * size * 4) as usize], size, size).unwrap()
    })
}
