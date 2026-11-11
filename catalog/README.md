# Catalog

The catalog is a set of curated entries: MCP servers, skills, complete prompt
templates, prompt packs, and guides. Each entry is one JSON file in this
folder, named `<id>.json`, where the id is kebab-case. The catalog ships with
the app and is read-only inside it; saving an entry to the library copies it
into an editable note.

## Entry format

```json
{
  "id": "mcp-filesystem",
  "name": "Filesystem (official reference server)",
  "kind": "mcp",
  "summary": "What the entry is and why it is worth using, in one or two sentences.",
  "source_name": "Model Context Protocol",
  "source_url": "https://github.com/modelcontextprotocol/servers",
  "license": "MIT",
  "clients": ["Claude", "Cursor", "VS Code"],
  "risks": ["files"],
  "starter": true,
  "verified": "2026-11-10",
  "install": "npx -y @modelcontextprotocol/server-filesystem",
  "config": "{\n  \"mcpServers\": { }\n}"
}
```

| Field | Meaning |
| --- | --- |
| `id` | Unique kebab-case identifier. Must match the file name. |
| `name` | Display name. |
| `kind` | One of `mcp`, `skill`, `prompt`, `prompt-pack`, `guide`. |
| `summary` | One or two plain sentences describing the entry. |
| `source_name` | Who publishes it (project, company, or community). |
| `source_url` | Canonical link to the source. |
| `license` | License of the source, or `See site` / `See repository` when it is not a code license. |
| `clients` | Tools the entry works with, as a list of names. |
| `risks` | What the entry can touch once installed (see below). |
| `starter` | `true` for recommended first picks within a kind. |
| `verified` | Date the entry's facts were last checked, `YYYY-MM-DD`. |
| `install` | Optional. The exact install command, when there is one. |
| `config` | Optional. A ready-to-paste configuration snippet, when there is one. |
| `body` | Prompt entries only. The full prompt template text. |

## Risk values

Each value states what an entry can reach once installed:

- `files` — reads or writes local files
- `commands` — runs commands on the machine
- `network` — makes network requests
- `api-key` — needs an API key or token
- `none` — touches nothing on the machine
- `read-only` — reads content without changing anything

## Prompt templates

Entries of kind `prompt` carry the complete template in `body`. Templates use
two placeholder forms:

- `{{name}}` — a required blank the reader fills in
- `{{name = some default}}` — a blank with a default value

## Adding an entry

1. Create `<id>.json` in this folder following the format above.
2. Check the facts at the source: name, license, install command, and what it
   can touch. Set `verified` to the date of the check.
3. Keep the summary factual and short. Mark `starter` only for entries a new
   user should try first.
4. Confirm the file parses as JSON before submitting it.

## Your own entries

Put a `catalog-custom.json` file in the app's documents folder (the
folder shown under Settings → Documents) to add your own entries or
override bundled ones. The file is a JSON array of entry objects in
the same shape as the files here; an entry whose `id` matches a
bundled entry replaces it, and a new `id` is added. Changes apply the
next time the app starts.
