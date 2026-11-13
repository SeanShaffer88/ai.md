//! Quick capture (#13): a small window for the two things you want
//! mid-thought — run a favorite prompt (pick it, fill the blanks,
//! copy) and jot a line into the library. Summoned from the command
//! palette, and on Windows by the global hotkey (the configured
//! quick-note combo) through a message-only window on its own thread:
//! GPUI has no global-hotkey hook, so the thread registers the key
//! with Windows and flags this side, which polls while the app runs.
//! On macOS the existing quick-note hotkey keeps its own window; the
//! capture window is one palette action away there.

use crate::input::{TextField, TextFieldEvent};
use crate::theme::Theme;
use gpui::{prelude::FluentBuilder, *};

actions!(quick_capture, [QuickCapture]);

#[derive(Clone)]
struct CapturePrompt {
    title: String,
    body: String,
    vars: Vec<String>,
}

pub struct QuickCaptureView {
    query: Entity<TextField>,
    jot: Entity<TextField>,
    prompts: Vec<CapturePrompt>,
    sel: Option<usize>,
    chosen: Option<usize>,
    fields: Vec<Entity<TextField>>,
    status: Option<String>,
    _subs: Vec<Subscription>,
}

impl QuickCaptureView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The prompt list: favorited catalog prompts first, then the
        // user's own documents that contain blanks.
        let mut prompts: Vec<CapturePrompt> = Vec::new();
        if let Some(store) = cx.try_global::<crate::shared_store::SharedStore>() {
            let settings = store.settings();
            if let Ok(catalog) = fdl_core::catalog::load_catalog(crate::catalog_data::ENTRIES) {
                for id in &settings.catalog_favorites {
                    if let Some(e) = catalog.entries.iter().find(|e| &e.id == id)
                        && crate::search::is_prompt_kind(&e.kind)
                        && let Some(body) = &e.body
                    {
                        prompts.push(CapturePrompt {
                            title: e.name.clone(),
                            body: body.clone(),
                            vars: fdl_core::prompt::parse_template(body).variables.iter().map(|v| v.name.clone()).collect(),
                        });
                    }
                }
            }
            let mut store = store.clone();
            for e in store.list() {
                if e.meta.folder.is_some() || e.meta.local() || prompts.iter().any(|p| p.title == e.title) { continue; }
                let vars: Vec<String> = fdl_core::prompt::parse_template(&e.body).variables.iter().map(|v| v.name.clone()).collect();
                if !vars.is_empty() {
                    prompts.push(CapturePrompt { title: e.title.clone(), body: e.body.to_string(), vars });
                }
            }
        }
        let query = cx.new(|cx| TextField::new(crate::settings::tr("搜索提示词…", "Search prompts…"), cx));
        let jot = cx.new(|cx| TextField::new(crate::settings::tr("记一行到文档库…（回车保存）", "Jot a line into the library… (Enter saves)"), cx));
        let mut subs = Vec::new();
        subs.push(cx.subscribe(&query, |w: &mut Self, _, ev: &TextFieldEvent, cx| match ev {
            TextFieldEvent::Changed => { w.sel = None; w.chosen = None; w.fields.clear(); w.status = None; cx.notify(); }
            TextFieldEvent::Up => { let n = w.filtered(cx).len(); if n > 0 { w.sel = Some(match w.sel { Some(s) => (s + n - 1) % n, None => n - 1 }); cx.notify(); } }
            TextFieldEvent::Down => { let n = w.filtered(cx).len(); if n > 0 { w.sel = Some(match w.sel { Some(s) => (s + 1) % n, None => 0 }); cx.notify(); } }
            TextFieldEvent::Submit => { let rows = w.filtered(cx); if let Some(p) = rows.get(w.sel.unwrap_or(0)) { let ix = w.prompts.iter().position(|x| x.title == p.title); if let Some(ix) = ix { w.choose(ix, cx); } } }
            _ => {}
        }));
        subs.push(cx.subscribe(&jot, |w: &mut Self, field, ev: &TextFieldEvent, cx| {
            if matches!(ev, TextFieldEvent::Submit) {
                let text = field.read(cx).text.clone();
                w.save_jot(&text, cx);
            }
        }));
        let view = Self { query, jot, prompts, sel: None, chosen: None, fields: Vec::new(), status: None, _subs: subs };
        window.focus(&view.query.read(cx).focus_handle(cx));
        view
    }

    fn filtered(&self, cx: &App) -> Vec<CapturePrompt> {
        let q = self.query.read(cx).text.trim().to_lowercase();
        self.prompts.iter().filter(|p| q.is_empty() || p.title.to_lowercase().contains(&q)).cloned().collect()
    }

    fn choose(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(p) = self.prompts.get(ix).cloned() else { return };
        if p.vars.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(p.body.clone()));
            self.status = Some(crate::settings::tr("已复制 — 窗口可以关了", "Copied — you can close the window").to_string());
            self.chosen = None;
            cx.notify();
            return;
        }
        let saved = cx.try_global::<crate::shared_store::SharedStore>().map(|s| s.settings().run_values.get(&p.title).cloned().unwrap_or_default());
        self.fields = p.vars.iter().map(|name| {
            let field = cx.new(|cx| TextField::new(name.clone(), cx));
            if let Some(saved) = &saved
                && let Some((_, v)) = saved.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))
            {
                field.update(cx, |f, cx| f.set_text(v, cx));
            }
            field
        }).collect();
        self.chosen = Some(ix);
        self.status = None;
        cx.notify();
    }

    fn copy_chosen(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.chosen else { return };
        let p = self.prompts[ix].clone();
        let values: Vec<(String, String)> = p.vars.iter().zip(&self.fields)
            .map(|(name, f)| (name.clone(), f.read(cx).text.trim().to_string()))
            .collect();
        match fdl_core::prompt::render(&p.body, &values) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.status = Some(crate::settings::tr("已复制", "Copied").to_string());
                cx.notify();
            }
            Err(missing) => {
                self.status = Some(format!("{}: {}", crate::settings::tr("还没填", "Still blank"), missing.join(", ")));
                cx.notify();
            }
        }
    }

    fn save_jot(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() { return; }
        let Some(store) = cx.try_global::<crate::shared_store::SharedStore>() else { return };
        let mut store = store.clone();
        let mut doc = crate::model::Document::default();
        doc.blocks = text.lines().map(crate::model::Block::text).collect();
        if doc.blocks.is_empty() { doc.blocks.push(crate::model::Block::text("")); }
        let title: String = text.lines().next().unwrap_or_default().chars().take(60).collect();
        match store.create_from(&doc).and_then(|id| store.rename(&id, &title)) {
            Ok(_) => {
                self.status = Some(crate::settings::tr("已存入文档库", "Saved to the library").to_string());
                self.jot.update(cx, |f, cx| f.set_text("", cx));
            }
            Err(e) => {
                crate::logging::write(&format!("quick capture jot: {e}"));
                self.status = Some(crate::settings::tr("保存失败", "Could not save").to_string());
            }
        }
        cx.notify();
    }
}

