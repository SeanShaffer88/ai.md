//! The bundled catalog: AI prompts, MCP servers, skills and guides
//! worth starting from. Entries ship as one JSON file per entry in the
//! repo's catalog folder and are read-only in the app; saving one to the
//! library copies it into an editable note (see [`fork_markdown`]).

use serde::Deserialize;

/// How much of a search query is used, in characters.
const MAX_QUERY_CHARS: usize = 200;

/// One catalog entry, as stored in its JSON file. Every field defaults,
/// so a partial entry still loads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    /// The kind of thing this points at: "mcp-server", "skill",
    /// "prompt", "prompt-pack", "guide", ...
    pub kind: String,
    pub summary: String,
    pub source_name: String,
    pub source_url: String,
    pub license: String,
    /// Clients the entry works with, e.g. "Claude", "Cursor".
    pub clients: Vec<String>,
    /// What the entry can touch once installed, as tags: "files",
    /// "commands", "network", "api-key", "none", "read-only".
    pub risks: Vec<String>,
    /// A recommended first pick in its kind.
    pub starter: bool,
    /// Date the entry was last checked at its source, "2026-11-10".
    pub verified: String,
    pub install: Option<String>,
    pub config: Option<String>,
    /// The full prompt text, for prompt entries.
    pub body: Option<String>,
}

/// The whole catalog, in file order.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub entries: Vec<CatalogEntry>,
    /// Lowercased search text per entry (name, summary, source,
    /// clients, license), built once at load so a keystroke does not
    /// re-lowercase every field of every entry.
    haystacks: Vec<String>,
}

fn haystack(e: &CatalogEntry) -> String {
    let mut h = String::new();
    for part in [&e.name, &e.summary, &e.source_name, &e.license] {
        h.push_str(&part.to_lowercase());
        h.push(' ');
    }
    for c in &e.clients {
        h.push_str(&c.to_lowercase());
        h.push(' ');
    }
    h
}

/// Parses one JSON string per entry. On failure the error names the
/// index of the entry that did not parse.
pub fn load_catalog(json_strs: &[&str]) -> Result<Catalog, String> {
    let mut entries = Vec::with_capacity(json_strs.len());
    for (index, json) in json_strs.iter().enumerate() {
        let entry: CatalogEntry =
            serde_json::from_str(json).map_err(|e| format!("catalog entry {index}: {e}"))?;
        entries.push(entry);
    }
    let haystacks = entries.iter().map(haystack).collect();
    Ok(Catalog { entries, haystacks })
}

impl Catalog {
    /// Entries matching a plain-text query, and, when `kind` is given,
    /// exactly that kind. The query is split into words and every word
    /// must appear somewhere in the entry's name, summary, source,
    /// clients or license — so "github mcp" finds the GitHub server.
    /// An empty query matches everything; the query is capped for
    /// safety. File order is preserved.
    pub fn search<'a>(&'a self, query: &str, kind: Option<&str>) -> Vec<&'a CatalogEntry> {
        let capped: String = query.trim().chars().take(MAX_QUERY_CHARS).collect();
        let tokens: Vec<String> = capped.split_whitespace().map(|t| t.to_lowercase()).collect();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| kind.is_none_or(|k| e.kind == k))
            .filter(|(i, _)| tokens.iter().all(|t| self.haystacks[*i].contains(t.as_str())))
            .map(|(_, e)| e)
            .collect()
    }

    /// The browse page's full filter set: the text/kind search above,
    /// plus an optional "can touch" risk (an entry whose risk list is
    /// empty counts as "none") and a starters-only switch.
    pub fn browse(&self, query: &str, kind: Option<&str>, risk: Option<&str>, starters_only: bool) -> Vec<CatalogEntry> {
        self.search(query, kind)
            .into_iter()
            .filter(|e| !starters_only || e.starter)
            .filter(|e| match risk {
                None => true,
                Some(r) => {
                    if e.risks.is_empty() { r == "none" } else { e.risks.iter().any(|x| x == r) }
                }
            })
            .cloned()
            .collect()
    }

    /// Merges the user's own entries (a JSON array of entry objects,
    /// from `catalog-custom.json` in the library folder) over the
    /// bundled ones: a custom entry with a bundled entry's id replaces
    /// it, a new id is appended. Returns how many were merged.
    pub fn merge_custom(&mut self, json: &str) -> Result<usize, String> {
        let custom: Vec<CatalogEntry> = serde_json::from_str(json).map_err(|e| format!("custom catalog: {e}"))?;
        let n = custom.len();
        for entry in custom {
            match self.entries.iter().position(|e| e.id == entry.id) {
                Some(pos) => {
                    self.haystacks[pos] = haystack(&entry);
                    self.entries[pos] = entry;
                }
                None => {
                    self.haystacks.push(haystack(&entry));
                    self.entries.push(entry);
                }
            }
        }
        Ok(n)
    }
}

