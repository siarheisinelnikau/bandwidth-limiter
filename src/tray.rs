use crate::window::Wake;
use bandwidth_limiter::i18n::{Language, tr};
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use winit::event_loop::EventLoopProxy;
pub struct Tray {
    _icon: TrayIcon,
    proxy: MenuItem,
    call: CheckMenuItem,
    last: Option<(bool, bool, Language)>,
    open: MenuItem,
    settings: MenuItem,
    quit: MenuItem,
}
impl Tray {
    pub fn new(events: EventLoopProxy<Wake>, language: Language) -> Result<Self, String> {
        let menu = Menu::new();
        let open = MenuItem::with_id("open", tr(language, "Open BandwidthLimiter"), true, None);
        let proxy = MenuItem::with_id("proxy", tr(language, "Start proxy"), true, None);
        let call = CheckMenuItem::with_id("call", tr(language, "Call mode"), true, false, None);
        let settings = MenuItem::with_id("settings", tr(language, "Settings"), true, None);
        let quit = MenuItem::with_id("quit", tr(language, "Quit and stop limiting"), true, None);
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
            .with_tooltip(format!(
                "BandwidthLimiter · {}",
                tr(language, "Proxy paused")
            ))
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
            open,
            settings,
            quit,
        })
    }
    pub fn sync(&mut self, running: bool, call: bool, language: Language) {
        if self.last == Some((running, call, language)) {
            return;
        }
        self.last = Some((running, call, language));
        self.proxy.set_text(tr(
            language,
            if running {
                "Pause proxy"
            } else {
                "Start proxy"
            },
        ));
        self.open.set_text(tr(language, "Open BandwidthLimiter"));
        self.settings.set_text(tr(language, "Settings"));
        self.quit.set_text(tr(language, "Quit and stop limiting"));
        self.call.set_text(tr(language, "Call mode"));
        self.call.set_checked(call);
        let _ = self._icon.set_tooltip(Some(format!(
            "BandwidthLimiter · {}{}",
            tr(
                language,
                if running {
                    "Proxy running"
                } else {
                    "Proxy paused"
                }
            ),
            if call {
                format!(" · {}", tr(language, "Call mode"))
            } else {
                String::new()
            }
        )));
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
    }
}