impl Render for QuickCaptureView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.filtered(cx);
        let mut list = div().id("qc-list").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
        if rows.is_empty() {
            list = list.child(div().mt(px(14.)).text_size(px(13.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("还没有带填空的提示词 — 先在目录里收藏提示词，或在文档里写 {{变量}}", "No prompts with blanks yet — favorite a catalog prompt, or write {{variables}} in a document")));
        }
        for (i, p) in rows.iter().enumerate() {
            let title = p.title.clone();
            let caption = if p.vars.is_empty() {
                crate::settings::tr("点按即可复制", "Click to copy").to_string()
            } else {
                crate::localized_format!("{} 个填空", "{} blanks", p.vars.len())
            };
            let selected = self.sel == Some(i);
            list = list.child(
                div().id(SharedString::from(format!("qc-row-{i}"))).px(px(10.)).py(px(8.)).flex().items_center().gap(px(10.)).rounded(px(10.)).cursor_pointer()
                    .when(selected, |d| d.bg(Theme::fill_hover_strong()))
                    .when(!selected, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                    .child(div().flex_1().min_w_0().flex().flex_col()
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(p.title.clone()))
                        .child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(caption)))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        if let Some(ix) = w.prompts.iter().position(|x| x.title == title) { w.choose(ix, cx); }
                    })),
            );
        }
        let mut root = div().size_full().flex().flex_col().bg(Theme::bg_body()).p(px(16.))
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, window, _| {
                if ev.keystroke.key == "escape" { window.remove_window(); }
            }))
            .child(div().flex().items_center()
                .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(Theme::text()).child(crate::settings::tr("快速捕获", "Quick capture")))
                .child(div().flex_1())
                .child(div().text_size(px(12.)).text_color(Theme::text_disabled()).child("Esc")))
            .child(div().mt(px(12.)).child(self.query.clone()))
            .child(list);
        if let Some(ix) = self.chosen
            && let Some(p) = self.prompts.get(ix).cloned()
        {
            let mut form = div().mt(px(12.)).pt(px(12.)).border_t_1().border_color(Theme::line_border()).flex().flex_col()
                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(p.title.clone()));
            for (name, field) in p.vars.iter().zip(&self.fields) {
                form = form.child(div().mt(px(8.)).flex().items_center().gap(px(10.))
                    .child(div().w(px(110.)).flex_none().text_size(px(12.5)).text_color(Theme::text_caption()).child(format!("{{{{{name}}}}}")))
                    .child(div().flex_1().child(field.clone())));
            }
            form = form.child(div().mt(px(12.)).flex().items_center().gap(px(8.))
                .child(crate::catalog::pill_button("qc-back", crate::settings::tr("返回", "Back"), false)
                    .on_click(cx.listener(|w, _, _, cx| { w.chosen = None; w.fields.clear(); cx.notify(); })))
                .child(div().flex_1())
                .child(crate::catalog::pill_button("qc-copy", crate::settings::tr("复制提示词", "Copy prompt"), true)
                    .on_click(cx.listener(|w, _, _, cx| w.copy_chosen(cx)))));
            root = root.child(form);
        }
        root = root
            .child(div().mt(px(12.)).pt(px(12.)).border_t_1().border_color(Theme::line_border()).flex().items_center().gap(px(8.))
                .child(div().flex_1().child(self.jot.clone()))
                .child(crate::catalog::pill_button("qc-save", crate::settings::tr("保存", "Save"), false)
                    .on_click(cx.listener(|w, _, _, cx| {
                        let text = w.jot.read(cx).text.clone();
                        w.save_jot(&text, cx);
                    }))))
            .when_some(self.status.clone(), |d, status| d
                .child(div().mt(px(10.)).text_size(px(12.5)).text_color(Theme::text_caption()).child(status)));
        root
    }
}

