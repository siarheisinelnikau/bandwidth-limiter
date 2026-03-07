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
        // An original, legible 24 px pair of transfer bars, embedded as code-native pixels.
        let mut pixels = vec![0u8; 24 * 24 * 4];
        for y in 3..21 {
            for x in 3..21 {
                if ((6..10).contains(&x) && y < 17) || ((14..18).contains(&x) && y > 7) {
                    pixels[(y * 24 + x) * 4..(y * 24 + x) * 4 + 4]
                        .copy_from_slice(&[76, 214, 186, 255]);
                }
            }
        }
        let icon = tray_icon::Icon::from_rgba(pixels, 24, 24).map_err(|e| e.to_string())?;
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
