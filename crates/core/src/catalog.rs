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
    Ok(Catalog { entries })
}

impl Catalog {
    /// Entries matching a plain-text query over name, summary and
    /// source, and, when `kind` is given, exactly that kind. An empty
    /// query matches everything; the query is capped for safety. File
    /// order is preserved.
    pub fn search<'a>(&'a self, query: &str, kind: Option<&str>) -> Vec<&'a CatalogEntry> {
        let needle: String = query.trim().chars().take(MAX_QUERY_CHARS).collect::<String>().to_lowercase();
        self.entries
            .iter()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| {
                needle.is_empty()
                    || e.name.to_lowercase().contains(&needle)
                    || e.summary.to_lowercase().contains(&needle)
                    || e.source_name.to_lowercase().contains(&needle)
            })
            .collect()
    }
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

/// Builds the Markdown note saved when an entry is added to the
/// library: the facts, the exact install/config text, the prompt body
/// for prompt entries, and room for personal notes.
pub fn fork_markdown(e: &CatalogEntry) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", e.name));
    out.push_str(&format!("{}\n\n", e.summary));
    out.push_str(&format!("- Type: {}\n", e.kind));
    out.push_str(&format!("- Source: {} — {}\n", e.source_name, e.source_url));
    out.push_str(&format!("- License: {}\n", e.license));
    out.push_str(&format!("- Works with: {}\n", e.clients.join(", ")));
    let risks = if e.risks.is_empty() {
        "Nothing".to_string()
    } else {
        e.risks.iter().map(|r| risk_label(r)).collect::<Vec<_>>().join(", ")
    };
    out.push_str(&format!("- Can touch: {risks}\n"));
    out.push_str(&format!("- Catalog verified: {}\n", e.verified));
    if let Some(install) = &e.install {
        out.push_str(&format!("\n## Install\n\n```\n{install}\n```\n"));
    }
    if let Some(config) = &e.config {
        out.push_str(&format!("\n## Config\n\n```json\n{config}\n```\n"));
    }
    if let Some(body) = &e.body {
        out.push_str(&format!("\n## Prompt\n\n{body}\n"));
    }
    out.push_str("\n## My notes\n\nVerify the entry at its source before installing. Servers and skills run with real permissions.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB: &str = r#"{
        "id": "mcp-github",
        "name": "GitHub MCP Server",
        "kind": "mcp-server",
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
        assert_eq!(catalog.search("", Some("mcp-server"))[0].id, "mcp-github");
        assert!(catalog.search("github", Some("prompt")).is_empty());
        assert!(catalog.search("nonexistent", None).is_empty());
    }

    #[test]
    fn fork_text_has_the_sections() {
        let catalog = sample();
        let note = fork_markdown(&catalog.entries[0]);
        assert!(note.starts_with("# GitHub MCP Server\n"));
        assert!(note.contains("- Type: mcp-server"));
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
}
