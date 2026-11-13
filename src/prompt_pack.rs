//! Prompt packs (#12): your prompts as plain files you own.
//! Export writes every favorited catalog prompt and every library
//! document that contains {{variables}} as a Markdown file, plus a
//! pack.json carrying their Run profiles. Import reads a folder of
//! Markdown files back in as documents and merges any pack.json
//! profiles. Nothing here needs the network or an account.

use crate::app::Workspace;
use crate::ui::ToastKind;
use gpui::*;
use std::collections::BTreeMap;
use std::path::Path;

/// The pack.json manifest: profiles keyed by prompt title, then by
/// profile name, then (variable, value) pairs — the same shape the
/// settings store uses, so merging is a plain insert.
#[derive(serde::Serialize, serde::Deserialize)]
struct PackManifest {
    pack: String,
    version: u32,
    #[serde(default)]
    profiles: BTreeMap<String, BTreeMap<String, Vec<(String, String)>>>,
}

/// A file name a prompt can travel under: portable on every OS.
fn pack_file_name(title: &str, used: &mut Vec<String>) -> String {
    let mut base: String = title
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '-' } else { c })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    if base.is_empty() { base = "prompt".to_string(); }
    base.truncate(80);
    let mut name = format!("{base}.md");
    let mut n = 2;
    while used.contains(&name) {
        name = format!("{base}-{n}.md");
        n += 1;
    }
    used.push(name.clone());
    name
}

impl Workspace {
    /// Export a prompt pack into a folder the user picks.
    pub(crate) fn open_export_pack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let options = PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::settings::tr("选择导出提示词包的文件夹", "Choose a folder for the prompt pack").into()),
            extensions: None,
        };
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(options));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(p) => p.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(dir) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, _, cx| w.write_prompt_pack(&dir, cx));
        })
        .detach();
    }

    fn write_prompt_pack(&mut self, dir: &Path, cx: &mut Context<Self>) {
        // (title, text) pairs: favorited catalog prompts first, then
        // the user's own documents that contain variables.
        let mut items: Vec<(String, String)> = Vec::new();
        for id in &self.catalog.favorites {
            if let Some(e) = self.catalog.entry(id)
                && crate::search::is_prompt_kind(&e.kind)
                && let Some(body) = &e.body
            {
                items.push((e.name.clone(), body.clone()));
            }
        }
        for e in &self.entries {
            if e.meta.folder.is_some() || e.meta.local() { continue; }
            if !fdl_core::prompt::parse_template(&e.body).variables.is_empty() {
                items.push((e.title.clone(), e.body.to_string()));
            }
        }
        if items.is_empty() {
            self.show_toast(crate::settings::tr("还没有可导出的提示词 — 先收藏目录里的提示词，或写一篇带 {{变量}} 的文档", "Nothing to export yet — favorite a catalog prompt, or write a document with {{variables}}"), cx);
            return;
        }
        let settings = self.store.settings();
        let mut used: Vec<String> = vec!["pack.json".to_string()];
        let mut profiles: BTreeMap<String, BTreeMap<String, Vec<(String, String)>>> = BTreeMap::new();
        let mut failed = 0usize;
        for (title, text) in &items {
            let name = pack_file_name(title, &mut used);
            if std::fs::write(dir.join(&name), text).is_err() { failed += 1; }
            if let Some(by_name) = settings.run_profiles.get(title) {
                profiles.insert(title.clone(), by_name.clone());
            }
        }
        let manifest = PackManifest { pack: "ai.md prompt pack".to_string(), version: 1, profiles };
        if let Ok(text) = serde_json::to_string_pretty(&manifest)
            && std::fs::write(dir.join("pack.json"), text).is_err() { failed += 1; }
        let n = items.len();
        if failed > 0 {
            self.show_toast_kind(ToastKind::Error, &crate::localized_format!("导出了 {n} 篇提示词，{failed} 个文件写入失败", "Exported {n} prompts; {failed} files could not be written"), cx);
        } else {
            self.show_toast(&crate::localized_format!("已导出 {n} 篇提示词到所选文件夹", "Exported {n} prompts to the chosen folder"), cx);
        }
        cx.notify();
    }

    /// Import a prompt pack: a folder picked by the user; its Markdown
    /// files become documents (the folder import), and a pack.json in
    /// it merges its Run profiles into the settings.
    pub(crate) fn open_import_pack(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.import_menu = None;
        cx.notify();
        let picked = crate::devtools::take_pick();
        let options = PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(crate::settings::tr("导入提示词包", "Import prompt pack").into()),
            extensions: None,
        };
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(options));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt {
                Some(p) => p.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(dir) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update_in(cx, |w, window, cx| w.import_pack(&dir, window, cx));
        })
        .detach();
    }

    fn import_pack(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let manifest = std::fs::read_to_string(dir.join("pack.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<PackManifest>(&text).ok());
        self.import_folder(dir, None, false, window, cx);
        if let Some(manifest) = manifest {
            let mut merged = 0usize;
            let mut s = self.store.settings();
            for (title, by_name) in manifest.profiles {
                for (name, values) in by_name {
                    s.run_profiles.entry(title.clone()).or_default().insert(name, values);
                    merged += 1;
                }
            }
            if merged > 0 {
                if let Err(e) = self.store.save_settings(&s) { crate::logging::write(&format!("prompt pack profiles: {e}")); }
                self.show_toast(&crate::localized_format!("已导入 {merged} 个运行方案", "Imported {merged} run profiles"), cx);
            }
        }
        cx.notify();
    }
}
