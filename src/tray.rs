use crate::window::Wake;
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use winit::event_loop::EventLoopProxy;
pub struct Tray {
    _icon: TrayIcon,
    proxy: MenuItem,
    call: CheckMenuItem,
    last: Option<(bool, bool)>,
}
impl Tray {
    pub fn new(events: EventLoopProxy<Wake>) -> Result<Self, String> {
        let menu = Menu::new();
        let open = MenuItem::with_id("open", "Open BandwidthLimiter", true, None);
        let proxy = MenuItem::with_id("proxy", "Start proxy", true, None);
        let call = CheckMenuItem::with_id("call", "Call mode", true, false, None);
        let settings = MenuItem::with_id("settings", "Settings", true, None);
        let quit = MenuItem::with_id("quit", "Quit and stop limiting", true, None);
        menu.append_items(&[
            &open,
            &proxy,
            &call,
            &settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        // Match the shell's native small-icon metric instead of shrinking a fixed bitmap.
        let size = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON,
            )
        }
        .max(16) as u32;
        let icon = tray_icon::Icon::from_rgba(crate::icons::pixels(size), size, size)
            .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .with_tooltip("BandwidthLimiter · proxy paused")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = events.send_event(Wake::Menu(event.id.0));
        }));
        Ok(Self {
            _icon: icon,
            proxy,
            call,
            last: None,
        })
    }
    pub fn sync(&mut self, running: bool, call: bool) {
        if self.last == Some((running, call)) {
            return;
        }
        self.last = Some((running, call));
        self.proxy.set_text(if running {
            "Pause proxy"
        } else {
            "Start proxy"
        });
        self.call.set_checked(call);
        let _ = self._icon.set_tooltip(Some(format!(
            "BandwidthLimiter · proxy {}{}",
            if running { "running" } else { "paused" },
            if call { " · Call mode" } else { "" }
        )));
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    }
}
