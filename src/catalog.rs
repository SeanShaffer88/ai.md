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

/// The "can touch" vocabulary, in menu order.
pub(crate) const RISKS: [&str; 6] = ["none", "files", "commands", "network", "api-key", "read-only"];

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

/// How the catalog browse page lays its entries out.
#[derive(Clone, Copy, PartialEq)]
pub enum CatalogLayout {
    Grid,
    List,
}

/// How the catalog browse page orders its entries.
#[derive(Clone, Copy, PartialEq)]
pub enum CatalogSort {
    NameAsc,
    NameDesc,
    Kind,
    Starter,
    Newest,
}

impl CatalogSort {
    fn key(self) -> &'static str {
        match self {
            Self::NameAsc => "name",
            Self::NameDesc => "name-desc",
            Self::Kind => "kind",
            Self::Starter => "starter",
            Self::Newest => "newest",
        }
    }
    fn of(key: &str) -> Self {
        match key {
            "name-desc" => Self::NameDesc,
            "kind" => Self::Kind,
            "starter" => Self::Starter,
            "newest" => Self::Newest,
            _ => Self::NameAsc,
        }
    }
}

pub(crate) const SORTS: [(CatalogSort, &str, &str); 5] = [
    (CatalogSort::NameAsc, "名称 A–Z", "Name A–Z"),
    (CatalogSort::NameDesc, "名称 Z–A", "Name Z–A"),
    (CatalogSort::Kind, "按类型", "By kind"),
    (CatalogSort::Starter, "入门推荐优先", "Starter picks first"),
    (CatalogSort::Newest, "最近核查", "Recently checked"),
];

pub struct CatalogState {
    pub data: Catalog,
    pub search: Entity<TextField>,
    pub kind: Option<&'static str>,
    /// The "can touch" filter; remembered in `settings.json`.
    pub risk: Option<&'static str>,
    pub risk_open: bool,
    /// The license filter (#10); remembered in `settings.json`.
    pub license: Option<String>,
    pub license_open: bool,
    /// Link health from the last user-run check (#4): entry id ->
    /// (alive, check date). Dead entries get a marker in the lists.
    pub health: std::collections::BTreeMap<String, (bool, String)>,
    /// A health check is running: (checked, total).
    pub health_checking: Option<(usize, usize)>,
    /// The duplicate-tidy sheet (#15) is open.
    pub tidy_open: bool,
    /// Show starter picks only.
    pub starters_only: bool,
    /// The last browse results and the inputs that produced them, so
    /// a repaint does not re-filter and re-sort the whole catalog.
    pub results_cache: Option<(String, Option<&'static str>, Option<&'static str>, Option<String>, bool, CatalogSort, Vec<String>, Vec<CatalogEntry>)>,
    /// The quick switcher's catalog hits for the last query it asked
    /// about (it asks several times per frame while open).
    pub switcher_cache: std::cell::RefCell<Option<(String, Vec<CatalogEntry>)>>,
    pub layout: CatalogLayout,
    pub sort: CatalogSort,
    pub sort_open: bool,
    /// The user's starred entry ids, remembered in `settings.json`;
    /// they float to the top of the browse page.
    pub favorites: Vec<String>,
    pub selected: Option<String>,
    /// Keyboard selection in the browse list: an index into the current
    /// results, moved with ↑ ↓ from the search field, Enter opens.
    pub sel: Option<usize>,
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
    pub default: Option<String>,
    pub field: Entity<TextField>,
}

pub struct RunSheet {
    /// What the finished prompt is copied from: the template text.
    pub template: String,
    pub title: String,
    pub fields: Vec<RunField>,
    pub scroll: ScrollHandle,
    /// Named value sets for this prompt (#6): (name, values). Applying
    /// one fills every field at once; the active one's name is kept so
    /// it can be updated or deleted.
    pub profiles: Vec<(String, Vec<(String, String)>)>,
    pub active_profile: Option<String>,
    /// The save-as name box beside the profile chips.
    pub profile_name: Entity<TextField>,
    _subs: Vec<Subscription>,
}

