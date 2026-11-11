//! 目录: the bundled ai.md catalog — MCP servers, skills, prompts, prompt
//! packs and guides, one JSON file per entry under `catalog/` (embedded by
//! `catalog_data`). Browsing only: the entries ship with the app and are
//! never written to; "Save to my library" copies one into the library as an
//! ordinary Markdown document the user owns.

use crate::app::{View, Workspace};
use crate::input::{TextField, TextFieldEvent};
use crate::theme::{self, Theme};
use crate::ui::icon;
use fdl_core::catalog::{Catalog, CatalogEntry};
use gpui::{prelude::FluentBuilder, *};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub(crate) const KIND_ALL: Option<&str> = None;
pub(crate) const KINDS: [(&str, &str, &str); 5] = [
    ("mcp", "MCP 服务器", "MCP servers"),
    ("skill", "技能", "Skills"),
    ("prompt", "提示词", "Prompts"),
    ("prompt-pack", "提示词包", "Prompt packs"),
    ("guide", "指南", "Guides"),
];

pub(crate) fn kind_label(kind: &str) -> &'static str {
    match kind {
        "mcp" => crate::settings::tr("MCP 服务器", "MCP server"),
        "skill" => crate::settings::tr("技能", "Skill"),
        "prompt" => crate::settings::tr("提示词", "Prompt"),
        "prompt-pack" => crate::settings::tr("提示词包", "Prompt pack"),
        "guide" => crate::settings::tr("指南", "Guide"),
        _ => crate::settings::tr("条目", "Entry"),
    }
}

pub(crate) fn risk_label(risk: &str) -> &'static str {
    match risk {
        "files" => crate::settings::tr("文件", "Files"),
        "commands" => crate::settings::tr("命令", "Commands"),
        "network" => crate::settings::tr("网络", "Network"),
        "api-key" => crate::settings::tr("需要 API 密钥", "Needs an API key"),
        "read-only" => crate::settings::tr("只读", "Read only"),
        _ => crate::settings::tr("无", "Nothing"),
    }
}

pub struct CatalogState {
    pub data: Catalog,
    pub search: Entity<TextField>,
    pub kind: Option<&'static str>,
    pub selected: Option<String>,
    pub scroll: ScrollHandle,
    pub detail_scroll: ScrollHandle,
    /// The Run prompt sheet: (entry id or document id, source text, fields).
    pub run: Option<RunSheet>,
    /// An install command waiting for the user's OK: the entry id.
    pub confirm_run: Option<String>,
    /// The install currently running, if any (shared with its waiter thread).
    pub running: Option<Arc<Mutex<Option<Child>>>>,
    /// The last install's outcome: (entry name, output tail, succeeded).
    pub run_log: Option<(String, String, bool)>,
    /// The insert-prompt palette (snippets): open, its filter field, selection.
    pub insert_open: bool,
    pub insert_search: Entity<TextField>,
    pub insert_sel: Option<usize>,
    _subs: Vec<Subscription>,
}

/// One fill-in field of the Run prompt sheet.
pub struct RunField {
    pub name: String,
    pub field: Entity<TextField>,
}

pub struct RunSheet {
    /// What the finished prompt is copied from: the template text.
    pub template: String,
    pub title: String,
    pub fields: Vec<RunField>,
    pub scroll: ScrollHandle,
    _subs: Vec<Subscription>,
}

impl CatalogState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let data = fdl_core::catalog::load_catalog(crate::catalog_data::ENTRIES)
            .unwrap_or_else(|e| { crate::logging::write(&format!("catalog: {e}")); Catalog { entries: vec![] } });
        let search = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("搜索目录", "Search the catalog"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let insert_search = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("搜索提示词", "Search prompts"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![
            cx.subscribe(&search, |_, _, ev: &TextFieldEvent, cx| {
                if matches!(ev, TextFieldEvent::Changed) { cx.notify(); }
            }),
            cx.subscribe_in(&insert_search, window, |w, _, ev: &TextFieldEvent, window, cx| {
                match ev {
                    TextFieldEvent::Changed => { w.catalog.insert_sel = Some(0); }
                    TextFieldEvent::Up => w.move_insert_sel(-1, cx),
                    TextFieldEvent::Down => w.move_insert_sel(1, cx),
                    TextFieldEvent::Submit => {
                        if let Some(ix) = w.catalog.insert_sel { w.pick_insert(ix, window, cx); }
                    }
                    TextFieldEvent::Cancel => { w.catalog.insert_open = false; }
                    _ => {}
                }
                cx.notify();
            }),
        ];
        Self {
            data,
            search,
            kind: None,
            selected: None,
            scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            run: None,
            confirm_run: None,
            running: None,
            run_log: None,
            insert_open: false,
            insert_search,
            insert_sel: None,
            _subs: subs,
        }
    }

    pub(crate) fn entry(&self, id: &str) -> Option<&CatalogEntry> {
        self.data.entries.iter().find(|e| e.id == id)
    }
}

