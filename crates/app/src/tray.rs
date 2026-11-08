use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
pub fn run(port: u16, shutdown: CancellationToken) {
    use std::time::Duration;

    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SW_SHOW, TranslateMessage,
    };
    use windows::core::{HSTRING, PCWSTR, w};

    fn open_in_browser(url: &str) {
        unsafe {
            let _ = ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOW,
            );
        }
    }

    fn make_icon() -> Icon {
        const SIZE: usize = 32;
        let mut rgba = vec![0u8; SIZE * SIZE * 4];
        let center = SIZE as f32 / 2.0;
        let radius = 13.0f32;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 + 0.5 - center;
                let dy = y as f32 + 0.5 - center;
                if dx * dx + dy * dy <= radius * radius {
                    let i = (y * SIZE + x) * 4;
                    rgba[i] = 56;
                    rgba[i + 1] = 189;
                    rgba[i + 2] = 248;
                    rgba[i + 3] = 255;
                }
            }
        }
        Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("failed to build tray icon image")
    }

    std::thread::spawn(move || {
        let menu = Menu::new();
        let panel_item = MenuItem::new("Open panel", true, None);
        let overlay_item = MenuItem::new("Open overlay", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        let panel_id = panel_item.id().clone();
        let overlay_id = overlay_item.id().clone();
        let quit_id = quit_item.id().clone();
        let _ = menu.append(&panel_item);
        let _ = menu.append(&overlay_item);
        let _ = menu.append(&quit_item);

        let tray = match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("stream-tools")
            .with_icon(make_icon())
            .build()
        {
            Ok(t) => t,
            Err(e) => {
                log::error!("failed to create tray icon: {e}");
                return;
            }
        };

        let shutdown_handler = shutdown.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == panel_id {
                open_in_browser(&format!("http://127.0.0.1:{port}/panel"));
            } else if event.id == overlay_id {
                open_in_browser(&format!("http://127.0.0.1:{port}/overlay"));
            } else if event.id == quit_id {
                log::info!("shutdown requested via tray menu");
                shutdown_handler.cancel();
            }
        }));

        while !shutdown.is_cancelled() {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        drop(tray);
        log::info!("tray icon removed");
    });
}

#[cfg(not(target_os = "windows"))]
pub fn run(_port: u16, _shutdown: CancellationToken) {
    log::warn!("tray icon is only supported on Windows in this build");
}