/// Orders browse results by a sort key ("name", "name-desc", "kind",
/// "starter", "newest"; anything else is name order) and then floats
/// the user's favorites to the top, keeping their relative order.
/// Kind order follows the app's tab order.
pub fn arrange(entries: &mut [CatalogEntry], sort_key: &str, favorites: &[String]) {
    const KIND_ORDER: [&str; 5] = ["mcp", "skill", "prompt", "prompt-pack", "guide"];
    let kind_rank = |k: &str| KIND_ORDER.iter().position(|id| *id == k).unwrap_or(KIND_ORDER.len());
    match sort_key {
        "name-desc" => entries.sort_by_key(|e| std::cmp::Reverse(e.name.to_lowercase())),
        "kind" => entries.sort_by_key(|e| (kind_rank(&e.kind), e.name.to_lowercase())),
        "starter" => entries.sort_by_key(|e| (!e.starter, e.name.to_lowercase())),
        "newest" => entries.sort_by_key(|e| (std::cmp::Reverse(e.verified.clone()), e.name.to_lowercase())),
        _ => entries.sort_by_key(|e| e.name.to_lowercase()),
    }
    if !favorites.is_empty() {
        entries.sort_by_key(|e| !favorites.contains(&e.id));
    }
}

/// Days from a "YYYY-MM-DD" date to `today_days` (days since the Unix
/// epoch), or None when the date does not parse. Lets the detail page
/// warn when an entry has not been re-checked in a long time.
pub fn verified_age_days(verified: &str, today_days: i64) -> Option<i64> {
    let mut parts = verified.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) { return None; }
    // Days from civil date (Howard Hinnant's algorithm).
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(today_days - days)
}

/// Data checks over a loaded catalog: duplicate or empty ids and
/// names, kinds and risk tags outside the known vocabularies, a
/// `verified` date that is not YYYY-MM-DD, and prompt entries with no
/// prompt text. Returns one plain-language problem per failure.
pub fn validate_catalog(catalog: &Catalog) -> Vec<String> {
    const KINDS: [&str; 5] = ["mcp", "skill", "prompt", "prompt-pack", "guide"];
    const RISKS: [&str; 6] = ["none", "files", "commands", "network", "api-key", "read-only"];
    let mut problems = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    for e in &catalog.entries {
        if e.id.is_empty() { problems.push(format!("an entry named {:?} has no id", e.name)); }
        if ids.contains(&e.id.as_str()) { problems.push(format!("duplicate entry id {:?}", e.id)); }
        ids.push(&e.id);
        if e.name.is_empty() { problems.push(format!("entry {:?} has no name", e.id)); }
        if !KINDS.contains(&e.kind.as_str()) { problems.push(format!("entry {:?} has unknown kind {:?}", e.id, e.kind)); }
        for r in &e.risks {
            if !RISKS.contains(&r.as_str()) { problems.push(format!("entry {:?} has unknown risk tag {:?}", e.id, r)); }
        }
        if verified_age_days(&e.verified, 0).is_none() {
            problems.push(format!("entry {:?} has a bad verified date {:?}", e.id, e.verified));
        }
        if e.kind == "prompt" && e.body.as_deref().is_none_or(|b| b.trim().is_empty()) {
            problems.push(format!("prompt entry {:?} has no prompt text", e.id));
        }
    }
    problems
}

/// Plain words for a risk tag, shown before anything is installed.
fn risk_label(risk: &str) -> &str {
    match risk {
        "files" => "Files",
        "commands" => "Commands",
        "network" => "Network",
        "api-key" => "Needs an API key",
        "none" => "Nothing",
        "read-only" => "Read only",
        other => other,
    }
}

/// The words a saved catalog note is written with; the app passes
/// its own language, the defaults are English.
#[derive(Debug, Clone)]
pub struct ForkLabels {
    pub type_: String,
    pub source: String,
    pub license: String,
    pub works_with: String,
    pub can_touch: String,
    pub verified: String,
    pub install: String,
    pub config: String,
    pub prompt: String,
    pub my_notes: String,
    pub notes_tip: String,
}

