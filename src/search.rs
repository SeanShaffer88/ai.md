//! 搜索 dialog: a 540-wide white dialog with the input row, 最近浏览 / hit
//! rows and a gray footer. Locally there is one owner and no sessions or
//! spaces, so each row's extra line is just 所有者 + the opened / modified
//! times. Radius 16, quiet row fills, monochrome page glyphs.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::store::Entry;
use crate::theme::{self, Theme};
use crate::trash::scrim;
use crate::ui::{ROW_RADIUS, format_visit, icon, ui_runs, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::ops::Range;

/// How many documents 最近浏览 lists.
const RECENT: usize = 20;
/// Body text kept for a hit's two-line summary.
const SUMMARY_CHARS: usize = 160;
/// The summary joins its blocks with three no-break spaces.
const SUMMARY_SEP: &str = "\u{a0}\u{a0}\u{a0}";
/// Text column of a row: 540 − list padding 6 / 6 (the right one holds the
/// scrollbar) − row padding 12 / 8 − icon 24 + 10.
const INFO_W: f32 = 474.;

/// One row of the unified palette: a document, a catalog entry (the flag
/// marks the prompt kinds, which get their own section), or an action.
#[derive(Clone)]
pub(crate) enum PalRow {
    Doc(String),
    Entry(String, bool),
    Action(&'static str),
}

/// Palette sections in display order.
#[derive(Clone, Copy, PartialEq)]
enum PalSection { Docs, Catalog, Prompts, Actions }

fn section_of(row: &PalRow) -> PalSection {
    match row {
        PalRow::Doc(_) => PalSection::Docs,
        PalRow::Entry(_, prompt) => if *prompt { PalSection::Prompts } else { PalSection::Catalog },
        PalRow::Action(_) => PalSection::Actions,
    }
}

fn section_title(section: PalSection, empty_query: bool) -> &'static str {
    match section {
        PalSection::Docs => if empty_query { crate::settings::tr("最近浏览", "Recently viewed") } else { crate::settings::tr("笔记", "Notes") },
        PalSection::Catalog => crate::settings::tr("目录", "Catalog"),
        PalSection::Prompts => if empty_query { crate::settings::tr("置顶提示词", "Pinned prompts") } else { crate::settings::tr("提示词", "Prompts") },
        PalSection::Actions => crate::settings::tr("操作", "Actions"),
    }
}

fn section_header(title: &'static str) -> Div {
    div().flex_none().h(px(36.)).pt(px(2.)).px(px(12.)).flex().items_center()
        .text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).child(title)
}

/// The prompt kinds group together in the palette and the catalog alike.
pub(crate) fn is_prompt_kind(kind: &str) -> bool { kind == "prompt" || kind == "prompt-pack" }

/// A typed palette query: `mcp:` / `skill:` / `guide:` / `prompt:` filter
/// the catalog by kind, `note:` searches documents only, and a leading
/// `>` lists actions. Anything else searches everything at once.
struct PalQuery { kind: Option<&'static str>, docs_only: bool, actions_only: bool, text: String }

fn parse_query(raw: &str) -> PalQuery {
    let mut pq = PalQuery { kind: None, docs_only: false, actions_only: false, text: raw.to_string() };
    if let Some(rest) = raw.strip_prefix('>') {
        pq.actions_only = true;
        pq.text = rest.trim().to_string();
        return pq;
    }
    for (prefix, kind) in [("mcp:", "mcp"), ("skill:", "skill"), ("guide:", "guide"), ("prompt:", "prompt")] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            pq.kind = Some(kind);
            pq.text = rest.trim().to_string();
            return pq;
        }
    }
    if let Some(rest) = raw.strip_prefix("note:") {
        pq.docs_only = true;
        pq.text = rest.trim().to_string();
    }
    pq
}

/// One palette action: an existing app command the palette can run.
struct PalAction { id: &'static str, en: &'static str, zh: &'static str, glyph: &'static str }