fn chip(label: &str) -> Div {
    div().flex_none().px(px(8.)).py(px(3.)).rounded(px(999.)).bg(Theme::fill_hover_strong())
        .text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child(label.to_string())
}

fn pill_button(id: &'static str, label: &str, primary: bool) -> Stateful<Div> {
    div().id(id).flex_none().h(px(32.)).px(px(14.)).flex().items_center().rounded(px(9.))
        .text_size(px(13.5)).line_height(px(20.)).cursor_pointer()
        .when(primary, |d| d.bg(Theme::text()).text_color(Theme::bg_body()).hover(|s| s.opacity(0.85)))
        .when(!primary, |d| d.border_1().border_color(Theme::line_border()).text_color(Theme::text()).hover(|s| s.bg(Theme::fill_hover())))
        .child(label.to_string())
}

impl Workspace {
    fn catalog_results(&self, cx: &App) -> Vec<CatalogEntry> {
        let q = self.catalog.search.read(cx).text.clone();
        self.catalog.data.search(&q, self.catalog.kind).into_iter().cloned().collect()
    }

    pub(crate) fn render_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(id) = self.catalog.selected.clone() {
            if let Some(e) = self.catalog.entry(&id).cloned() {
                return self.render_catalog_detail(&e, window, cx);
            }
            self.catalog.selected = None;
        }
        let header = crate::trash::page_header(self, crate::settings::tr("目录", "Catalog"), 26., cx)
            .child(div().flex_1())
            .child(
                div().w(px(360.)).h(px(32.)).px(px(10.)).flex().items_center().gap(px(8.)).rounded(px(9.)).bg(Theme::fill_hover_strong())
                    .child(icon("search", 15., Theme::text_disabled()))
                    .child(div().flex_1().min_w_0().flex().child(self.catalog.search.clone())),
            );
        // Kind tabs, in the home page's segmented style.
        let tab = |id: &'static str, label: &'static str, kind: Option<&'static str>| {
            let on = self.catalog.kind == kind;
            div().id(id).mr(px(8.)).h(px(28.)).px(px(10.)).flex().items_center().rounded(px(9.)).cursor_pointer()
                .text_size(px(13.)).line_height(px(20.))
                .when(on, |d| d.bg(Theme::fill_hover_strong()).text_color(Theme::text()))
                .when(!on, |d| d.text_color(Theme::text_caption()).hover(|s| s.bg(Theme::fill_hover())))
                .child(label)
                .on_click(cx.listener(move |w, _, _, cx| { w.catalog.kind = kind; cx.notify(); }))
        };
        let mut tabs = div().mt(px(12.)).h(px(28.)).flex_none().pl(px(24.)).pr(px(24.)).flex().items_center()
            .child(tab("catalog-tab-all", crate::settings::tr("全部", "All"), KIND_ALL));
        for (kind, zh, en) in KINDS {
            tabs = tabs.child(tab(kind, crate::settings::tr(zh, en), Some(kind)));
        }
        let results = self.catalog_results(cx);
        let body: AnyElement = if results.is_empty() {
            crate::trash::empty_state("illus/empty-list.svg", crate::settings::tr("没有匹配的条目", "No entries match")).into_any_element()
        } else {
            let mut grid = div().flex().flex_wrap().gap(px(18.)).p(px(24.));
            for (ix, e) in results.iter().enumerate() {
                grid = grid.child(self.catalog_card(e, ix, cx));
            }
            div().id("catalog-grid").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.catalog.scroll).child(grid).into_any_element()
        };
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header).child(tabs).child(body).into_any_element()
    }

    fn catalog_card(&self, e: &CatalogEntry, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let id = e.id.clone();
        let risks = if e.risks.is_empty() { vec!["none".to_string()] } else { e.risks.clone() };
        let risk_text = risks.iter().map(|r| risk_label(r)).collect::<Vec<_>>().join(" · ");
        div().id(("catalog-card", ix)).w(px(300.)).min_h(px(196.)).p(px(18.)).flex().flex_col()
            .rounded(px(14.)).border_1().border_color(Theme::line_border()).bg(Theme::bg_body()).shadow(theme::shadow_card())
            .cursor_pointer().hover(|s| s.bg(Theme::fill_hover()))
            .child(div().text_size(px(16.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(e.name.clone()))
            .child(div().mt(px(6.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::text_caption()).child(e.summary.clone()))
            .child(div().flex_1())
            .child(div().mt(px(12.)).flex().items_center().gap(px(8.)).child(chip(kind_label(&e.kind)))
                .when(e.starter, |d| d.child(div().text_size(px(12.)).text_color(Theme::primary()).child(crate::settings::tr("★ 入门推荐", "★ Starter pick")))))
            .child(div().mt(px(8.)).text_size(px(12.)).text_color(Theme::text_disabled())
                .child(format!("{}: {}", crate::settings::tr("能访问", "Can touch"), risk_text)))
            .on_click(cx.listener(move |w, _, _, cx| { w.catalog.selected = Some(id.clone()); w.catalog.detail_scroll.set_offset(point(px(0.), px(0.))); cx.notify(); }))
            .into_any_element()
    }

    fn render_catalog_detail(&mut self, e: &CatalogEntry, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let header = crate::trash::page_header(self, crate::settings::tr("目录", "Catalog"), 26., cx);
        let meta = format!(
            "{} {}  ·  {} {}  ·  {} {}  ·  {} {}",
            crate::settings::tr("来源", "Source"), e.source_name,
            crate::settings::tr("许可证", "License"), e.license,
            crate::settings::tr("适用于", "Works with"), e.clients.join(", "),
            crate::settings::tr("已核查", "Checked"), e.verified,
        );
        let risks = if e.risks.is_empty() { vec!["none".to_string()] } else { e.risks.clone() };
        let mut page = div().w(px(760.)).mx_auto().pt(px(10.)).pb(px(40.)).flex().flex_col()
            .child(div().id("catalog-back").flex().items_center().gap(px(6.)).cursor_pointer().text_size(px(13.)).text_color(Theme::text_caption())
                .hover(|s| s.text_color(Theme::text()))
                .child(icon("arrow-left", 14., Theme::text_caption()))
                .child(crate::settings::tr("返回目录", "Back to catalog"))
                .on_click(cx.listener(|w, _, _, cx| { w.catalog.selected = None; cx.notify(); })))
            .child(div().mt(px(14.)).text_size(px(26.)).line_height(px(34.)).font_weight(FontWeight::BOLD).text_color(Theme::text()).child(e.name.clone()))
            .child(div().mt(px(6.)).text_size(px(13.)).line_height(px(20.)).text_color(Theme::text_disabled()).child(meta))
            .child(div().mt(px(16.)).text_size(px(14.5)).line_height(px(23.)).text_color(Theme::text()).child(e.summary.clone()))
            .child(div().mt(px(22.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text_disabled())
                .child(crate::settings::tr("能访问什么", "WHAT IT CAN TOUCH")))
            .child(risks.iter().fold(div().mt(px(8.)).flex().gap(px(8.)), |d, r| d.child(chip(risk_label(r)))));
        if let Some(install) = &e.install {
            let text = install.clone();
            page = page.child(div().mt(px(22.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text_disabled())
                    .child(crate::settings::tr("安装命令", "INSTALL COMMAND")))
                .child(self.code_block("catalog-copy-install", install, text, cx));
        }
        if let Some(config) = &e.config {
            let text = config.clone();
            page = page.child(div().mt(px(22.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text_disabled())
                    .child(crate::settings::tr("配置片段", "CONFIG SNIPPET")))
                .child(self.code_block("catalog-copy-config", config, text, cx));
        }
        if let Some(body) = &e.body {
            page = page.child(div().mt(px(22.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text_disabled())
                    .child(crate::settings::tr("提示词正文", "PROMPT")))
                .child(div().mt(px(8.)).p(px(14.)).rounded(px(10.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
                    .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text_code()).font_family(theme::MONO_FONT).child(body.clone()));
        }
        let id = e.id.clone();
        let mut btns = div().mt(px(26.)).flex().gap(px(10.))
            .child(pill_button("catalog-save", crate::settings::tr("保存到我的文档", "Save to my library"), true)
                .on_click(cx.listener(move |w, _, window, cx| w.save_catalog_entry(&id, window, cx))));
        if e.body.is_some() {
            let id2 = e.id.clone();
            btns = btns.child(pill_button("catalog-run-prompt", crate::settings::tr("运行提示词", "Run prompt"), false)
                .on_click(cx.listener(move |w, _, window, cx| w.open_run_sheet_for_entry(&id2, window, cx))));
        }
        if e.install.is_some() {
            let id3 = e.id.clone();
            let running = self.catalog.running.is_some();
            btns = btns.child(pill_button("catalog-run-install", if running { crate::settings::tr("停止", "Stop") } else { crate::settings::tr("运行安装…", "Run install…") }, false)
                .on_click(cx.listener(move |w, _, _, cx| {
                    if w.catalog.running.is_some() { w.stop_install(cx); } else { w.catalog.confirm_run = Some(id3.clone()); cx.notify(); }
                })));
        }
        let src = e.source_url.clone();
        btns = btns.child(pill_button("catalog-open-source", crate::settings::tr("打开来源", "Open source"), false)
            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&src))));
        page = page.child(btns);
        if let Some((name, out, ok)) = &self.catalog.run_log && *name == e.name {
            page = page.child(div().mt(px(18.)).p(px(14.)).rounded(px(10.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
                .child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                    .child(if *ok { crate::settings::tr("安装命令已完成", "Install command finished") } else { crate::settings::tr("安装命令失败或被停止", "Install command failed or was stopped") }))
                .child(div().mt(px(6.)).text_size(px(12.)).line_height(px(18.)).font_family(theme::MONO_FONT).text_color(Theme::text_caption()).child(out.clone())));
        }
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header)
            .child(div().id("catalog-detail").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.catalog.detail_scroll)
                .child(div().flex().justify_center().child(page)))
            .into_any_element()
    }

    fn code_block(&self, id: &'static str, text: &str, copy_text: String, cx: &mut Context<Self>) -> AnyElement {
        div().mt(px(8.)).p(px(14.)).flex().items_start().gap(px(12.)).rounded(px(10.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
            .child(div().flex_1().min_w_0().text_size(px(13.)).line_height(px(20.)).font_family(theme::MONO_FONT).text_color(Theme::text_code()).child(text.to_string()))
            .child(pill_button(id, crate::settings::tr("复制", "Copy"), false)
                .on_click(cx.listener(move |w, _, _, cx| w.copy_text(&copy_text, cx))))
            .into_any_element()
    }

    pub(crate) fn copy_text(&mut self, text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        self.show_toast(crate::settings::tr("已复制", "Copied"), cx);
    }

    /// Save a catalog entry into the library as an ordinary Markdown
    /// document, then open it. The catalog itself stays read-only.
    pub(crate) fn save_catalog_entry(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.catalog.entry(id).cloned() else { return };
        let md = fdl_core::catalog::fork_markdown(&e);
        let mut doc = crate::markdown::parse_file(&md, &e.name, None);
        if doc.blocks.is_empty() { doc.blocks.push(fdl_core::model::Block::text("")); }
        match self.store.create_markdown(&doc, crate::markdown::front_matter(&md).0) {
            Ok(new_id) => {
                self.entries = self.store.list();
                self.sort();
                self.show_toast(crate::settings::tr("已保存到我的文档", "Saved to my documents"), cx);
                self.open(&new_id, window, cx);
            }
            Err(err) => {
                crate::logging::write(&format!("catalog save: {err}"));
                self.show_toast(crate::settings::tr("保存失败", "Could not save"), cx);
            }
        }
    }

    /// Ask first, every time: the consent dialog renders while this is set.
    pub(crate) fn render_run_consent(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.catalog.confirm_run.clone()?;
        let e = self.catalog.entry(&id)?.clone();
        let install = e.install.clone()?;
        let run_id = id.clone();
        let overlay = div().id("catalog-run-consent").absolute().inset_0().flex().items_center().justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.28))
            .on_click(cx.listener(|w, _, _, cx| { w.catalog.confirm_run = None; cx.notify(); }))
            .child(div().id("run-consent-card").w(px(520.)).p(px(24.)).rounded(px(16.)).bg(Theme::bg_body()).shadow(theme::shadow_dialog())
                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .child(div().text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(Theme::text())
                    .child(crate::settings::tr("运行这条安装命令？", "Run this install command?")))
                .child(div().mt(px(8.)).text_size(px(13.5)).line_height(px(21.)).text_color(Theme::text_caption())
                    .child(crate::settings::tr("它会在你的电脑上以你的权限运行。可以随时停止。", "It runs on your computer with your permissions. You can stop it at any time.")))
                .child(div().mt(px(14.)).p(px(12.)).rounded(px(9.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
                    .text_size(px(12.5)).font_family(theme::MONO_FONT).text_color(Theme::text_code()).child(install))
                .child(div().mt(px(20.)).flex().justify_end().gap(px(10.))
                    .child(pill_button("catalog-consent-cancel", crate::settings::tr("取消", "Cancel"), false)
                        .on_click(cx.listener(|w, _, _, cx| { w.catalog.confirm_run = None; cx.notify(); })))
                    .child(pill_button("catalog-consent-run", crate::settings::tr("运行", "Run"), true)
                        .on_click(cx.listener(move |w, _, _, cx| w.run_install(&run_id, cx))))));
        Some(overlay.into_any_element())
    }

    /// Run the entry's install command after consent. Output is captured;
    /// the last lines land in the detail page when it finishes.
    pub(crate) fn run_install(&mut self, id: &str, cx: &mut Context<Self>) {
        self.catalog.confirm_run = None;
        let Some(e) = self.catalog.entry(id).cloned() else { return };
        let Some(install) = e.install.clone() else { return };
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", &install]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &install]);
            c
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(child) => {
                let slot = Arc::new(Mutex::new(Some(child)));
                self.catalog.running = Some(slot.clone());
                self.catalog.run_log = None;
                cx.notify();
                let name = e.name.clone();
                cx.spawn(async move |this, cx| {
                    let out = std::thread::spawn(move || {
                        let child = slot.lock().unwrap().take().unwrap();
                        let out = child.wait_with_output();
                        match out {
                            Ok(o) => {
                                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                                s.push_str(&String::from_utf8_lossy(&o.stderr));
                                let tail: String = s.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                                (tail, o.status.success())
                            }
                            Err(err) => (err.to_string(), false),
                        }
                    }).join().unwrap_or_else(|_| ("worker failed".to_string(), false));
                    let _ = this.update(cx, |w, cx| {
                        w.catalog.running = None;
                        w.catalog.run_log = Some((name.clone(), out.0.clone(), out.1));
                        w.show_toast(if out.1 { crate::settings::tr("安装命令已完成", "Install command finished") } else { crate::settings::tr("安装命令失败或被停止", "Install command failed or was stopped") }, cx);
                        cx.notify();
                    });
                }).detach();
            }
            Err(err) => {
                self.catalog.run_log = Some((e.name.clone(), err.to_string(), false));
                cx.notify();
            }
        }
    }

    pub(crate) fn stop_install(&mut self, cx: &mut Context<Self>) {
        if let Some(slot) = &self.catalog.running && let Some(mut child) = slot.lock().unwrap().take() {
            let _ = child.kill();
        }
        cx.notify();
    }

    /// Open the Run prompt sheet for a catalog prompt entry.
    pub(crate) fn open_run_sheet_for_entry(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.catalog.entry(id).cloned() else { return };
        let Some(body) = e.body.clone() else { return };
        self.open_run_sheet(e.name.clone(), body, window, cx);
    }

    /// The Run prompt sheet over any template text (a catalog prompt, or the
    /// open document when it contains {{variables}}).
    pub(crate) fn open_run_sheet(&mut self, title: String, template: String, window: &mut Window, cx: &mut Context<Self>) {
        let parsed = fdl_core::prompt::parse_template(&template);
        let mut subs = Vec::new();
        let mut fields = Vec::new();
        for var in &parsed.variables {
            let field = cx.new(|cx| {
                let mut f = TextField::new(var.name.clone(), cx);
                f.placeholder_color = Theme::text_disabled();
                if let Some(d) = &var.default { f.set_text(d, cx); }
                f
            });
            subs.push(cx.subscribe(&field, |_, _, ev: &TextFieldEvent, cx| {
                if matches!(ev, TextFieldEvent::Changed) { cx.notify(); }
            }));
            fields.push(RunField { name: var.name.clone(), field });
        }
        if let Some(first) = fields.first() {
            window.focus(&first.field.focus_handle(cx));
        }
        self.catalog.run = Some(RunSheet { template, title, fields, scroll: ScrollHandle::new(), _subs: subs });
        cx.notify();
    }

    /// The Run prompt sheet overlay, rendered while a sheet is open.
    pub(crate) fn render_run_sheet(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let run = self.catalog.run.as_ref()?;
        let values: Vec<(String, String)> = run.fields.iter().map(|f| (f.name.clone(), f.field.read(cx).text.clone())).collect();
        let rendered = fdl_core::prompt::render(&run.template, &values);
        let (preview, missing) = match &rendered {
            Ok(text) => (text.clone(), Vec::<String>::new()),
            Err(m) => (run.template.clone(), m.clone()),
        };
        let total = run.fields.len();
        let filled = total - missing.len();
        let complete = missing.is_empty();
        let mut fields_col = div().flex().flex_col();
        for f in &run.fields {
            fields_col = fields_col
                .child(div().mt(px(12.)).text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(f.name.clone()))
                .child(div().mt(px(5.)).h(px(34.)).px(px(10.)).flex().items_center().rounded(px(9.)).border_1().border_color(Theme::line_border())
                    .child(div().flex_1().min_w_0().flex().child(f.field.clone())));
        }
        let sheet = div().id("run-sheet").absolute().inset_0().flex().justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.28))
            .on_click(cx.listener(|w, _, _, cx| { w.catalog.run = None; cx.notify(); }))
            .child(div().id("run-sheet-card").mt(px(72.)).w(px(640.)).h(px(560.)).p(px(24.)).flex().flex_col().rounded(px(18.)).bg(Theme::bg_body()).shadow(theme::shadow_dialog())
                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(Theme::text()).child(crate::settings::tr("运行提示词", "Run prompt")))
                .child(div().mt(px(3.)).text_size(px(13.)).text_color(Theme::text_disabled())
                    .child(format!("{} — {}", run.title, crate::settings::tr("填好空格，然后复制成品提示词。", "fill in the blanks, then copy the finished prompt."))))
                .child(div().id("run-fields").flex_1().min_h_0().overflow_y_scroll().track_scroll(&run.scroll)
                    .child(fields_col)
                    .child(div().mt(px(16.)).text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(crate::settings::tr("预览", "Preview")))
                    .child(div().mt(px(5.)).p(px(12.)).rounded(px(9.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
                        .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(preview)))
                .child(div().mt(px(16.)).flex().items_center()
                    .child(div().text_size(px(12.5)).text_color(Theme::text_disabled())
                        .child(crate::localized_format!("已填 {filled} / {total}", "Filled {filled} of {total}")))
                    .child(div().flex_1())
                    .child(pill_button("run-copy", crate::settings::tr("复制提示词", "Copy prompt"), complete)
                        .when(complete, |d| d.on_click(cx.listener(move |w, _, _, cx| {
                            let run = w.catalog.run.as_ref().unwrap();
                            let values: Vec<(String, String)> = run.fields.iter().map(|f| (f.name.clone(), f.field.read(cx).text.clone())).collect();
                            if let Ok(text) = fdl_core::prompt::render(&run.template, &values) {
                                w.copy_text(&text, cx);
                                w.catalog.run = None;
                                cx.notify();
                            }
                        }))))
                    .child(div().w(px(10.)))
                    .child(pill_button("run-cancel", crate::settings::tr("取消", "Cancel"), false)
                        .on_click(cx.listener(|w, _, _, cx| { w.catalog.run = None; cx.notify(); })))));
        Some(sheet.into_any_element())
    }

    /// Switch to the catalog and open one entry (used by search results).
    pub(crate) fn open_catalog_entry(&mut self, id: &str, cx: &mut Context<Self>) {
        self.catalog.selected = Some(id.to_string());
        self.show_view(View::Catalog, cx);
    }

    /// Snippets, Ferrite-style: the prompt palette. Type to filter the
    /// catalog's prompts, Enter inserts the template into the open document
    /// at the caret, ready to fill in inline (or Run prompt to fill first).
    pub(crate) fn open_insert_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.catalog.insert_open = true;
        self.catalog.insert_sel = Some(0);
        let f = self.catalog.insert_search.clone();
        f.update(cx, |s, cx| s.set_text("", cx));
        window.focus(&f.focus_handle(cx));
        cx.notify();
    }

    fn insert_results(&self, cx: &App) -> Vec<CatalogEntry> {
        let q = self.catalog.insert_search.read(cx).text.clone();
        self.catalog.data.search(&q, Some("prompt")).into_iter().cloned().collect()
    }

    pub(crate) fn move_insert_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.insert_results(cx).len();
        if n == 0 { self.catalog.insert_sel = None; return; }
        self.catalog.insert_sel = Some(match self.catalog.insert_sel {
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
            None if delta > 0 => 0,
            None => n - 1,
        });
        cx.notify();
    }

    pub(crate) fn pick_insert(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let results = self.insert_results(cx);
        let Some(e) = results.get(row) else { return };
        let Some(body) = e.body.clone() else { return };
        let mut doc = crate::markdown::parse_file(&body, &e.name, None);
        let blocks = std::mem::take(&mut doc.blocks);
        self.catalog.insert_open = false;
        if let Some(ed) = &self.editor {
            ed.update(cx, |ed, cx| ed.insert_blocks_at_caret(blocks, window, cx));
        }
        self.show_toast(crate::settings::tr("已插入提示词", "Prompt inserted"), cx);
        cx.notify();
    }

    pub(crate) fn render_insert_palette(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.catalog.insert_open { return None; }
        let results = self.insert_results(cx);
        let mut list = div().flex().flex_col().px(px(6.)).py(px(4.));
        if results.is_empty() {
            list = list.child(div().px(px(12.)).py(px(10.)).text_size(px(13.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("没有匹配的提示词", "No prompts match")));
        }
        for (i, e) in results.iter().enumerate() {
            list = list.child(
                div().id(("insert-prompt", i)).flex_none().px(px(12.)).py(px(9.)).flex().items_center().rounded(px(8.)).cursor_pointer()
                    .when(self.catalog.insert_sel == Some(i), |d| d.bg(Theme::fill_hover()))
                    .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
                        if *hovered && w.catalog.insert_sel != Some(i) { w.catalog.insert_sel = Some(i); cx.notify(); }
                    }))
                    .on_click(cx.listener(move |w, _, window, cx| w.pick_insert(i, window, cx)))
                    .child(icon("braces", 15., Theme::icon()))
                    .child(div().ml(px(10.)).flex_1().min_w_0()
                        .child(div().text_size(px(13.5)).text_color(Theme::text()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(e.name.clone()))
                        .child(div().text_size(px(12.)).text_color(Theme::text_disabled()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(e.summary.clone()))),
            );
        }
        let modal = div().id("insert-palette").absolute().inset_0().flex().justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.28))
            .on_click(cx.listener(|w, _, _, cx| { w.catalog.insert_open = false; cx.notify(); }))
            .child(div().id("insert-palette-card").mt(px(110.)).w(px(540.)).h(px(430.)).flex().flex_col().rounded(px(16.)).bg(Theme::bg_body()).shadow(theme::shadow_dialog())
                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .child(div().flex_none().h(px(46.)).pl(px(12.)).pr(px(8.)).flex().items_center()
                    .child(icon("search", 16., Theme::text_disabled()))
                    .child(div().flex_1().min_w_0().pl(px(8.)).flex().child(self.catalog.insert_search.clone())))
                .child(div().flex_none().h(px(1.)).bg(Theme::line_divider()))
                .child(div().id("insert-list").flex_1().min_h_0().overflow_y_scroll().child(list))
                .child(div().flex_none().h(px(34.)).pl(px(14.)).flex().items_center().border_t_1().border_color(Theme::line_divider()).bg(Theme::bg_overlay())
                    .text_size(px(12.)).text_color(Theme::text_caption())
                    .child(crate::settings::tr("↑↓ 选择，Enter 插入到光标处", "Use ↑↓ to select, Enter to insert at the cursor"))));
        Some(modal.into_any_element())
    }
}
