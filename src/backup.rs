//! Library backup (#4 for 0.7.1): the whole library as one zip.
//!
//! "Back up library…" sits in the header's Import menu next to
//! "Restore backup…". The zip work lives in fdl_core::backup; this
//! file is only the dialogs and the toasts. Restore never
//! overwrites: it brings back files that are missing and leaves
//! everything current untouched, so it needs no confirmation.

use crate::app::Workspace;
use crate::ui::ToastKind;
use gpui::*;
use std::path::{Path, PathBuf};

fn downloads() -> PathBuf {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_default()
}

impl Workspace {
    /// Save the whole library as one zip where the user chooses.
    pub(crate) fn open_backup_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import_menu = None;
        cx.notify();
        let name = format!("ai-md-backup-{}.zip", crate::catalog::today_str());
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&downloads(), Some(&name)));
        cx.spawn_in(window, async move |this, cx| {
            let path = match prompt {
                Some(p) => p.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(path) = path else { return };
            let _ = this.update_in(cx, |w, _, cx| w.write_library_backup(&path, cx));
        })
        .detach();
    }

    fn write_library_backup(&mut self, path: &Path, cx: &mut Context<Self>) {
        let root = fdl_core::store::Store::root_dir();
        match fdl_core::backup::write_backup(&root, path) {
            Ok(n) => self.show_toast(&crate::localized_format!("已把 {n} 个文件备份到所选的压缩文件", "Backed up {n} files to the chosen zip file"), cx),
            Err(e) => self.show_toast_kind(ToastKind::Error, &crate::localized_format!("备份失败：{}", "The backup failed: {}", crate::settings::diagnostic(&e.to_string())), cx),
        }
        cx.notify();
    }

    /// Bring back the missing files from a backup zip the user picks.
    pub(crate) fn open_restore_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import_menu = None;
        cx.notify();
        let picked = crate::devtools::take_pick();
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(crate::settings::tr("选择备份文件", "Choose a backup file").into()),
            extensions: Some(vec![SharedString::from("zip")]),
        };
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(options));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(p) => p.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(path) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, _, cx| w.restore_library_backup(&path, cx));
        })
        .detach();
    }

    fn restore_library_backup(&mut self, path: &Path, cx: &mut Context<Self>) {
        let fail = |w: &mut Workspace, msg: String, cx: &mut Context<Self>| w.show_toast_kind(ToastKind::Error, &msg, cx);
        let Ok(meta) = std::fs::metadata(path) else {
            return fail(self, crate::settings::tr("读不到这个备份文件", "That backup file could not be read").to_string(), cx);
        };
        if meta.len() > 512 * 1024 * 1024 {
            return fail(self, crate::settings::tr("这个备份文件太大了", "That backup file is too large").to_string(), cx);
        }
        let Ok(bytes) = std::fs::read(path) else {
            return fail(self, crate::settings::tr("读不到这个备份文件", "That backup file could not be read").to_string(), cx);
        };
        let names = match fdl_core::backup::entry_names(&bytes) {
            Ok(n) => n,
            Err(_) => return fail(self, crate::settings::tr("这不是一个有效的备份文件", "That is not a valid backup file").to_string(), cx),
        };
        if !fdl_core::backup::looks_like_library(&names) {
            return fail(self, crate::settings::tr("这不是 ai.md 的文档库备份", "That zip is not an ai.md library backup").to_string(), cx);
        }
        let root = fdl_core::store::Store::root_dir();
        match fdl_core::backup::extract_backup(&bytes, &root) {
            Ok((written, skipped)) => {
                self.entries = self.store.list();
                self.sort();
                self.show_toast(&crate::localized_format!("已从备份恢复 {written} 个文件；{skipped} 个现有文件保持不变", "Restored {written} files from the backup; {skipped} existing files were left as they are"), cx);
            }
            Err(e) => self.show_toast_kind(ToastKind::Error, &crate::localized_format!("恢复失败：{}", "The restore failed: {}", crate::settings::diagnostic(&e.to_string())), cx),
        }
        cx.notify();
    }
}