const PAL_ACTIONS: &[PalAction] = &[
    PalAction { id: "new-doc", en: "New document", zh: "新建文档", glyph: "plus" },
    PalAction { id: "go-catalog", en: "Open Catalog", zh: "打开目录", glyph: "library" },
    PalAction { id: "go-home", en: "Go to library home", zh: "回到文档库首页", glyph: "home-new" },
    PalAction { id: "run-prompt", en: "Run a prompt…", zh: "运行提示词…", glyph: "braces" },
    PalAction { id: "quick-capture", en: "Quick capture…", zh: "快速捕获…", glyph: "square-pen" },
    PalAction { id: "insert-prompt", en: "Insert a prompt…", zh: "插入提示词…", glyph: "file-text" },
    PalAction { id: "settings", en: "Open settings", zh: "打开设置", glyph: "ellipsis" },
    PalAction { id: "appearance", en: "Appearance settings", zh: "外观设置", glyph: "eye" },
];

fn pal_action(id: &str) -> Option<&'static PalAction> { PAL_ACTIONS.iter().find(|a| a.id == id) }

/// Case-insensitive match position in `hay`, in `hay`'s own byte offsets.
pub(crate) fn find_ci(hay: &str, needle_lower: &str) -> Option<Range<usize>> {
    if needle_lower.is_empty() {
        return None;
    }
    let lower = hay.to_lowercase();
    if lower.len() == hay.len() {
        let i = lower.find(needle_lower)?;
        (hay.is_char_boundary(i) && hay.is_char_boundary(i + needle_lower.len())).then(|| i..i + needle_lower.len())
    } else {
        let i = hay.find(needle_lower)?;
        Some(i..i + needle_lower.len())
    }
}

/// Every match of `q` in `hay`.
fn find_all(hay: &str, q: &str) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut at = 0;
    while let Some(m) = find_ci(&hay[at..], q) {
        out.push(at + m.start..at + m.end);
        at += m.end;
    }
    out
}

/// Body text from the block holding the first match, with about 16
/// characters of context before it and the following blocks after it.
pub(crate) fn snippet(body: &str, q: &str) -> Option<(String, Vec<Range<usize>>)> {
    let m = find_ci(body, q)?;
    let line_start = body[..m.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let before: Vec<(usize, char)> = body[line_start..m.start].char_indices().collect();
    let cut = if before.len() > 16 { line_start + before[before.len() - 16].0 } else { line_start };
    let mut text = String::from(if cut > line_start { "…" } else { "" });
    let mut chars = 0;
    for line in body[cut..].lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        if !text.is_empty() && text != "…" {
            text.push_str(SUMMARY_SEP);
        }
        for ch in line.chars() {
            text.push(ch);
            chars += 1;
            if chars >= SUMMARY_CHARS {
                break;
            }
        }
        if chars >= SUMMARY_CHARS {
            break;
        }
    }
    let hits = find_all(&text, q);
    Some((text, hits))
}

/// The summary as two lines at most, ending in "…" when cut. GPUI's own
/// `line_clamp` truncates at twice the width, ignoring wrap leftovers.
pub(crate) fn summary_text(text: String, mut hits: Vec<Range<usize>>, width: f32, window: &Window) -> StyledText {
    let mut style = window.text_style();
    style.font_family = theme::UI_FONT.into();
    style.font_size = px(12.).into();
    style.color = Theme::text_caption();
    let lines = |s: &str| {
        window
            .text_system()
            .shape_text(s.to_string().into(), px(12.), &ui_runs(s, &style, &[], style.color), Some(px(width)), None)
            .map(|ls| ls.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>())
            .unwrap_or(1)
    };
    let text = if lines(&text) <= 2 {
        text
    } else {
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut lo, mut hi) = (0, ends.len());
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if lines(&format!("{}…", &text[..ends[mid]])) <= 2 { lo = mid } else { hi = mid }
        }
        let cut = ends[lo];
        hits.retain(|r| r.end <= cut);
        format!("{}…", &text[..cut])
    };
    let runs = ui_runs(&text, &style, &hits, Theme::link());
    StyledText::new(text).with_runs(runs)
}

/// A result's icon: the page's emoji, or the page glyph.
pub(crate) fn entry_icon(e: &Entry) -> Div {
    match &e.icon {
        _ if e.meta.markdown => doc_icon_named("doc-md"),
        Some(em) => div().flex_none().size(px(24.)).mr(px(10.)).flex().items_center().justify_center().text_size(px(20.)).child(em.clone()),
        None => doc_icon(),
    }
}