#[derive(Default)]
struct CaptureWindow(Option<WindowHandle<QuickCaptureView>>);
impl Global for CaptureWindow {}

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<CaptureWindow>().and_then(|w| w.0)
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        cx.activate(true);
        return;
    }
    let bounds = Bounds::centered(None, size(px(460.), px(540.)), cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: crate::theme::window_background(),
            app_id: Some(crate::APP_ID.into()),
            window_decorations: if cfg!(target_os = "linux") { Some(WindowDecorations::Client) } else { None },
            titlebar: Some(TitlebarOptions {
                title: Some(crate::settings::text("ai.md · 快速捕获").into()),
                appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                traffic_light_position: Some(point(px(20.), px(crate::app::HEADER_H / 2. - 7.))),
            }),
            window_min_size: Some(size(px(380.), px(420.))),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| QuickCaptureView::new(window, cx)),
    );
    match result {
        Ok(handle) => {
            if !cx.has_global::<CaptureWindow>() { cx.set_global(CaptureWindow::default()); }
            cx.global_mut::<CaptureWindow>().0 = Some(handle);
            cx.activate(true);
        }
        Err(e) => eprintln!("无法打开快速捕获：{e}"),
    }
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &QuickCapture, cx| open(cx));
    #[cfg(target_os = "windows")]
    win_hotkey::start(cx);
}

/// Windows global hotkey: a message-only window on its own thread
/// registers the configured combo (Settings › Shortcuts › global,
/// default Alt+Space); WM_HOTKEY sets a flag the app polls.
#[cfg(target_os = "windows")]
mod win_hotkey {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG,
        WINDOW_EX_STYLE, WM_HOTKEY, WNDCLASSW, WS_OVERLAPPED,
    };
    use windows::core::PCWSTR;

    static PRESSED: AtomicBool = AtomicBool::new(false);

    fn parse_combo(combo: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
        let mut mods = MOD_NOREPEAT;
        let mut vk: Option<u32> = None;
        for part in combo.split('-') {
            match part.to_lowercase().as_str() {
                "alt" | "option" => mods |= MOD_ALT,
                "ctrl" | "control" => mods |= MOD_CONTROL,
                "shift" => mods |= MOD_SHIFT,
                "win" | "cmd" | "command" | "meta" => mods |= MOD_WIN,
                "space" => vk = Some(0x20),
                k if k.len() == 1 => {
                    let c = k.chars().next()?;
                    if c.is_ascii_alphanumeric() { vk = Some(c.to_ascii_uppercase() as u32); }
                }
                _ => return None,
            }
        }
        vk.map(|vk| (mods, vk))
    }

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_HOTKEY {
            PRESSED.store(true, Ordering::SeqCst);
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    pub fn start(cx: &mut App) {
        let combo = crate::shortcuts::global_key();
        let Some((mods, vk)) = parse_combo(&combo) else {
            crate::logging::write(&format!("quick capture hotkey: cannot parse combo {combo}"));
            return;
        };
        std::thread::spawn(move || unsafe {
            let class_name: Vec<u16> = "AiMdQuickCaptureHotkey\0".encode_utf16().collect();
            let wc = WNDCLASSW { lpfnWndProc: Some(wnd_proc), lpszClassName: PCWSTR(class_name.as_ptr()), ..Default::default() };
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), PCWSTR(class_name.as_ptr()), PCWSTR::null(), WS_OVERLAPPED, 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else {
                crate::logging::write("quick capture hotkey: no message window");
                return;
            };
            if RegisterHotKey(Some(hwnd), 1, mods, vk).is_err() {
                crate::logging::write("quick capture hotkey: combo already taken");
                return;
            }
            let mut msg = MSG::default();
            loop {
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 <= 0 { break; }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                if PRESSED.swap(false, Ordering::SeqCst) {
                    let _ = cx.update(|cx| super::open(cx));
                }
            }
        })
        .detach();
    }
}
