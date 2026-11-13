//! Slice 9: closing the main window asks first — quit, or send the
//! window to the tray (Windows) / minimize it (elsewhere) — with a
//! "don't ask again" choice and a matching Settings option
//! (`exit_behavior`: "" ask · "exit" · "tray").
//!
//! On Windows the tray icon lives on its own message-window thread
//! (Shell_NotifyIconW); it signals back through two atomics that a
//! small poll loop in the app picks up, the same pattern as the
//! quick-capture hotkey. The tray thread watches the main window
//! handle: when it is destroyed (the app quit some other way), the
//! thread removes the icon and exits.

use crate::app::Workspace;
use crate::theme::Theme;
use crate::ui::{dialog, primary_button, scrim, secondary_button, ui_para, ui_text};
use gpui::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub static RESTORE: AtomicBool = AtomicBool::new(false);
pub static QUIT: AtomicBool = AtomicBool::new(false);

/// Poll the tray signals on the app thread.
pub fn install_poll(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(std::time::Duration::from_millis(350)).await;
            if QUIT.swap(false, Ordering::Relaxed) {
                let _ = cx.update(crate::app::quit);
            } else if RESTORE.swap(false, Ordering::Relaxed) {
                let _ = cx.update(restore_main);
            }
        }
    })
    .detach();
}

fn restore_main(cx: &mut App) {
    #[cfg(target_os = "windows")]
    win::show_main();
    for handle in cx.windows() {
        if let Some(ws) = handle.downcast::<Workspace>() {
            let _ = ws.update(cx, |w, window, cx| {
                if w.is_main {
                    window.activate_window();
                    cx.notify();
                }
            });
        }
    }
}