/// A page without an emoji: the monochrome page glyph (the sidebar's), in
/// a 24 slot.
pub(crate) fn doc_icon() -> Div { doc_icon_named("doc") }

/// A page glyph by name (`doc`, `doc-md` for a Markdown file), in a 24 slot.
pub(crate) fn doc_icon_named(name: &str) -> Div {
    div()
        .flex_none()
        .size(px(24.))
        .mr(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name, 18., Theme::icon()))
}

/// The 24px square buttons at the right of the input row.
fn input_button(id: &'static str, glyph: &'static str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .hover(|s| s.bg(Theme::fill_hover()))
        .child(icon(glyph, size, color))
}

/// One `·`-separated part of a row's extra line.
fn extra_segment(glyph: Option<&'static str>, text: String) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .children(glyph.map(|g| icon(g, 12., Theme::icon()).mr(px(4.))))
        .child(ui_text(text))
}

/// A result's extra line: 所有者 and the opened / modified times.
pub(crate) fn extra_line(e: &Entry) -> Div {
    let mut extra = vec![extra_segment(None, crate::settings::tr("所有者：我", "Owner: me").into())];
    if e.meta.opened > 0 {
        extra.push(extra_segment(Some("eye"), if crate::settings::english() { format!("Opened {}", format_visit(e.meta.opened)) } else { format!("你在 {}打开过", format_visit(e.meta.opened)) }));
    }
    extra.push(extra_segment(Some("pen-line"), if crate::settings::english() { format!("Modified {}", format_visit(e.meta.modified)) } else { format!("我在 {}修改过", format_visit(e.meta.modified)) }));
    let last = extra.len() - 1;
    let extra = extra.into_iter().enumerate().flat_map(|(i, seg)| {
        let seg = if i == last { seg.flex_shrink().min_w_0().overflow_hidden() } else { seg };
        [(i > 0).then(|| div().flex_none().child(" · ")), Some(seg)].into_iter().flatten()
    });
    div()
        .mt(px(2.))
        .h(px(20.))
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(12.))
        .line_height(px(20.))
        .text_color(Theme::text_caption())
        .children(extra)
}

/// A result's title (13pt semibold, hits in the link colour), summary and extra line.
pub(crate) fn result_info(e: &Entry, q: &str, summary: Option<(String, Vec<Range<usize>>)>, width: f32, window: &Window) -> Div {
    let title = display_title(&e.title);
    let title_hit = find_ci(&title, q);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::text())
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(ui_text(title).hit(title_hit, Theme::link())),
        )
        .children(summary.map(|(text, hits)| div().text_size(px(12.)).line_height(px(20.)).child(summary_text(text, hits, width, window))))
        .child(extra_line(e))
}