impl CatalogState {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let data = fdl_core::catalog::load_catalog(crate::catalog_data::ENTRIES)
            .unwrap_or_else(|e| { crate::logging::write(&format!("catalog: {e}")); Catalog::default() });
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
            cx.subscribe_in(&search, window, |w, _, ev: &TextFieldEvent, _, cx| {
                match ev {
                    TextFieldEvent::Changed => { w.catalog.sel = Some(0); }
                    TextFieldEvent::Up => w.move_catalog_sel(-1, cx),
                    TextFieldEvent::Down => w.move_catalog_sel(1, cx),
                    TextFieldEvent::Submit => {
                        if let Some(ix) = w.catalog.sel {
                            let results = w.catalog_results(cx);
                            if let Some(e) = results.get(ix) {
                                w.catalog.selected = Some(e.id.clone());
                                w.catalog.detail_scroll.set_offset(point(px(0.), px(0.)));
                            }
                        }
                    }
                    _ => {}
                }
                cx.notify();
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
            risk: None,
            risk_open: false,
            license: None,
            license_open: false,
            health: std::collections::BTreeMap::new(),
            health_checking: None,
            tidy_open: false,
            starters_only: false,
            results_cache: None,
            switcher_cache: std::cell::RefCell::new(None),
            layout: CatalogLayout::Grid,
            sort: CatalogSort::NameAsc,
            sort_open: false,
            favorites: Vec::new(),
            selected: None,
            sel: None,
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

/// Today's date as "2026-11-13" in local civil time.
pub(crate) fn today_str() -> String {
    let (y, m, d, _, _) = fdl_core::time::civil(fdl_core::time::now());
    format!("{y:04}-{m:02}-{d:02}")
}

/// One link check (#4): a HEAD request through the system curl (the
/// same tool the updater uses — no new dependency, https/http only).
/// Gated and method-blocked answers still prove the host is alive.
fn link_alive(url: &str) -> bool {
    fn code(url: &str, head: bool) -> Option<u32> {
        let mut cmd = std::process::Command::new("curl");
        cmd.args(["-s", "-w", "%{http_code}", "--max-time", "10", "--proto", "=https,http"]);
        if head { cmd.arg("-I"); } else { cmd.args(["-r", "0-0"]); }
        let out = cmd.arg(url).output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    match code(url, true).or_else(|| code(url, false)) {
        Some(c) => (200..400).contains(&c) || matches!(c, 401 | 403 | 405 | 429),
        None => false,
    }
}

fn chip(label: &str) -> Div {
    div().flex_none().px(px(8.)).py(px(3.)).rounded(px(999.)).bg(Theme::fill_hover_strong())
        .text_size(px(12.)).line_height(px(16.)).text_color(Theme::text_caption()).child(label.to_string())
}

pub(crate) fn pill_button(id: &'static str, label: &str, primary: bool) -> Stateful<Div> {
    div().id(id).flex_none().h(px(32.)).px(px(14.)).flex().items_center().rounded(px(9.))
        .text_size(px(13.5)).line_height(px(20.)).cursor_pointer()
        .when(primary, |d| d.bg(Theme::text()).text_color(Theme::bg_body()).hover(|s| s.opacity(0.85)))
        .when(!primary, |d| d.border_1().border_color(Theme::line_border()).text_color(Theme::text()).hover(|s| s.bg(Theme::fill_hover())))
        .child(label.to_string())
}

impl Workspace {
    fn catalog_results(&mut self, cx: &App) -> Vec<CatalogEntry> {
        let q = self.catalog.search.read(cx).text.clone();
        let key = (q, self.catalog.kind, self.catalog.risk, self.catalog.license.clone(), self.catalog.starters_only, self.catalog.sort, self.catalog.favorites.clone());
        if let Some((q0, k0, r0, l0, s0, so0, f0, cached)) = &self.catalog.results_cache
            && *q0 == key.0 && *k0 == key.1 && *r0 == key.2 && *l0 == key.3 && *s0 == key.4 && *so0 == key.5 && *f0 == key.6
        {
            return cached.clone();
        }
        let mut results = self.catalog.data.browse(&key.0, key.1, key.2, key.4);
        if let Some(lic) = &key.3 {
            results.retain(|e| &e.license == lic);
        }
        fdl_core::catalog::arrange(&mut results, key.5.key(), &key.6);
        self.catalog.results_cache = Some((key.0, key.1, key.2, key.3, key.4, key.5, key.6, results.clone()));
        results
    }

    /// Every license in the catalog, sorted, for the filter menu (#10).
    fn catalog_licenses(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in &self.catalog.data.entries {
            if !out.contains(&e.license) { out.push(e.license.clone()); }
        }
        out.sort();
        out
    }

    /// Run the link health check (#4): every entry's source URL, six
    /// at a time, only when the user asks — the catalog otherwise
    /// never touches the network. Results land in settings.
    pub(crate) fn start_health_check(&mut self, cx: &mut Context<Self>) {
        if self.catalog.health_checking.is_some() { return; }
        let urls: Vec<(String, String)> = self.catalog.data.entries.iter().map(|e| (e.id.clone(), e.source_url.clone())).collect();
        let total = urls.len();
        self.catalog.health_checking = Some((0, total));
        cx.notify();
        let progress = Arc::new(Mutex::new((0usize, Vec::<(String, bool)>::new())));
        {
            let queue = Arc::new(Mutex::new(urls.into_iter()));
            for _ in 0..6 {
                let queue = queue.clone();
                let progress = progress.clone();
                std::thread::spawn(move || loop {
                    let item = queue.lock().unwrap().next();
                    let Some((id, url)) = item else { break };
                    let ok = link_alive(&url);
                    let mut g = progress.lock().unwrap();
                    g.0 += 1;
                    g.1.push((id, ok));
                });
            }
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
                let (done, results) = { let g = progress.lock().unwrap(); (g.0, g.1.clone()) };
                let finished = done >= total;
                let _ = this.update(cx, |w, cx| {
                    if finished {
                        let date = today_str();
                        for (id, ok) in &results {
                            w.catalog.health.insert(id.clone(), (*ok, date.clone()));
                        }
                        w.catalog.health_checking = None;
                        w.save_catalog_prefs();
                        let dead = results.iter().filter(|(_, ok)| !ok).count();
                        let n = results.len();
                        let msg = if crate::settings::english() {
                            format!("Link check done: {dead} of {n} did not open")
                        } else {
                            format!("链接检查完成：{n} 条里 {dead} 条打不开")
                        };
                        w.show_toast(&msg, cx);
                    } else {
                        w.catalog.health_checking = Some((done, total));
                    }
                    cx.notify();
                });
                if finished { break; }
            }
        }).detach();
    }

    /// Groups of the user's own documents that share a title (#15):
    /// the same prompt saved twice, an import that landed two copies.
    /// Each group is sorted oldest first — the first is the keeper.
    pub(crate) fn duplicate_groups(&self) -> Vec<Vec<crate::store::Entry>> {
        let mut by_title: std::collections::BTreeMap<String, Vec<crate::store::Entry>> = std::collections::BTreeMap::new();
        for e in &self.entries {
            if e.meta.folder.is_some() || e.meta.local() { continue; }
            let key = e.title.trim().to_lowercase();
            if key.is_empty() { continue; }
            by_title.entry(key).or_default().push(e.clone());
        }
        let mut groups: Vec<Vec<crate::store::Entry>> = by_title.into_values().filter(|g| g.len() > 1).collect();
        for g in &mut groups {
            g.sort_by_key(|e| e.meta.created);
        }
        groups.sort_by(|a, b| a[0].title.cmp(&b[0].title));
        groups
    }

    /// The duplicate-tidy sheet (#15): review groups of same-title
    /// documents and send the extra copies to the trash (restorable).
    pub(crate) fn render_tidy_sheet(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.catalog.tidy_open { return None; }
        let groups = self.duplicate_groups();
        let mut body = div().flex().flex_col();
        if groups.is_empty() {
            body = body.child(div().mt(px(18.)).text_size(px(13.5)).line_height(px(20.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("没有发现同名文档 — 你的文档库很整齐。", "No documents share a title — your library is tidy.")));
        }
        for (gi, g) in groups.iter().enumerate() {
            let mut group_box = div().mt(px(16.)).flex().flex_col()
                .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(g[0].title.clone()));
            for (i, e) in g.iter().enumerate() {
                let (y, m, d, _, _) = fdl_core::time::civil(e.meta.modified);
                let when = format!("{y:04}-{m:02}-{d:02}");
                let id = e.id.clone();
                group_box = group_box.child(div().mt(px(6.)).flex().items_center().gap(px(10.))
                    .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(Theme::text_caption())
                        .child(format!("{} · {}", crate::settings::tr("修改于", "Modified"), when)))
                    .when(i == 0, |d| d.child(div().flex_none().text_size(px(12.)).text_color(Theme::text_caption())
                        .child(crate::settings::tr("最早的一份 · 保留", "Earliest copy · kept"))))
                    .when(i > 0, |d| d.child(div().id(SharedString::from(format!("tidy-trash-{gi}-{i}")))
                        .flex_none().h(px(28.)).px(px(12.)).flex().items_center().rounded(px(999.)).cursor_pointer()
                        .border_1().border_color(Theme::line_border()).text_size(px(12.5)).text_color(Theme::text())
                        .hover(|s| s.bg(Theme::fill_hover()))
                        .child(crate::settings::tr("移到回收站", "Move to trash"))
                        .on_click(cx.listener(move |w, _, window, cx| {
                            w.delete_docs(std::slice::from_ref(&id), window, cx);
                            cx.notify();
                        })))));
            }
            body = body.child(group_box);
        }
        let sheet = div().id("tidy-sheet").absolute().inset_0().flex().justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.28))
            .on_click(cx.listener(|w, _, _, cx| { w.catalog.tidy_open = false; cx.notify(); }))
            .child(div().id("tidy-card").mt(px(72.)).w(px(620.)).max_h(px(620.)).p(px(24.)).flex().flex_col().rounded(px(18.)).bg(Theme::bg_body()).shadow(theme::shadow_dialog())
                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(Theme::text()).child(crate::settings::tr("整理重复文档", "Tidy duplicates")))
                .child(div().mt(px(3.)).text_size(px(13.)).text_color(Theme::text_disabled())
                    .child(crate::settings::tr("同名的文档多半是同一篇存了两遍。最早的一份保留，其余的移入回收站 — 30 天内可以恢复。", "Documents with the same title are usually one saved twice. The earliest stays; the rest go to the trash, restorable for 30 days.")))
                .child(div().id("tidy-body").flex_1().min_h_0().overflow_y_scroll().child(body))
                .child(div().mt(px(16.)).flex().items_center()
                    .child(div().flex_1())
                    .child(pill_button("tidy-close", crate::settings::tr("关闭", "Close"), true)
                        .on_click(cx.listener(|w, _, _, cx| { w.catalog.tidy_open = false; cx.notify(); })))));
        Some(sheet.into_any_element())
    }

    /// The user's own entries: `catalog-custom.json` in the library
    /// folder (a JSON array of entry objects) merges over the bundled
    /// catalog — same id replaces, new id appends.
    pub(crate) fn load_custom_catalog(&mut self) {
        let path = crate::store::Store::root_dir().join("catalog-custom.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            match self.catalog.data.merge_custom(&text) {
                Ok(n) => crate::logging::write(&format!("catalog: merged {n} custom entries")),
                Err(e) => crate::logging::write(&format!("catalog: {e}")),
            }
        }
    }

    /// The catalog's view, sort and favorites are remembered
    /// (`settings.json`), the same way the home page remembers its own.
    pub(crate) fn load_catalog_prefs(&mut self) {
        let s = self.store.settings();
        self.catalog.layout = if s.catalog_list { CatalogLayout::List } else { CatalogLayout::Grid };
        self.catalog.sort = s.catalog_sort.as_deref().map(CatalogSort::of).unwrap_or(CatalogSort::NameAsc);
        self.catalog.favorites = s.catalog_favorites.clone();
        self.catalog.risk = s.catalog_risk.as_deref().and_then(|r| RISKS.iter().find(|x| **x == r).copied());
        self.catalog.license = s.catalog_license.clone();
        self.catalog.health = s.catalog_health.iter().filter_map(|(id, v)| {
            let (word, date) = v.split_once(' ')?;
            Some((id.clone(), (word == "ok", date.to_string())))
        }).collect();
    }

    fn save_catalog_prefs(&mut self) {
        let mut s = self.store.settings();
        s.catalog_list = self.catalog.layout == CatalogLayout::List;
        s.catalog_sort = Some(self.catalog.sort.key().to_string());
        s.catalog_favorites = self.catalog.favorites.clone();
        s.catalog_risk = self.catalog.risk.map(|r| r.to_string());
        s.catalog_license = self.catalog.license.clone();
        s.catalog_health = self.catalog.health.iter()
            .map(|(id, (ok, date))| (id.clone(), format!("{} {}", if *ok { "ok" } else { "dead" }, date)))
            .collect();
        if let Err(e) = self.store.save_settings(&s) { eprintln!("无法保存目录视图：{e}"); }
    }

    /// Move the browse list's keyboard selection, wrapping at the ends.
    pub(crate) fn move_catalog_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.catalog_results(cx).len();
        if n == 0 { self.catalog.sel = None; return; }
        let next = match self.catalog.sel {
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.catalog.sel = Some(next);
        self.catalog.scroll.scroll_to_item(next);
        cx.notify();
    }

    pub(crate) fn toggle_favorite(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.catalog.favorites.iter().position(|f| f == id) {
            self.catalog.favorites.remove(pos);
        } else {
            self.catalog.favorites.push(id.to_string());
        }
        self.save_catalog_prefs();
        cx.notify();
    }

    fn fav_button(&self, ix: usize, e: &CatalogEntry, cx: &mut Context<Self>) -> Stateful<Div> {
        let fav = self.catalog.favorites.iter().any(|f| f == &e.id);
        let id = e.id.clone();
        div().id(("catalog-star", ix)).w(px(26.)).h(px(26.)).flex_none().flex().items_center().justify_center().rounded(px(7.)).cursor_pointer()
            .hover(|s| s.bg(Theme::fill_hover()))
            .child(icon(if fav { "star" } else { "star-off" }, 15., if fav { Theme::primary() } else { Theme::text_disabled() }))
            .on_click(cx.listener(move |w, _, _, cx| { cx.stop_propagation(); w.toggle_favorite(&id, cx); }))
    }

    /// Quick copy: put the entry's install command on the clipboard
    /// straight from the card or row, no trip to the detail page.
    fn copy_install_btn(&self, ix: usize, e: &CatalogEntry, cx: &mut Context<Self>) -> Stateful<Div> {
        let text = e.install.clone().unwrap_or_default();
        div().id(("catalog-copy", ix)).w(px(26.)).h(px(26.)).flex_none().flex().items_center().justify_center().rounded(px(7.)).cursor_pointer()
            .hover(|s| s.bg(Theme::fill_hover()))
            .child(icon("copy", 13., Theme::text_caption()))
            .on_click(cx.listener(move |w, _, _, cx| { cx.stop_propagation(); w.copy_text(&text, cx); }))
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
        let results = self.catalog_results(cx);
        let total = self.catalog.data.entries.len();
        // Kind tabs as GitHub-style pills: bordered, the active one filled
        // with the accent color.
        let tab = |id: &'static str, label: &'static str, kind: Option<&'static str>| {
            let on = self.catalog.kind == kind;
            div().id(id).mr(px(8.)).h(px(28.)).px(px(12.)).flex().items_center().rounded(px(999.)).cursor_pointer()
                .border_1().text_size(px(13.)).line_height(px(20.))
                .when(on, |d| d.bg(Theme::primary()).border_color(Theme::primary()).text_color(Theme::bg_body()).font_weight(FontWeight::SEMIBOLD))
                .when(!on, |d| d.border_color(Theme::line_border()).text_color(Theme::text_caption()).hover(|s| s.bg(Theme::fill_hover())))
                .child(label)
                .on_click(cx.listener(move |w, _, _, cx| { w.catalog.kind = kind; cx.notify(); }))
        };
        let mut tabs = div().mt(px(12.)).h(px(28.)).flex_none().pl(px(24.)).pr(px(24.)).flex().items_center()
            .child(tab("catalog-tab-all", crate::settings::tr("全部", "All"), KIND_ALL));
        for (kind, zh, en) in KINDS {
            tabs = tabs.child(tab(kind, crate::settings::tr(zh, en), Some(kind)));
        }
        let starters_on = self.catalog.starters_only;
        tabs = tabs.child(div().id("catalog-tab-starters").h(px(28.)).px(px(12.)).flex().items_center().rounded(px(999.)).cursor_pointer()
            .border_1().text_size(px(13.)).line_height(px(20.))
            .when(starters_on, |d| d.bg(Theme::primary()).border_color(Theme::primary()).text_color(Theme::bg_body()).font_weight(FontWeight::SEMIBOLD))
            .when(!starters_on, |d| d.border_color(Theme::line_border()).text_color(Theme::text_caption()).hover(|s| s.bg(Theme::fill_hover())))
            .child(crate::settings::tr("★ 入门", "★ Starters"))
            .on_click(cx.listener(|w, _, _, cx| { w.catalog.starters_only = !w.catalog.starters_only; cx.notify(); })));
        // Sort menu button + grid/list toggle, at the right of the tabs row.
        let sort_label = SORTS.iter().find(|(s, _, _)| *s == self.catalog.sort).map(|(_, zh, en)| crate::settings::tr(zh, en)).unwrap_or("");
        let sort_open = self.catalog.sort_open;
        let risk_label_text = match self.catalog.risk {
            Some(r) => risk_label(r),
            None => crate::settings::tr("全部", "All"),
        };
        let risk_open = self.catalog.risk_open;
        tabs = tabs.child(div().flex_1())
            .child(div().mr(px(10.)).text_size(px(12.5)).text_color(Theme::text_disabled())
                .child({ let n = results.len(); crate::localized_format!("{n} / {total} 个条目", "{n} of {total} entries") }))
            .child(div().id("catalog-export-btn").mr(px(8.)).h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .hover(|s| s.bg(Theme::fill_hover()))
                .child(crate::settings::tr("导出包", "Export pack"))
                .on_click(cx.listener(|w, _, window, cx| w.open_export_pack(window, cx))))
            .child(div().id("catalog-health-btn").mr(px(8.)).h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .hover(|s| s.bg(Theme::fill_hover()))
                .child(match self.catalog.health_checking {
                    Some((done, total)) => crate::localized_format!("检查中 {done}/{total}", "Checking {done}/{total}"),
                    None => crate::settings::tr("检查链接", "Check links").to_string(),
                })
                .on_click(cx.listener(|w, _, _, cx| w.start_health_check(cx))))
            .child(div().id("catalog-tidy-btn").mr(px(8.)).h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .hover(|s| s.bg(Theme::fill_hover()))
                .child(crate::settings::tr("整理重复", "Tidy duplicates"))
                .on_click(cx.listener(|w, _, _, cx| { w.catalog.tidy_open = true; cx.notify(); })))
            .child(div().id("catalog-license-btn").mr(px(8.)).h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .when(self.catalog.license_open, |d| d.bg(Theme::fill_hover_strong()))
                .when(!self.catalog.license_open, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                .child(format!("{}: {}", crate::settings::tr("许可证", "License"), self.catalog.license.as_deref().unwrap_or(crate::settings::tr("全部", "All"))))
                .child(icon("chevron-down", 12., Theme::text_caption()))
                .on_click(cx.listener(|w, _, _, cx| { w.catalog.license_open = !w.catalog.license_open; w.catalog.sort_open = false; w.catalog.risk_open = false; cx.notify(); })))
            .child(div().id("catalog-risk-btn").h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .when(risk_open, |d| d.bg(Theme::fill_hover_strong()))
                .when(!risk_open, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                .child(format!("{}: {}", crate::settings::tr("能访问", "Can touch"), risk_label_text))
                .child(icon("chevron-down", 12., Theme::text_caption()))
                .on_click(cx.listener(|w, _, _, cx| { w.catalog.risk_open = !w.catalog.risk_open; w.catalog.sort_open = false; w.catalog.license_open = false; cx.notify(); })))
            .child(div().id("catalog-sort-btn").h(px(28.)).px(px(10.)).flex().items_center().gap(px(6.)).rounded(px(9.)).cursor_pointer()
                .border_1().border_color(Theme::line_border()).bg(Theme::bg_body())
                .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text())
                .when(sort_open, |d| d.bg(Theme::fill_hover_strong()))
                .when(!sort_open, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                .child(icon("sort-down", 14., Theme::text_caption()))
                .child(sort_label)
                .child(icon("chevron-down", 12., Theme::text_caption()))
                .on_click(cx.listener(|w, _, _, cx| { w.catalog.sort_open = !w.catalog.sort_open; w.catalog.risk_open = false; w.catalog.license_open = false; cx.notify(); })))
            .child(div().ml(px(10.)).flex().items_center().gap(px(2.)).p(px(2.)).rounded(px(9.)).bg(Theme::fill_hover_strong())
                .child(self.view_toggle_btn("catalog-view-grid", "view-grid", CatalogLayout::Grid, cx))
                .child(self.view_toggle_btn("catalog-view-list", "view-list", CatalogLayout::List, cx)));
        let filtered = !self.catalog.search.read(cx).text.trim().is_empty() || self.catalog.kind.is_some() || self.catalog.risk.is_some() || self.catalog.license.is_some() || self.catalog.starters_only;
        let body: AnyElement = if results.is_empty() {
            let mut wrap = div().relative().flex_1().min_h_0()
                .child(crate::trash::empty_state("illus/empty-list.svg", crate::settings::tr("没有匹配的条目", "No entries match")));
            if filtered {
                wrap = wrap.child(div().absolute().left_0().right_0().bottom(px(150.)).flex().justify_center()
                    .child(pill_button("catalog-clear-filters", crate::settings::tr("清除搜索和筛选", "Clear search & filters"), false)
                        .on_click(cx.listener(|w, _, _, cx| {
                            let f = w.catalog.search.clone();
                            f.update(cx, |f, cx| f.set_text("", cx));
                            w.catalog.kind = None;
                            w.catalog.risk = None;
                            w.catalog.license = None;
                            w.catalog.starters_only = false;
                            w.save_catalog_prefs();
                            cx.notify();
                        }))));
            }
            wrap.into_any_element()
        } else if self.catalog.layout == CatalogLayout::List {
            let mut list = div().flex().flex_col().gap(px(10.)).p(px(24.));
            for (ix, e) in results.iter().enumerate() {
                list = list.child(self.catalog_row(e, ix, cx));
            }
            div().id("catalog-list").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.catalog.scroll).child(list).into_any_element()
        } else {
            let mut grid = div().flex().flex_wrap().gap(px(18.)).p(px(24.));
            for (ix, e) in results.iter().enumerate() {
                grid = grid.child(self.catalog_card(e, ix, cx));
            }
            div().id("catalog-grid").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.catalog.scroll).child(grid).into_any_element()
        };
        let hint = div().flex_none().h(px(28.)).px(px(24.)).flex().items_center()
            .text_size(px(12.)).text_color(Theme::text_disabled())
            .child(crate::settings::tr("↑↓ 选择 · Enter 打开 · 打开条目后按 C 复制 / R 运行 / S 保存 / F 收藏", "↑↓ select · Enter open · in an entry: C copy / R run / S save / F favorite"));
        let page = div().flex_1().min_w_0().h_full().flex().flex_col().child(header).child(tabs).child(body).child(hint);
        let mut root = div().relative().flex_1().min_w_0().h_full().flex().child(page);
        if self.catalog.sort_open || self.catalog.risk_open || self.catalog.license_open {
            root = root
                .child(div().id("catalog-sort-catcher").absolute().inset_0()
                    .on_click(cx.listener(|w, _, _, cx| { w.catalog.sort_open = false; w.catalog.risk_open = false; w.catalog.license_open = false; cx.notify(); })));
            if self.catalog.sort_open { root = root.child(self.catalog_sort_menu(cx)); }
            if self.catalog.risk_open { root = root.child(self.catalog_risk_menu(cx)); }
            if self.catalog.license_open { root = root.child(self.catalog_license_menu(cx)); }
        }
        root.into_any_element()
    }

    fn catalog_license_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut menu = div().flex().flex_col().py(px(6.));
        let add = |menu: Div, ix: usize, license: Option<String>, label: String| {
            let on = self.catalog.license == license;
            menu.child(
                div().id(("catalog-license", ix)).mx(px(6.)).px(px(10.)).h(px(30.)).flex().items_center().gap(px(8.)).rounded(px(8.)).cursor_pointer()
                    .text_size(px(13.)).line_height(px(20.))
                    .when(on, |d| d.text_color(Theme::text()).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |d| d.text_color(Theme::text_caption()))
                    .hover(|s| s.bg(Theme::fill_hover()))
                    .child(div().w(px(14.)).flex_none().flex().items_center().when(on, |d| d.child(icon("check", 13., Theme::primary()))))
                    .child(label)
                    .on_click(cx.listener(move |w, _, _, cx| { w.catalog.license = license.clone(); w.catalog.license_open = false; w.save_catalog_prefs(); cx.notify(); })),
            )
        };
        menu = add(menu, 0, None, crate::settings::tr("全部", "All").to_string());
        for (i, lic) in self.catalog_licenses().into_iter().enumerate() {
            menu = add(menu, i + 1, Some(lic.clone()), lic);
        }
        div().id("catalog-license-menu").absolute().top(px(112.)).right(px(268.)).w(px(230.)).max_h(px(420.)).overflow_y_scroll().rounded(px(12.)).border_1().border_color(Theme::line_border())
            .bg(Theme::bg_body()).shadow(theme::shadow_dialog()).child(menu).into_any_element()
    }

    fn view_toggle_btn(&self, id: &'static str, ic: &'static str, layout: CatalogLayout, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = self.catalog.layout == layout;
        div().id(id).w(px(26.)).h(px(24.)).flex().items_center().justify_center().rounded(px(7.)).cursor_pointer()
            .when(on, |d| d.bg(Theme::bg_body()).shadow(theme::shadow_card()))
            .when(!on, |d| d.hover(|s| s.bg(Theme::fill_hover())))
            .child(icon(ic, 15., if on { Theme::text() } else { Theme::text_caption() }))
            .on_click(cx.listener(move |w, _, _, cx| { w.catalog.layout = layout; w.save_catalog_prefs(); cx.notify(); }))
    }

    fn catalog_sort_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut menu = div().flex().flex_col().py(px(6.));
        for (ix, (sort, zh, en)) in SORTS.iter().enumerate() {
            let on = self.catalog.sort == *sort;
            let sort = *sort;
            menu = menu.child(
                div().id(("catalog-sort", ix)).mx(px(6.)).px(px(10.)).h(px(30.)).flex().items_center().gap(px(8.)).rounded(px(8.)).cursor_pointer()
                    .text_size(px(13.)).line_height(px(20.))
                    .when(on, |d| d.text_color(Theme::text()).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |d| d.text_color(Theme::text_caption()))
                    .hover(|s| s.bg(Theme::fill_hover()))
                    .child(div().w(px(14.)).flex_none().flex().items_center().when(on, |d| d.child(icon("check", 13., Theme::primary()))))
                    .child(crate::settings::tr(zh, en))
                    .on_click(cx.listener(move |w, _, _, cx| { w.catalog.sort = sort; w.catalog.sort_open = false; w.save_catalog_prefs(); cx.notify(); })),
            );
        }
        div().absolute().top(px(112.)).right(px(24.)).w(px(230.)).rounded(px(12.)).border_1().border_color(Theme::line_border())
            .bg(Theme::bg_body()).shadow(theme::shadow_dialog()).child(menu).into_any_element()
    }

    fn catalog_risk_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut menu = div().flex().flex_col().py(px(6.));
        let add = |menu: Div, ix: usize, risk: Option<&'static str>, label: &str| {
            let on = self.catalog.risk == risk;
            menu.child(
                div().id(("catalog-risk", ix)).mx(px(6.)).px(px(10.)).h(px(30.)).flex().items_center().gap(px(8.)).rounded(px(8.)).cursor_pointer()
                    .text_size(px(13.)).line_height(px(20.))
                    .when(on, |d| d.text_color(Theme::text()).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |d| d.text_color(Theme::text_caption()))
                    .hover(|s| s.bg(Theme::fill_hover()))
                    .child(div().w(px(14.)).flex_none().flex().items_center().when(on, |d| d.child(icon("check", 13., Theme::primary()))))
                    .child(label.to_string())
                    .on_click(cx.listener(move |w, _, _, cx| { w.catalog.risk = risk; w.catalog.risk_open = false; w.save_catalog_prefs(); cx.notify(); })),
            )
        };
        menu = add(menu, 0, None, crate::settings::tr("全部", "All"));
        for (i, r) in RISKS.iter().enumerate() {
            menu = add(menu, i + 1, Some(r), risk_label(r));
        }
        div().absolute().top(px(112.)).right(px(268.)).w(px(210.)).rounded(px(12.)).border_1().border_color(Theme::line_border())
            .bg(Theme::bg_body()).shadow(theme::shadow_dialog()).child(menu).into_any_element()
    }

    /// The list layout: the card design stretched wide and cut short —
    /// name, kind and starter on one line, the summary on a single
    /// truncated line, what it can touch at the right.
    fn catalog_row(&self, e: &CatalogEntry, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let id = e.id.clone();
        let risks = if e.risks.is_empty() { vec!["none".to_string()] } else { e.risks.clone() };
        let risk_text = risks.iter().map(|r| risk_label(r)).collect::<Vec<_>>().join(" · ");
        div().id(("catalog-row", ix)).w_full().px(px(16.)).py(px(11.)).flex().items_center().gap(px(14.))
            .rounded(px(10.)).border_1().border_color(Theme::line_border()).bg(Theme::bg_body()).shadow(theme::shadow_card())
            .when(self.catalog.sel == Some(ix), |d| d.border_color(Theme::primary()))
            .cursor_pointer().hover(|s| s.bg(Theme::fill_hover()))
            .child(div().flex_1().min_w_0().flex().flex_col()
                .child(div().flex().items_center().gap(px(8.))
                    .when(self.catalog.health.get(&e.id).is_some_and(|(ok, _)| !ok), |d| d.child(div().flex_none().size(px(8.)).rounded(px(4.)).bg(Theme::danger())))
                    .child(div().text_size(px(15.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::primary()).child(e.name.clone()))
                    .child(chip(kind_label(&e.kind)))
                    .when(e.starter, |d| d.child(div().text_size(px(12.)).text_color(Theme::primary()).child(crate::settings::tr("★ 入门推荐", "★ Starter pick")))))
                .child(div().mt(px(2.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::text_caption())
                    .overflow_hidden().whitespace_nowrap().text_ellipsis().child(e.summary.clone())))
            .child(div().flex_none().flex().flex_col().items_end().gap(px(2.))
                .child(div().text_size(px(12.)).text_color(Theme::text_disabled())
                    .child(format!("{}: {}", crate::settings::tr("能访问", "Can touch"), risk_text)))
                .child(div().text_size(px(12.)).text_color(Theme::text_disabled())
                    .child(format!("{}: {}", crate::settings::tr("许可证", "License"), e.license))))
            .when(e.install.is_some(), |d| d.child(self.copy_install_btn(ix, e, cx)))
            .child(self.fav_button(ix, e, cx))
            .on_click(cx.listener(move |w, _, _, cx| { w.catalog.sel = Some(ix); w.catalog.selected = Some(id.clone()); w.catalog.detail_scroll.set_offset(point(px(0.), px(0.))); cx.notify(); }))
            .into_any_element()
    }

    fn catalog_card(&self, e: &CatalogEntry, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let id = e.id.clone();
        let risks = if e.risks.is_empty() { vec!["none".to_string()] } else { e.risks.clone() };
        let risk_text = risks.iter().map(|r| risk_label(r)).collect::<Vec<_>>().join(" · ");
        div().id(("catalog-card", ix)).w(px(300.)).min_h(px(196.)).p(px(18.)).flex().flex_col()
            .rounded(px(10.)).border_1().border_color(Theme::line_border()).bg(Theme::bg_body()).shadow(theme::shadow_card())
            .when(self.catalog.sel == Some(ix), |d| d.border_color(Theme::primary()))
            .cursor_pointer().hover(|s| s.bg(Theme::fill_hover()))
            .child(div().flex().items_start().gap(px(8.))
                .when(self.catalog.health.get(&e.id).is_some_and(|(ok, _)| !ok), |d| d.child(div().flex_none().mt(px(7.)).size(px(8.)).rounded(px(4.)).bg(Theme::danger())))
                .child(div().flex_1().min_w_0().text_size(px(16.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::primary()).child(e.name.clone()))
                .child(self.fav_button(ix, e, cx)))
            .child(div().mt(px(6.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::text_caption()).child(e.summary.clone()))
            .child(div().flex_1())
            .child(div().mt(px(12.)).flex().items_center().gap(px(8.)).child(chip(kind_label(&e.kind))).child(chip(&e.license))
                .when(e.starter, |d| d.child(div().text_size(px(12.)).text_color(Theme::primary()).child(crate::settings::tr("★ 入门推荐", "★ Starter pick")))))
            .child(div().mt(px(8.)).flex().items_center()
                .child(div().flex_1().text_size(px(12.)).text_color(Theme::text_disabled())
                    .child(format!("{}: {}", crate::settings::tr("能访问", "Can touch"), risk_text)))
                .when(e.install.is_some(), |d| d.child(self.copy_install_btn(ix, e, cx))))
            .on_click(cx.listener(move |w, _, _, cx| { w.catalog.sel = Some(ix); w.catalog.selected = Some(id.clone()); w.catalog.detail_scroll.set_offset(point(px(0.), px(0.))); cx.notify(); }))
            .into_any_element()
    }

    fn render_catalog_detail(&mut self, e: &CatalogEntry, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let header = crate::trash::page_header(self, crate::settings::tr("目录", "Catalog"), 26., cx);
        let today_days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.as_secs() / 86400) as i64).unwrap_or(0);
        let stale = fdl_core::catalog::verified_age_days(&e.verified, today_days).is_some_and(|age| age > 180);
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
            .when(stale, |d| d
                .child(div().mt(px(8.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::text_caption())
                    .child(crate::settings::tr("这条已经很久没有复核了 — 安装前请先到来源处确认。", "Not re-checked in over six months — verify it at the source before installing."))))
            .when(self.catalog.health.get(&e.id).is_some_and(|(ok, _)| !ok), |d| d
                .child(div().mt(px(8.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::danger())
                    .child(crate::settings::tr("上次检查时这个链接打不开 — 来源可能已经下线。", "This link did not open at the last check — the source may be gone."))))
            .when(self.catalog.health.get(&e.id).is_some_and(|(ok, _)| *ok), |d| d
                .child(div().mt(px(8.)).text_size(px(12.5)).line_height(px(18.)).text_color(Theme::text_caption())
                    .child(format!("{} · {}", crate::settings::tr("链接正常", "Link OK"), self.catalog.health.get(&e.id).map(|(_, date)| date.clone()).unwrap_or_default()))))
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
        let fav = self.catalog.favorites.iter().any(|f| f == &e.id);
        let idf = e.id.clone();
        btns = btns.child(pill_button("catalog-fav", if fav { crate::settings::tr("★ 已收藏", "★ Favorited") } else { crate::settings::tr("☆ 加入收藏", "☆ Add to favorites") }, false)
            .on_click(cx.listener(move |w, _, _, cx| w.toggle_favorite(&idf, cx))));
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
        if let Some(existing) = self.entries.iter().find(|en| en.title.trim() == e.name).map(|en| en.id.clone()) {
            self.show_toast(crate::settings::tr("已在你的文档中", "Already in your documents"), cx);
            self.open(&existing, window, cx);
            return;
        }
        let t = crate::settings::tr;
        let labels = fdl_core::catalog::ForkLabels {
            type_: t("类型", "Type").to_string(),
            source: t("来源", "Source").to_string(),
            license: t("许可证", "License").to_string(),
            works_with: t("适用于", "Works with").to_string(),
            can_touch: t("能访问", "Can touch").to_string(),
            verified: t("目录核查", "Catalog verified").to_string(),
            install: t("安装", "Install").to_string(),
            config: t("配置", "Config").to_string(),
            prompt: t("提示词", "Prompt").to_string(),
            my_notes: t("我的笔记", "My notes").to_string(),
            notes_tip: t("安装前请先到来源处核实。服务器和技能运行时拥有真实权限。", "Verify the entry at its source before installing. Servers and skills run with real permissions.").to_string(),
        };
        let md = fdl_core::catalog::fork_markdown_labeled(&e, &labels);
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
                .child(div().mt(px(10.)).text_size(px(12.5)).text_color(Theme::text_disabled())
                    .child(format!("{}: {} — {}", crate::settings::tr("来源", "Source"), e.source_name, e.source_url)))
                .child((if e.risks.is_empty() { vec!["none".to_string()] } else { e.risks.clone() }).iter().fold(div().mt(px(8.)).flex().items_center().gap(px(8.))
                    .child(div().text_size(px(12.5)).text_color(Theme::text_disabled()).child(format!("{}:", crate::settings::tr("能访问", "Can touch")))), |d, r| d.child(chip(risk_label(r)))))
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
                        let mut child = slot.lock().unwrap().take().unwrap();
                        // A hung installer must not run forever: stop
                        // it after two minutes, like Stop would.
                        let start = std::time::Instant::now();
                        let mut timed_out = false;
                        loop {
                            match child.try_wait() {
                                Ok(Some(_)) => break,
                                Ok(None) => {
                                    if start.elapsed() > std::time::Duration::from_secs(120) {
                                        let _ = child.kill();
                                        timed_out = true;
                                        break;
                                    }
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                }
                                Err(_) => break,
                            }
                        }
                        match child.wait_with_output() {
                            Ok(o) => {
                                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                                s.push_str(&String::from_utf8_lossy(&o.stderr));
                                if timed_out { s.push_str("\n(stopped after 120 seconds)"); }
                                let tail: String = s.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                                (tail, o.status.success() && !timed_out)
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
            fields.push(RunField { name: var.name.clone(), default: var.default.clone(), field });
        }
        // Your last answers for this prompt come back prefilled.
        let saved = self.store.settings().run_values.get(&title).cloned().unwrap_or_default();
        for f in &fields {
            if let Some((_, v)) = saved.iter().find(|(n, _)| *n == f.name) && !v.is_empty() {
                let v = v.clone();
                f.field.update(cx, |fld, cx| fld.set_text(&v, cx));
            }
        }
        if let Some(first) = fields.first() {
            window.focus(&first.field.focus_handle(cx));
        }
        let profiles: Vec<(String, Vec<(String, String)>)> = self.store.settings().run_profiles.get(&title)
            .map(|by_name| by_name.iter().map(|(n, v)| (n.clone(), v.clone())).collect())
            .unwrap_or_default();
        let profile_name = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("方案名，如 本地 / 正式", "Profile name, e.g. Local / Production"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        self.catalog.run = Some(RunSheet { template, title, fields, scroll: ScrollHandle::new(), profiles, active_profile: None, profile_name, _subs: subs });
        cx.notify();
    }

    /// Fill the sheet from one saved profile (#6).
    fn apply_run_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(run) = &self.catalog.run else { return };
        let Some((_, values)) = run.profiles.iter().find(|(n, _)| n == name).cloned() else { return };
        for f in &run.fields {
            let v = values.iter().find(|(n, _)| *n == f.name).map(|(_, v)| v.clone()).unwrap_or_default();
            f.field.update(cx, |fld, cx| fld.set_text(&v, cx));
        }
        if let Some(run) = &mut self.catalog.run { run.active_profile = Some(name.to_string()); }
        cx.notify();
    }

    /// Save the sheet's current answers under the typed profile name
    /// (or the active profile's name when the box is empty).
    fn save_run_profile(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &self.catalog.run else { return };
        let typed = run.profile_name.read(cx).text.trim().to_string();
        let name = if !typed.is_empty() { typed } else if let Some(a) = &run.active_profile { a.clone() } else { return };
        let title = run.title.clone();
        let values: Vec<(String, String)> = run.fields.iter()
            .map(|f| (f.name.clone(), f.field.read(cx).text.clone()))
            .filter(|(_, v)| !v.trim().is_empty()).collect();
        if values.is_empty() { return; }
        let mut s = self.store.settings();
        s.run_profiles.entry(title).or_default().insert(name.clone(), values.clone());
        if let Err(e) = self.store.save_settings(&s) { crate::logging::write(&format!("run profiles: {e}")); }
        if let Some(run) = &mut self.catalog.run {
            match run.profiles.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = values,
                None => { run.profiles.push((name.clone(), values)); run.profiles.sort_by(|a, b| a.0.cmp(&b.0)); }
            }
            run.active_profile = Some(name);
        }
        self.show_toast(crate::settings::tr("方案已保存", "Profile saved"), cx);
        cx.notify();
    }

    /// Delete the active profile (#6).
    fn delete_run_profile(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &self.catalog.run else { return };
        let Some(name) = run.active_profile.clone() else { return };
        let title = run.title.clone();
        let mut s = self.store.settings();
        if let Some(by_name) = s.run_profiles.get_mut(&title) { by_name.remove(&name); }
        if let Err(e) = self.store.save_settings(&s) { crate::logging::write(&format!("run profiles: {e}")); }
        if let Some(run) = &mut self.catalog.run {
            run.profiles.retain(|(n, _)| *n != name);
            run.active_profile = None;
        }
        self.show_toast(crate::settings::tr("方案已删除", "Profile deleted"), cx);
        cx.notify();
    }

    /// The profiles row of the Run sheet (#6): one chip per saved
    /// profile (click fills the sheet), a name box, save and delete.
    fn run_profiles_row(&self, cx: &mut Context<Self>) -> Div {
        let Some(run) = &self.catalog.run else { return div() };
        let mut row = div().mt(px(12.)).flex().items_center().gap(px(6.))
            .child(div().flex_none().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                .child(crate::settings::tr("方案", "Profiles")));
        for (i, (name, _)) in run.profiles.iter().enumerate() {
            let active = run.active_profile.as_deref() == Some(name.as_str());
            let name2 = name.clone();
            row = row.child(div().id(("run-profile", i)).flex_none().px(px(10.)).py(px(4.)).rounded(px(999.)).cursor_pointer()
                .border_1().border_color(if active { Theme::primary() } else { Theme::line_border() })
                .when(active, |d| d.bg(Theme::fill_hover()))
                .text_size(px(12.)).text_color(Theme::text())
                .on_click(cx.listener(move |w, _, _, cx| w.apply_run_profile(&name2, cx)))
                .child(name.clone()));
        }
        row.child(div().flex_none().w(px(180.)).h(px(30.)).px(px(9.)).flex().items_center().rounded(px(8.)).border_1().border_color(Theme::line_border())
                .child(div().flex_1().min_w_0().flex().child(run.profile_name.clone())))
            .child(pill_button("run-profile-save", crate::settings::tr("存为方案", "Save as profile"), false)
                .on_click(cx.listener(|w, _, _, cx| w.save_run_profile(cx))))
            .when(run.active_profile.is_some(), |d| d.child(pill_button("run-profile-delete", crate::settings::tr("删除方案", "Delete profile"), false)
                .on_click(cx.listener(|w, _, _, cx| w.delete_run_profile(cx)))))
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
                .child(self.run_profiles_row(cx))
                .child(div().id("run-fields").flex_1().min_h_0().overflow_y_scroll().track_scroll(&run.scroll)
                    .child(fields_col)
                    .child(div().mt(px(16.)).text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(crate::settings::tr("预览", "Preview")))
                    .child(div().mt(px(5.)).p(px(12.)).rounded(px(9.)).bg(Theme::bg_overlay()).border_1().border_color(Theme::line_border())
                        .text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(preview)))
                .child(div().mt(px(16.)).flex().items_center()
                    .child(div().text_size(px(12.5)).text_color(Theme::text_disabled())
                        .child(crate::localized_format!("已填 {filled} / {total}", "Filled {filled} of {total}")))
                    .child(div().flex_1())
                    .when(run.fields.iter().any(|f| f.default.is_some()), |d| d
                        .child(pill_button("run-defaults", crate::settings::tr("填入默认值", "Use defaults"), false)
                            .on_click(cx.listener(|w, _, _, cx| {
                                if let Some(run) = &w.catalog.run {
                                    for f in &run.fields {
                                        if let Some(def) = &f.default {
                                            let def = def.clone();
                                            f.field.update(cx, |fld, cx| fld.set_text(&def, cx));
                                        }
                                    }
                                }
                                cx.notify();
                            })))
                        .child(div().w(px(10.))))
                    .child(pill_button("run-copy", crate::settings::tr("复制提示词", "Copy prompt"), complete)
                        .when(complete, |d| d.on_click(cx.listener(move |w, _, _, cx| {
                            let run = w.catalog.run.as_ref().unwrap();
                            let title = run.title.clone();
                            let values: Vec<(String, String)> = run.fields.iter().map(|f| (f.name.clone(), f.field.read(cx).text.clone())).collect();
                            if let Ok(text) = fdl_core::prompt::render(&run.template, &values) {
                                w.copy_text(&text, cx);
                                w.catalog.run = None;
                                // Remember the answers for next time.
                                let mut s = w.store.settings();
                                s.run_values.insert(title, values.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect());
                                while s.run_values.len() > 20 {
                                    let first = s.run_values.keys().next().unwrap().clone();
                                    s.run_values.remove(&first);
                                }
                                if let Err(e) = w.store.save_settings(&s) { crate::logging::write(&format!("run values: {e}")); }
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
        let mut results: Vec<CatalogEntry> = self.catalog.data.search(&q, Some("prompt")).into_iter().cloned().collect();
        fdl_core::catalog::arrange(&mut results, "starter", &[]);
        results
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
            let blanks = e.body.as_deref().map(|b| fdl_core::prompt::parse_template(b).variables.len()).unwrap_or(0);
            list = list.child(
                div().id(("insert-prompt", i)).flex_none().px(px(12.)).py(px(9.)).flex().items_center().rounded(px(8.)).cursor_pointer()
                    .when(blanks > 0, |d| d.child(div().absolute().right(px(12.)).text_size(px(11.5)).text_color(Theme::text_disabled())
                        .child(format!("{} {}", blanks, crate::settings::tr("个空格", "blanks")))))
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

#[cfg(test)]
mod tests {
    /// The shipped data itself must stay valid as the catalog grows:
    /// unique ids, known kinds and risk tags, real dates, and prompt
    /// entries that actually carry a prompt.
    #[test]
    fn bundled_catalog_data_is_valid() {
        let catalog = fdl_core::catalog::load_catalog(crate::catalog_data::ENTRIES).unwrap();
        assert!(catalog.entries.len() >= 100);
        let problems = fdl_core::catalog::validate_catalog(&catalog);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