/// The main window's native handle (Windows), also recorded for the
/// tray thread.
pub fn window_hwnd(window: &Window) -> Option<isize> {
    #[cfg(target_os = "windows")]
    {
        let raw_handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
        match raw_handle.as_raw() {
            raw_window_handle::RawWindowHandle::Win32(w) => {
                let v = w.hwnd.get();
                win::MAIN.store(v, std::sync::atomic::Ordering::Relaxed);
                Some(v)
            }
            _ => None,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

/// Hide the main window: to the tray on Windows, minimized elsewhere.
pub fn hide_main(window: &mut Window) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = window_hwnd(window) {
            win::ensure_tray(hwnd);
            win::hide(hwnd);
            return;
        }
    }
    window.minimize_window();
}

impl Workspace {
    /// The main window's close request: follow `exit_behavior`.
    pub(crate) fn window_should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.exit_approved {
            return true;
        }
        match self.store.settings().exit_behavior.as_str() {
            "exit" => true,
            "tray" => {
                hide_main(window);
                false
            }
            _ => {
                self.exit_ask = true;
                self.exit_no_ask = false;
                cx.notify();
                false
            }
        }
    }

    fn save_exit_behavior(&mut self, value: &str) {
        let mut s = self.store.settings();
        s.exit_behavior = value.to_string();
        if let Err(e) = self.store.save_settings(&s) {
            crate::logging::write(&format!("exit behaviour setting: {e}"));
        }
    }

    pub(crate) fn exit_from_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exit_no_ask {
            self.save_exit_behavior("exit");
        }
        self.exit_ask = false;
        self.exit_approved = true;
        window.remove_window();
        cx.notify();
    }

    pub(crate) fn tray_from_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exit_no_ask {
            self.save_exit_behavior("tray");
        }
        self.exit_ask = false;
        hide_main(window);
        cx.notify();
    }

    pub(crate) fn render_exit_ask(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.exit_ask {
            return None;
        }
        let body = if cfg!(target_os = "windows") {
            crate::settings::text("退出后，未保存的内容会先自动保存。收起到托盘可以让 ai.md 在后台继续待命。")
        } else {
            crate::settings::text("退出后，未保存的内容会先自动保存。最小化只是把窗口收起来，应用仍在运行。")
        };
        let tray_label = if cfg!(target_os = "windows") { crate::settings::text("最小化到托盘") } else { crate::settings::text("最小化") };
        let on = self.exit_no_ask;
        let dialog = dialog("exit-dialog")
            .w(px(400.))
            .px(px(24.))
            .pt(px(20.))
            .pb(px(24.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).child(ui_para(crate::settings::text("关闭 ai.md"))))
            .child(div().mt(px(8.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(ui_para(body)))
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .id("exit-no-ask")
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .on_click(cx.listener(|w, _, _, cx| {
                                        w.exit_no_ask = !w.exit_no_ask;
                                        cx.notify();
                                    }))
                                    .child(crate::ui::checkbox(on))
                                    .child(ui_text(crate::settings::text("不再提示"))),
                            ),
                    )
                    .child(secondary_button("exit-cancel", crate::settings::text("取消")).on_click(cx.listener(|w, _, _, cx| {
                        w.exit_ask = false;
                        cx.notify();
                    })))
                    .child(secondary_button("exit-tray", tray_label).on_click(cx.listener(|w, _, window, cx| w.tray_from_dialog(window, cx))))
                    .child(primary_button("exit-ok", crate::settings::text("退出")).on_click(cx.listener(|w, _, window, cx| w.exit_from_dialog(window, cx)))),
            );
        Some(
            deferred(
                div()
                    .id("exit-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::{QUIT, RESTORE};
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub static MAIN: AtomicIsize = AtomicIsize::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);
    const CALLBACK: u32 = WM_APP + 7;
    const ID_OPEN: usize = 1;
    const ID_EXIT: usize = 2;

    pub fn ensure_tray(main: isize) {
        MAIN.store(main, Ordering::Relaxed);
        if STARTED.swap(true, Ordering::Relaxed) {
            return;
        }
        std::thread::spawn(move || run(main));
    }

    pub fn hide(main: isize) {
        unsafe {
            let _ = ShowWindow(HWND(main as *mut core::ffi::c_void), SW_HIDE);
        }
    }

    pub fn show_main() {
        let m = MAIN.load(Ordering::Relaxed);
        if m != 0 {
            unsafe {
                let h = HWND(m as *mut core::ffi::c_void);
                let _ = ShowWindow(h, SW_SHOW);
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn run(_main: isize) {
        unsafe {
            let Ok(hmod) = GetModuleHandleW(None) else { return };
            let hinst: HINSTANCE = hmod.into();
            let class = w!("AiMdTrayWindow");
            let wc = WNDCLASSW { hInstance: hinst, lpszClassName: class, lpfnWndProc: Some(proc), ..Default::default() };
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("ai.md"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinst),
                None,
            ) else {
                return;
            };
            let icon = LoadIconW(Some(hinst), PCWSTR::from_raw(std::ptr::without_provenance(1)))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default();
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: CALLBACK,
                hIcon: icon,
                ..Default::default()
            };
            for (i, u) in "ai.md".encode_utf16().enumerate() {
                data.szTip[i] = u;
            }
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
            let _ = SetTimer(Some(hwnd), 1, 1000, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        // Rust 2024: calls to unsafe fns need an explicit unsafe block
        // even inside an unsafe fn.
        unsafe { match msg {
            CALLBACK => {
                let event = lp.0 as u32;
                if event == WM_LBUTTONDBLCLK {
                    RESTORE.store(true, Ordering::Relaxed);
                } else if event == WM_RBUTTONUP {
                    menu(hwnd);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                match wp.0 & 0xffff {
                    ID_OPEN => RESTORE.store(true, Ordering::Relaxed),
                    ID_EXIT => QUIT.store(true, Ordering::Relaxed),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_TIMER => {
                let m = MAIN.load(Ordering::Relaxed);
                if m == 0 || !IsWindow(Some(HWND(m as *mut core::ffi::c_void))).as_bool() {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        } }
    }

    unsafe fn menu(hwnd: HWND) {
      unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let en = crate::settings::english();
        let open = wide(if en { "Open ai.md" } else { "打开 ai.md" });
        let exit = wide(if en { "Exit" } else { "退出" });
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, PCWSTR(open.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, PCWSTR(exit.as_ptr()));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
        let _ = DestroyMenu(menu);
      }
    }
}
