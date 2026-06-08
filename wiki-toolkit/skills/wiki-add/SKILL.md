---
name: wiki-add
description: >
  Write a NEW finished page into the dev_wiki knowledge base via the HTTP write channel
  (POST /api/v1/projects/{id}/sources). Builds schema-compliant frontmatter
  (type/title/summary/bc/sdlc_phases/...), routes the file to the right wiki/ subdir by type,
  and posts it so the page is keyword-searchable immediately and embedded by the canonical
  pipeline. Use when you have a concrete piece of knowledge (a concept, decision, solution,
  playbook, entity) worth persisting — NOT for session wrap-up (use /wiki-distill for that).
  Triggers on: "/wiki-add", "wiki-add", "add this to the wiki", "save this as a wiki page",
  "记到知识库", "写一页 wiki", "persist this knowledge".
allowed-tools: Read, Grep, Glob, Bash
---

# wiki-add: write a finished page into dev_wiki

Author one schema-compliant page and POST it through the write channel. The endpoint lands
the file on disk and triggers the canonical `embedPage` pipeline (chunk → contextual-prefix →
embed). **Keyword retrieval is immediate; vector retrieval fills in once embedding completes
(needs the desktop app open).**

This skill WRITES. Confirm the target path + type + bc with the user before posting if any of
them is ambiguous.

---

## Inputs (read at runtime — never hardcode)

Operate on the current project/vault. Authoritative inputs:

- `schema.md` (vault root) — field + type→directory routing authority.
- `wiki/_meta/bc-registry.yaml` — legal `bc:` ids (`status: active`/`deprecated`).
- `wiki/_meta/scope-vocabulary.yaml` — legal `type:`, `sdlc_phases:`, and `keyword_to_bc` map.
- Page skeletons in `wiki-toolkit/templates/<type>.md` — copy the matching one.

If `bc-registry.yaml` is missing, STOP and report it — do not invent a `bc`.

## Type → directory routing

| type | dir | use |
|---|---|---|
| entity | `wiki/entities/` | named things (person/tool/org/protocol/dataset) |
| concept | `wiki/concepts/` | concepts, techniques, frameworks |
| source | `wiki/sources/` | papers, articles, books |
| comparison | `wiki/comparisons/` | side-by-side comparisons |
| query | `wiki/queries/` | open questions |
| synthesis | `wiki/synthesis/` | cross-page round-ups |
| solution | `wiki/solutions/` | cross-domain solution hubs |
| playbook | `wiki/playbooks/` | actionable step manuals |
| decision | `wiki/decisions/` | decision records / ADR |
| learning | `wiki/learnings/` | experience capture (source-only; prefer /wiki-distill) |
| bc-readme | `wiki/_meta/` | bounded-context overview |

`schema.md` is authoritative — if it disagrees with this table, follow `schema.md`.

---

## Procedure

1. **Pick `type`.** Choose from the routing table by what the content IS. When torn between
   solution (a path that ties things together) and concept/entity (a single thing), prefer the
   more specific leaf type.
2. **Pick `bc`.** Match the content against `keyword_to_bc`; the resulting id must exist in
   `bc-registry.yaml` with `status: active`. If nothing matches, use `general` OR ask the user
   whether to register a new context (do not silently coin a new bc).
3. **Write the `summary`** — 100-150 中文字 (hard floor 60, ceiling 180), self-contained,
   because it is the only string spliced directly into recall. **Never `summary: TODO` or
   blank.** It must make sense to a reader who has not opened the page.
4. **Fill frontmatter** from `wiki-toolkit/templates/<type>.md`: `type, title, summary, bc,
   sdlc_phases (list, may be []), tags, related, created, updated` (+ `sources` where the
   template has it). `created`/`updated` = today (`date +%F`).
5. **Slug + path.** kebab-case slug from the title; `path = wiki/<dir>/<slug>.md`.
6. **Write the body** under the frontmatter, following the template's section skeleton. Use
   `[[wikilinks]]` to existing pages where relevant (Glob `wiki/**/*.md` to confirm targets).
7. **POST it** (see below). Handle the response.

---

## Posting (the write channel)

```bash
BASE="http://127.0.0.1:19828/api/v1"
TOKEN="${LLM_WIKI_API_TOKEN:-}"          # set in shell, or enable allowUnauthenticated in Settings
AUTH=(); [ -n "$TOKEN" ] && AUTH=(-H "Authorization: Bearer $TOKEN")  # robust in bash AND zsh
REL="wiki/decisions/my-slug.md"
PAGE="$(mktemp)"; cat > "$PAGE" <<'MD'
---
type: decision
title: "决策：X vs Y"
summary: …(100-150 中文字)…
bc: payment
sdlc_phases: [design]
tags: […, 决策记录]
related: []
created: 2026-06-08
updated: 2026-06-08
sources: []
---

# 决策：X vs Y
…body…
MD

# Build the JSON body with jq so content is escaped correctly, then POST.
jq -n --arg path "$REL" --rawfile content "$PAGE" \
  '{path:$path, content:$content, overwrite:false}' \
| curl -sS -X POST "$BASE/projects/current/sources" \
    -H "Content-Type: application/json" "${AUTH[@]}" \
    -d @-
rm -f "$PAGE"
```

If `jq` is unavailable, build the body with `python3 -c 'import json,sys;
json.dump({"path":sys.argv[1],"content":open(sys.argv[2]).read(),"overwrite":False}, sys.stdout)'`.

**Response handling:**
- `{"ok":true,...,"embedQueued":true}` → done. Tell the user the page is keyword-searchable
  now; vector embedding runs in the app (needs it open).
- `409` "already exists" → a page is there. Show it (`GET .../files/content?path=`), then either
  pick a new slug or re-POST with `"overwrite":true` (only after the user agrees).
- `403` "path must be a .md under wiki/ or raw/sources/" → fix the path (no dotfiles/traversal,
  must be `.md`).
- `401` Unauthorized → the API needs a token. Tell the user to set `LLM_WIKI_API_TOKEN` (or
  enable "allow unauthenticated" / set a token in Settings → API Server).
- connection refused → the desktop app isn't running, or the API server is disabled in Settings.

---

## Invariants

- One page per call. Don't batch-write a stack of pages without showing the user the list first.
- `bc` MUST be a registered id — registering a new one is a `bc-registry.yaml` edit + a human
  decision, never an ad-hoc value smuggled into a page.
- `summary` is mandatory and semantic — never blank, never `TODO`, never auto-padded filler.
- Respect type↔directory routing; a mis-homed page breaks `/kb-lint` check #8 and discovery.
- Post through `POST /sources` — do NOT hand-write into `wiki/` and skip the endpoint (you'd
  lose the embed trigger).
- After writing, a quick `/kb-lint` on the new page is a cheap correctness check.
