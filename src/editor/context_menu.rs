//! Right-click context menu for the editor page (added by ai.md; the
//! upstream editor had none). Every item runs an existing editor
//! operation — the same handlers the keyboard shortcuts dispatch to —
//! so the menu adds no new editing behaviour of its own.

use super::keymap as k;
use super::view::Editor;
use crate::editor_ui::{MENU_INSET, MENU_PAD_Y, MENU_ROW_H, icon, menu};
use crate::feishu_theme::Theme;
use gpui::{prelude::FluentBuilder, *};

/// One menu entry's operation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CtxAct {
    Cut,
    Copy,
    Paste,
    PastePlain,
    SelectAll,
    Bold,
    Italic,
    Underline,
    Strike,
    InlineCode,
    Link,
    Undo,
    Redo,
}

impl Editor {
    /// Right press on the page: focus the editor, land the caret where
    /// the press fell (unless it fell inside the current selection,
    /// which is kept), and open the menu at the pointer.
    pub(super) fn on_context_press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.menu = None;
        self.menu_hovered = false;
        self.slash = None;
        self.hide_link_card(cx);
        self.toolbar_hidden = true;
        if let Some(pos) = self.pos_at(ev.position) {
            let sel = self.core.sel;
            let inside = !sel.is_collapsed()
                && sel.anchor.block == pos.block
                && sel.head.block == pos.block
                && pos.offset >= sel.anchor.offset.min(sel.head.offset)
                && pos.offset <= sel.anchor.offset.max(sel.head.offset);
            if !inside {
                self.set_caret(pos, false);
            }
        }
        self.ctx_menu = Some(ev.position);
        cx.notify();
    }

    fn run_ctx(&mut self, act: CtxAct, window: &mut Window, cx: &mut Context<Self>) {
        match act {
            CtxAct::Cut => self.on_cut(&k::Cut, window, cx),
            CtxAct::Copy => self.on_copy(&k::Copy, window, cx),
            CtxAct::Paste => self.on_paste(&k::Paste, window, cx),
            CtxAct::PastePlain => self.on_paste_plain(&k::PastePlain, window, cx),
            CtxAct::SelectAll => self.on_select_all(&k::SelectAll, window, cx),
            CtxAct::Bold => self.on_bold(&k::Bold, window, cx),
            CtxAct::Italic => self.on_italic(&k::Italic, window, cx),
            CtxAct::Underline => self.on_underline(&k::Underline, window, cx),
            CtxAct::Strike => self.on_strike(&k::Strike, window, cx),
            CtxAct::InlineCode => self.on_inline_code(&k::InlineCode, window, cx),
            CtxAct::Link => self.on_link(&k::Link, window, cx),
            CtxAct::Undo => self.on_undo(&k::Undo, window, cx),
            CtxAct::Redo => self.on_redo(&k::Redo, window, cx),
        }
    }

    /// The menu itself, anchored at the press point. Reading mode gets
    /// the two operations that work there (Copy, Select all); editing
    /// mode gets the clipboard, format and history groups.
    pub(super) fn render_ctx_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pos = self.ctx_menu?;
        let item = |id: &'static str, ic: &'static str, label: &'static str, hint: Option<&'static str>, act: CtxAct| {
            ctx_row(id, ic, label, hint).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e: &mut Editor, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    e.ctx_menu = None;
                    e.run_ctx(act, window, cx);
                }),
            )
        };
        let hint = |action: &str, keys: &'static str| Some(crate::shortcuts::hint(action, keys));
        let mut m = menu()
            .id("editor-ctx-menu")
            .occlude()
            .w(px(232.))
            .py(px(MENU_PAD_Y))
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|e: &mut Editor, _, _, cx| {
                e.ctx_menu = None;
                cx.notify();
            }));
        if !self.read_only {
            m = m.child(item("ctx-cut", "scissors", crate::settings::tr("剪切", "Cut"), hint("editor::Cut", "⌘ + X"), CtxAct::Cut));
        }
        m = m.child(item("ctx-copy", "copy", crate::settings::tr("复制", "Copy"), hint("editor::Copy", "⌘ + C"), CtxAct::Copy));
        if !self.read_only {
            m = m
                .child(item("ctx-paste", "clipboard-paste", crate::settings::tr("粘贴", "Paste"), hint("editor::Paste", "⌘ + V"), CtxAct::Paste))
                .child(item("ctx-paste-plain", "file-text", crate::settings::tr("粘贴为纯文本", "Paste as plain text"), hint("editor::PastePlain", "⌘ + Shift + V"), CtxAct::PastePlain));
        }
        m = m.child(item("ctx-select-all", "text-select", crate::settings::tr("全选", "Select all"), hint("editor::SelectAll", "⌘ + A"), CtxAct::SelectAll));
        if !self.read_only {
            m = m
                .child(separator_h())
                .child(item("ctx-bold", "bold", crate::settings::tr("粗体", "Bold"), hint("editor::Bold", "⌘ + B"), CtxAct::Bold))
                .child(item("ctx-italic", "italic", crate::settings::tr("斜体", "Italic"), hint("editor::Italic", "⌘ + I"), CtxAct::Italic))
                .child(item("ctx-underline", "underline", crate::settings::tr("下划线", "Underline"), hint("editor::Underline", "⌘ + U"), CtxAct::Underline))
                .child(item("ctx-strike", "strikethrough", crate::settings::tr("删除线", "Strikethrough"), hint("editor::Strike", "⌘ + Shift + X"), CtxAct::Strike))
                .child(item("ctx-code", "code", crate::settings::tr("代码", "Inline code"), hint("editor::InlineCode", "⌘ + Control + C"), CtxAct::InlineCode))
                .child(item("ctx-link", "link", crate::settings::tr("链接", "Link"), hint("editor::Link", "⌘ + K"), CtxAct::Link))
                .child(separator_h())
                .child(item("ctx-undo", "undo", crate::settings::tr("撤销", "Undo"), hint("editor::Undo", "⌘ + Z"), CtxAct::Undo))
                .child(item("ctx-redo", "redo", crate::settings::tr("重做", "Redo"), hint("editor::Redo", "⌘ + Shift + Z"), CtxAct::Redo));
        }
        Some(
            deferred(anchored().anchor(Corner::TopLeft).position(pos).snap_to_window_with_margin(px(8.)).child(m))
                .with_priority(2)
                .into_any_element(),
        )
    }
}

/// A menu row in the editor's own menu style (mirrors overlays' `row`).
fn ctx_row(id: &'static str, icon_name: &str, label: &'static str, hint: Option<&'static str>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(MENU_ROW_H))
        .mx(px(MENU_INSET))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(crate::ui::ROW_RADIUS))
        .text_size(px(13.))
        .line_height(px(20.))
        .text_color(Theme::text())
        .cursor_pointer()
        .hover(|s| s.bg(Theme::fill_hover()))
        .child(icon(icon_name, 16., Theme::icon()))
        .child(div().ml(px(8.)).flex_1().whitespace_nowrap().child(crate::settings::label(SharedString::from(label))))
        .when_some(hint, |d, r| d.child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(r.to_string())))
}

fn separator_h() -> Div {
    div().flex_none().mx(px(MENU_INSET + 8.)).my(px(4.)).h(px(1.)).bg(Theme::line_border())
}