impl Workspace {
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        self.search_sel = Some(0);
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        let search = self.search.clone();
        search.update(cx, |s, cx| {
            // The dialog's query reads at body size, above the 13pt rows.
            s.size = 15.;
            s.line_height = 22.;
            s.set_text("", cx)
        });
        window.focus(&search.focus_handle(cx));
        self.search_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        if let Some(ed) = &self.editor {
            ed.read(cx).focus(window);
        }
        cx.notify();
    }

    /// First result is selected; an empty result list has no selection.
    pub(crate) fn default_search_sel(&self, cx: &App) -> Option<usize> {
        (self.search_total(cx) > 0).then_some(0)
    }

    /// Catalog entries matching the query (the quick switcher searches the
    /// library and the catalog together; catalog hits list after documents).
    pub(crate) fn catalog_hits(&self, cx: &App) -> Vec<fdl_core::catalog::CatalogEntry> {
        let raw = self.query(cx);
        let pq = parse_query(&raw);
        if pq.text.is_empty() || pq.docs_only || pq.actions_only { return vec![]; }
        // Asked several times per frame while the switcher is open;
        // the answer only changes with the query.
        if let Some((q0, hits)) = &*self.catalog.switcher_cache.borrow() && *q0 == raw {
            return hits.clone();
        }
        // A `prompt:` query searches all kinds, then keeps the prompt
        // kinds; the other prefixes map straight onto the kind filter.
        let kind = match pq.kind { Some("prompt") => None, k => k };
        let mut hits: Vec<fdl_core::catalog::CatalogEntry> = self.catalog.data.search(&pq.text, kind).into_iter().cloned().collect();
        if pq.kind == Some("prompt") { hits.retain(|e| is_prompt_kind(&e.kind)); }
        *self.catalog.switcher_cache.borrow_mut() = Some((raw, hits.clone()));
        hits
    }

    /// The palette's actions matching `text` (an empty text matches all).
    /// Insert-a-prompt only exists while a document is open.
    fn palette_actions(&self, text: &str) -> Vec<&'static PalAction> {
        PAL_ACTIONS.iter().filter(|a| {
            (a.id != "insert-prompt" || self.editor.is_some())
                && (text.is_empty() || a.en.to_lowercase().contains(text) || a.zh.contains(text))
        }).collect()
    }

    /// Every palette row in display order: documents, catalog entries,
    /// prompts, then actions. An empty query lists the recently viewed
    /// documents and the pinned (favorited) prompts.
    pub(crate) fn palette_rows(&self, cx: &App) -> Vec<PalRow> {
        let raw = self.query(cx);
        let pq = parse_query(&raw);
        if pq.actions_only {
            return self.palette_actions(&pq.text).into_iter().map(|a| PalRow::Action(a.id)).collect();
        }
        let mut rows = vec![];
        if pq.kind.is_none() {
            let docs = if pq.docs_only { self.search_results_for(&pq.text) } else { self.search_results(cx) };
            rows.extend(docs.into_iter().map(|e| PalRow::Doc(e.id.clone())));
        }
        if !pq.docs_only {
            if raw.is_empty() {
                for id in &self.catalog.favorites {
                    if let Some(e) = self.catalog.entry(id) && is_prompt_kind(&e.kind) {
                        rows.push(PalRow::Entry(id.clone(), true));
                    }
                }
            } else {
                let hits = self.catalog_hits(cx);
                rows.extend(hits.iter().filter(|h| !is_prompt_kind(&h.kind)).map(|h| PalRow::Entry(h.id.clone(), false)));
                rows.extend(hits.iter().filter(|h| is_prompt_kind(&h.kind)).map(|h| PalRow::Entry(h.id.clone(), true)));
                rows.extend(self.palette_actions(&pq.text).into_iter().map(|a| PalRow::Action(a.id)));
            }
        }
        rows
    }

    fn run_palette_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            // Stay in the palette, narrowed to prompts.
            "run-prompt" => {
                self.search.update(cx, |s, cx| s.set_text("prompt:", cx));
                self.search_sel = Some(0);
                self.search_scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }
            "quick-capture" => { self.close_search(window, cx); crate::quick_capture::open(cx); }
            "new-doc" => { self.close_search(window, cx); self.new_doc(window, cx); }
            "go-catalog" => { self.close_search(window, cx); self.show_view(crate::app::View::Catalog, cx); }
            "go-home" => { self.close_search(window, cx); self.show_view(crate::app::View::Home, cx); }
            "insert-prompt" => { self.close_search(window, cx); self.open_insert_palette(window, cx); }
            "settings" => { self.close_search(window, cx); self.open_settings(crate::settings::Page::General, cx); }
            "appearance" => { self.close_search(window, cx); self.open_settings(crate::settings::Page::Appearance, cx); }
            _ => {}
        }
    }

    fn search_total(&self, cx: &App) -> usize {
        self.palette_rows(cx).len()
    }

    pub(crate) fn pick_search(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.palette_rows(cx).get(row).cloned() else { return };
        match r {
            PalRow::Doc(id) => {
                self.close_search(window, cx);
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor {
                    ed.read(cx).focus(window);
                }
            }
            PalRow::Entry(id, _) => {
                self.close_search(window, cx);
                self.open_catalog_entry(&id, cx);
            }
            PalRow::Action(a) => self.run_palette_action(a, window, cx),
        }
    }

    pub(crate) fn move_search_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.palette_rows(cx);
        let n = rows.len();
        if n == 0 { self.search_sel = None; return; }
        let next = match self.search_sel {
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.search_sel = Some(next);
        // Section headers are list items too; count the ones at or
        // before the selection so scrolling lands on the row itself.
        let mut headers = 0;
        let mut prev: Option<PalSection> = None;
        for r in &rows[..=next] {
            let s = section_of(r);
            if prev != Some(s) { headers += 1; prev = Some(s); }
        }
        self.search_scroll.scroll_to_item(next + headers);
        cx.notify();
    }

    fn palette_catalog_row(&self, hit: &fdl_core::catalog::CatalogEntry, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        div().id(("catalog-hit", ix)).flex_none().pt(px(8.)).pr(px(8.)).pb(px(8.)).pl(px(12.))
            .flex().items_center().rounded(px(ROW_RADIUS)).cursor_pointer()
            .when(self.search_sel == Some(ix), |d| d.bg(Theme::fill_hover()))
            .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
                if *hovered && w.search_sel != Some(ix) { w.search_sel = Some(ix); cx.notify(); }
            }))
            .on_click(cx.listener(move |w, _, window, cx| w.pick_search(ix, window, cx)))
            .child(crate::ui::icon("library", 16., Theme::icon()))
            .child(div().ml(px(10.)).flex_1().min_w_0().text_size(px(13.5)).text_color(Theme::text())
                .overflow_hidden().whitespace_nowrap().text_ellipsis().child(hit.name.clone()))
            .child(div().flex_none().text_size(px(12.)).text_color(Theme::text_disabled())
                .child(format!("{} · {}", crate::settings::tr("目录", "Catalog"), crate::catalog::kind_label(&hit.kind))))
    }

    /// Peek card (#2): a preview of the palette's selected catalog
    /// entry, shown beside the dialog so a hit can be judged — kind,
    /// summary, license, risks, source, last check — without opening
    /// it. Prompt entries also preview the start of their text.
    fn palette_peek(&self, e: &fdl_core::catalog::CatalogEntry, h: f32) -> Div {
        let meta: [(&str, &str, String); 4] = [
            ("许可证", "License", e.license.clone()),
            ("风险", "Risk", if e.risks.is_empty() { "—".into() } else { e.risks.join(" · ") }),
            ("来源", "Source", e.source_name.clone()),
            ("上次核查", "Last checked", e.verified.clone()),
        ];
        let mut card = div().flex_none().w(px(300.)).h(px(h)).flex().flex_col()
            .rounded(px(16.)).bg(crate::theme::raised(Theme::bg_body())).shadow(theme::shadow_dialog())
            .px(px(16.)).py(px(14.))
            .child(div().flex_none().flex().items_center()
                .child(div().flex_1().min_w_0().text_size(px(12.)).text_color(Theme::text_caption())
                    .overflow_hidden().whitespace_nowrap().text_ellipsis()
                    .child(format!("{} · {}", crate::settings::tr("目录", "Catalog"), crate::catalog::kind_label(&e.kind))))
                .when(self.catalog.favorites.contains(&e.id), |d| d.child(div().flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(crate::settings::tr("已收藏", "Favorited")))))
            .child(div().flex_none().mt(px(6.)).text_size(px(15.)).line_height(px(21.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(e.name.clone()))
            .child(div().flex_none().mt(px(6.)).text_size(px(12.5)).line_height(px(19.)).text_color(Theme::text()).line_clamp(6).child(e.summary.clone()));
        if let Some(body) = &e.body {
            let preview: String = body.chars().take(240).collect();
            card = card.child(div().flex_none().mt(px(10.)).px(px(10.)).py(px(8.)).rounded(px(8.)).bg(Theme::fill_hover())
                .text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).line_clamp(5).child(preview));
        }
        if e.install.is_some() {
            card = card.child(div().flex_none().mt(px(10.)).text_size(px(12.)).text_color(Theme::text_caption())
                .child(crate::settings::tr("附带安装命令（运行时需再次同意）", "Includes an install command (asks again before running)")));
        }
        let mut meta_box = div().flex_none().mt(px(12.)).flex().flex_col().gap(px(5.));
        for (zh, en, value) in meta {
            meta_box = meta_box.child(div().flex().items_baseline()
                .child(div().flex_none().w(px(76.)).text_size(px(12.)).text_color(Theme::text_caption()).child(crate::settings::tr(zh, en)))
                .child(div().flex_1().min_w_0().text_size(px(12.5)).text_color(Theme::text()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(value)));
        }
        card.child(meta_box)
            .child(div().flex_1())
            .child(div().flex_none().text_size(px(12.)).text_color(Theme::text_disabled())
                .child(crate::settings::tr("Enter 打开详情", "Enter to open the full entry")))
    }

    fn palette_action_row(&self, a: &'static PalAction, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let name = if crate::settings::english() { a.en } else { a.zh };
        div().id(("palette-action", ix)).flex_none().pt(px(8.)).pr(px(8.)).pb(px(8.)).pl(px(12.))
            .flex().items_center().rounded(px(ROW_RADIUS)).cursor_pointer()
            .when(self.search_sel == Some(ix), |d| d.bg(Theme::fill_hover()))
            .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
                if *hovered && w.search_sel != Some(ix) { w.search_sel = Some(ix); cx.notify(); }
            }))
            .on_click(cx.listener(move |w, _, window, cx| w.pick_search(ix, window, cx)))
            .child(crate::ui::icon(a.glyph, 16., Theme::icon()))
            .child(div().ml(px(10.)).flex_1().min_w_0().text_size(px(13.5)).text_color(Theme::text())
                .overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
            .child(div().flex_none().text_size(px(12.)).text_color(Theme::text_disabled())
                .child(crate::settings::tr("操作", "Action")))
    }

    /// Title hits first, then body hits; with no query, 最近浏览.
    pub fn search_results(&self, cx: &App) -> Vec<&Entry> {
        self.search_results_for(&self.query(cx))
    }

    fn search_results_for(&self, q: &str) -> Vec<&Entry> {
        let q = q.to_string();
        if q.is_empty() {
            let mut recent: Vec<&Entry> = self.entries.iter().collect();
            recent.sort_by_key(|e| std::cmp::Reverse(e.meta.opened.max(e.meta.modified)));
            recent.truncate(RECENT);
            return recent;
        }
        let mut hits: Vec<(u8, &Entry)> = self
            .entries
            .iter()
            .filter_map(|e| {
                if e.title.to_lowercase().contains(&q) {
                    Some((0, e))
                } else if e.body.to_lowercase().contains(&q) {
                    Some((1, e))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by_key(|(rank, _)| *rank);
        hits.into_iter().map(|(_, e)| e).collect()
    }

    fn search_row(&self, e: &Entry, ix: usize, q: &str, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = snippet(&e.body, q);
        // Result indices match keyboard selection.
        let row = ix;
        div()
            .id(("search-hit", ix))
            .flex_none()
            .pt(px(8.))
            .pr(px(8.))
            .pb(px(8.))
            .pl(px(12.))
            .flex()
            .items_center()
            .rounded(px(ROW_RADIUS))
            .cursor_pointer()
            .when(self.search_sel == Some(row), |d| d.bg(Theme::fill_hover()))
            .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
                if *hovered && w.search_sel != Some(row) {
                    w.search_sel = Some(row);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |w, _, window, cx| w.pick_search(row, window, cx)))
            .child(entry_icon(e))
            .child(result_info(e, q, summary, INFO_W, window))
    }

    pub(crate) fn render_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.search_open {
            return None;
        }
        let q = self.query(cx);
        let has_text = !self.search.read(cx).text.is_empty();
        let rows = self.palette_rows(cx);
        let vh = f32::from(window.viewport_size().height);
        let modal_h = 630f32.min(vh - 120.).max(200.);
        // Peek (#2): beside the dialog when the selection is a catalog
        // entry and the window is wide enough for both cards.
        let peek = if f32::from(window.viewport_size().width) > 900. {
            self.search_sel.and_then(|i| rows.get(i)).and_then(|r| match r {
                PalRow::Entry(id, _) => self.catalog.entry(id).map(|e| self.palette_peek(e, modal_h)),
                _ => None,
            })
        } else { None };

        let input = div()
            .flex_none()
            .h(px(46.))
            .pl(px(12.))
            .pr(px(8.))
            .flex()
            .items_center()
            .child(
                // The glyph's ink fills 14.6 of the 16px box; Lucide's fills 20/24.
                div()
                    .relative()
                    .flex_none()
                    .size(px(16.))
                    .ml(px(4.))
                    .mr(px(10.))
                    .child(icon("search", 17.5, Theme::icon()).absolute().top(px(-0.75)).left(px(-0.75))),
            )
            .child(div().flex_1().min_w_0().pl(px(2.)).flex().child(self.search.clone()))
            .when(has_text, |d| {
                d.child(input_button("search-clear", "search-clear", 18., Theme::text_disabled()).ml(px(8.)).on_click(cx.listener(
                    |w, _, window, cx| {
                        w.search.update(cx, |s, cx| s.set_text("", cx));
                        window.focus(&w.search.focus_handle(cx));
                        w.search_sel = Some(0);
                        w.search_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    },
                )))
            })
            ;

        let mut list = div()
            .id("search-results")
            .flex_1()
            .min_h_0()
            .pt(px(4.))
            .px(px(6.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.search_scroll);
        if rows.is_empty() && !q.is_empty() {
            list = list.child(
                div()
                    .flex_none()
                    .m(px(8.))
                    .px(px(10.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(Theme::text_caption())
                    .child(crate::settings::text("没有找到匹配的结果，尝试其他关键词进行搜索")),
            );
        }
        let text_q = parse_query(&q).text;
        let mut prev: Option<PalSection> = None;
        for (i, row) in rows.iter().enumerate() {
            let section = section_of(row);
            if prev != Some(section) {
                list = list.child(section_header(section_title(section, q.is_empty())));
                prev = Some(section);
            }
            list = match row {
                PalRow::Doc(id) => match self.entries.iter().find(|e| &e.id == id) {
                    Some(e) => list.child(self.search_row(e, i, &text_q, window, cx)),
                    None => list,
                },
                PalRow::Entry(id, _) => match self.catalog.entry(id) {
                    Some(hit) => list.child(self.palette_catalog_row(hit, i, cx)),
                    None => list,
                },
                PalRow::Action(aid) => match pal_action(aid) {
                    Some(a) => list.child(self.palette_action_row(a, i, cx)),
                    None => list,
                },
            };
        }

        let footer = div()
            .flex_none()
            .h(px(36.))
            .pl(px(14.))
            .pr(px(8.))
            .flex()
            .items_center()
            // GPUI clips children to the dialog's rect, not its rounded corners.
            .rounded_b(px(16.))
            .border_t_1()
            .border_color(Theme::line_divider())
            .bg(Theme::bg_overlay())
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(Theme::text_caption())
            .whitespace_nowrap()
            .overflow_hidden()
            .child(ui_text(crate::settings::tr("支持 ↑↓ 键选择、Enter 键打开 · 输入 > 查找操作", "Use ↑↓ to select, Enter to open · type > for actions")));

        let modal = div()
            .id("search-dialog")
            .occlude()
            .w(px(540.))
            .h(px(modal_h))
            .flex()
            .flex_col()
            .rounded(px(16.))
            .bg(crate::theme::raised(Theme::bg_body()))
            .shadow(theme::shadow_dialog())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(input)
            .child(div().flex_none().h(px(1.)).bg(Theme::line_divider()))
            // The thumb sits in the list's right padding, over no row.
            .child(div().flex_1().min_h_0().relative().flex().flex_col().child(list).child(self.home.search_scrollbar.render(&self.search_scroll)))
            .child(footer);

        Some(
            deferred(
                div()
                    .id("search-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| w.close_search(window, cx)))
                    .child(div().flex().items_start().gap(px(12.)).child(modal).children(peek)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippet_context() {
        let (s, r) = snippet("first line\nsome long prefix text before the Needle here\nnext", "needle").unwrap();
        assert_eq!(&s[r[0].clone()], "Needle");
        assert!(s.starts_with('…'));
        assert!(s.ends_with("\u{a0}\u{a0}\u{a0}next"));
        let (s, r) = snippet("中文内容测试内容", "内容").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(&s[r[1].clone()], "内容");
        assert!(snippet("abc", "x").is_none());
    }
}