impl Default for ForkLabels {
    fn default() -> Self {
        Self {
            type_: "Type".into(),
            source: "Source".into(),
            license: "License".into(),
            works_with: "Works with".into(),
            can_touch: "Can touch".into(),
            verified: "Catalog verified".into(),
            install: "Install".into(),
            config: "Config".into(),
            prompt: "Prompt".into(),
            my_notes: "My notes".into(),
            notes_tip: "Verify the entry at its source before installing. Servers and skills run with real permissions.".into(),
        }
    }
}

/// Builds the Markdown note saved when an entry is added to the
/// library: the facts, the exact install/config text, the prompt body
/// for prompt entries, and room for personal notes.
pub fn fork_markdown(e: &CatalogEntry) -> String {
    fork_markdown_labeled(e, &ForkLabels::default())
}

/// [`fork_markdown`] with caller-provided labels.
pub fn fork_markdown_labeled(e: &CatalogEntry, labels: &ForkLabels) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", e.name));
    out.push_str(&format!("{}\n\n", e.summary));
    out.push_str(&format!("- {}: {}\n", labels.type_, e.kind));
    out.push_str(&format!("- {}: {} — {}\n", labels.source, e.source_name, e.source_url));
    out.push_str(&format!("- {}: {}\n", labels.license, e.license));
    out.push_str(&format!("- {}: {}\n", labels.works_with, e.clients.join(", ")));
    let risks = if e.risks.is_empty() {
        "Nothing".to_string()
    } else {
        e.risks.iter().map(|r| risk_label(r)).collect::<Vec<_>>().join(", ")
    };
    out.push_str(&format!("- {}: {risks}\n", labels.can_touch));
    out.push_str(&format!("- {}: {}\n", labels.verified, e.verified));
    if let Some(install) = &e.install {
        out.push_str(&format!("\n## {}\n\n```\n{install}\n```\n", labels.install));
    }
    if let Some(config) = &e.config {
        out.push_str(&format!("\n## {}\n\n```json\n{config}\n```\n", labels.config));
    }
    if let Some(body) = &e.body {
        out.push_str(&format!("\n## {}\n\n{body}\n", labels.prompt));
    }
    out.push_str(&format!("\n## {}\n\n{}\n", labels.my_notes, labels.notes_tip));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB: &str = r#"{
        "id": "mcp-github",
        "name": "GitHub MCP Server",
        "kind": "mcp",
        "summary": "Repositories, issues and pull requests from your AI client.",
        "source_name": "GitHub",
        "source_url": "https://github.com/github/github-mcp-server",
        "license": "MIT",
        "clients": ["Claude", "Cursor"],
        "risks": ["network", "api-key"],
        "starter": true,
        "verified": "2026-11-10",
        "install": "npx -y @modelcontextprotocol/server-github",
        "config": "{\n  \"mcpServers\": {}\n}"
    }"#;

    const REVIEW: &str = r#"{
        "id": "prompt-code-review",
        "name": "Code Review Pro",
        "kind": "prompt",
        "summary": "A senior-review prompt.",
        "verified": "2026-11-10",
        "body": "You are a senior reviewer. Review {{code}}."
    }"#;

    fn sample() -> Catalog {
        load_catalog(&[GITHUB, REVIEW]).unwrap()
    }

    #[test]
    fn loads_full_and_partial_entries() {
        let catalog = sample();
        assert_eq!(catalog.entries.len(), 2);
        let gh = &catalog.entries[0];
        assert_eq!(gh.name, "GitHub MCP Server");
        assert!(gh.starter);
        assert_eq!(gh.clients, vec!["Claude".to_string(), "Cursor".to_string()]);
        let prompt = &catalog.entries[1];
        assert_eq!(prompt.kind, "prompt");
        assert!(!prompt.starter);
        assert!(prompt.install.is_none());
        assert_eq!(prompt.license, "");
    }

    #[test]
    fn load_error_names_the_index() {
        let err = load_catalog(&[GITHUB, "{not json"]).unwrap_err();
        assert!(err.contains("catalog entry 1"), "unexpected error: {err}");
    }

    #[test]
    fn search_by_text_and_kind() {
        let catalog = sample();
        assert_eq!(catalog.search("", None).len(), 2);
        assert_eq!(catalog.search("github", None).len(), 1);
        assert_eq!(catalog.search("GITHUB", None)[0].id, "mcp-github");
        assert_eq!(catalog.search("review", None)[0].id, "prompt-code-review");
        assert_eq!(catalog.search("", Some("prompt")).len(), 1);
        assert_eq!(catalog.search("", Some("mcp"))[0].id, "mcp-github");
        assert!(catalog.search("github", Some("prompt")).is_empty());
        assert!(catalog.search("nonexistent", None).is_empty());
    }

    #[test]
    fn fork_text_has_the_sections() {
        let catalog = sample();
        let note = fork_markdown(&catalog.entries[0]);
        assert!(note.starts_with("# GitHub MCP Server\n"));
        assert!(note.contains("- Type: mcp"));
        assert!(note.contains("- Source: GitHub — https://github.com/github/github-mcp-server"));
        assert!(note.contains("- Can touch: Network, Needs an API key"));
        assert!(note.contains("- Catalog verified: 2026-11-10"));
        assert!(note.contains("## Install\n\n```\nnpx -y @modelcontextprotocol/server-github\n```"));
        assert!(note.contains("## Config\n\n```json"));
        assert!(note.contains("## My notes"));
        let note = fork_markdown(&catalog.entries[1]);
        assert!(note.contains("## Prompt\n\nYou are a senior reviewer. Review {{code}}."));
        assert!(!note.contains("## Install"));
    }

    #[test]
    fn search_matches_every_word_across_fields() {
        let catalog = sample();
        // Words can come from different fields (name + kind source).
        assert_eq!(catalog.search("github repositories", None).len(), 1);
        // "claude" only appears in the clients list.
        assert_eq!(catalog.search("claude", None)[0].id, "mcp-github");
        // A word that appears nowhere excludes the entry.
        assert!(catalog.search("github zzz", None).is_empty());
    }

    #[test]
    fn browse_filters_risk_and_starters() {
        let catalog = sample();
        assert_eq!(catalog.browse("", None, Some("network"), false).len(), 1);
        assert_eq!(catalog.browse("", None, Some("none"), false).len(), 1);
        assert!(catalog.browse("", None, Some("files"), false).is_empty());
        assert_eq!(catalog.browse("", None, None, true).len(), 1);
    }

    #[test]
    fn arrange_orders_and_floats_favorites() {
        let catalog = sample();
        let mut v = catalog.entries.clone();
        arrange(&mut v, "name-desc", &[]);
        assert_eq!(v[0].id, "mcp-github");
        arrange(&mut v, "name", &["prompt-code-review".to_string()]);
        assert_eq!(v[0].id, "prompt-code-review");
        let mut v = catalog.entries.clone();
        arrange(&mut v, "name", &["mcp-github".to_string()]);
        assert_eq!(v[0].id, "mcp-github");
    }

    #[test]
    fn merge_custom_replaces_and_appends() {
        let mut catalog = sample();
        let n = catalog.merge_custom(r#"[
            {"id": "mcp-github", "name": "My GitHub", "kind": "mcp"},
            {"id": "mine", "name": "Mine", "kind": "guide"}
        ]"#).unwrap();
        assert_eq!(n, 2);
        assert_eq!(catalog.entries.len(), 3);
        assert_eq!(catalog.entries[0].name, "My GitHub");
        assert_eq!(catalog.search("mine", None).len(), 1);
        assert!(catalog.merge_custom("{nope").is_err());
    }

    #[test]
    fn verified_age_days_math() {
        // 2026-11-10 is 20767 days after the epoch.
        assert_eq!(verified_age_days("2026-11-10", 20767), Some(0));
        assert_eq!(verified_age_days("2026-11-09", 20767), Some(1));
        assert_eq!(verified_age_days("2025-11-10", 20767), Some(365));
        assert_eq!(verified_age_days("10/11/2026", 20767), None);
        assert_eq!(verified_age_days("", 20767), None);
        assert_eq!(verified_age_days("2026-13-01", 20767), None);
    }

    #[test]
    fn validate_flags_bad_data() {
        let catalog = sample();
        assert!(validate_catalog(&catalog).is_empty());
        let mut bad = sample();
        bad.entries.push(CatalogEntry { id: "mcp-github".into(), name: "Dup".into(), kind: "weird".into(), verified: "soon".into(), ..Default::default() });
        bad.entries.push(CatalogEntry { id: "p1".into(), name: "P".into(), kind: "prompt".into(), verified: "2026-11-10".into(), ..Default::default() });
        let problems = validate_catalog(&bad);
        assert!(problems.iter().any(|p| p.contains("duplicate")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("unknown kind")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("bad verified date")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("no prompt text")), "{problems:?}");
    }

    #[test]
    fn fork_labels_localize_the_note() {
        let catalog = sample();
        let labels = ForkLabels { type_: "类型".into(), my_notes: "我的笔记".into(), ..Default::default() };
        let note = fork_markdown_labeled(&catalog.entries[0], &labels);
        assert!(note.contains("- 类型: mcp"));
        assert!(note.contains("## 我的笔记"));
    }
}
