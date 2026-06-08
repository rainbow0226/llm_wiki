---
name: wiki-find
description: >
  Fast single-hop retrieval from the dev_wiki knowledge base. Runs the hybrid search
  (keyword + vector RRF) over the HTTP API, then splices each hit's pre-generated `summary`
  verbatim into the answer (summary 直拼) so you get usable knowledge in ~5s without opening
  pages. Optional bounded-context (`bc`) filter to scope to one knowledge domain. Use when you
  need to recall what the wiki already knows about a topic before doing new work; for multi-hop
  "how do these connect" exploration use /wiki-find-deep.
  Triggers on: "/wiki-find", "wiki-find", "search the wiki", "what do we know about",
  "查知识库", "召回", "recall from wiki", "find pages about".
allowed-tools: Read, Grep, Glob, Bash
---

# wiki-find: fast recall with summary 直拼

Search → read each hit's frontmatter `summary` → present the summaries **verbatim**. The
`summary` field is pre-generated for exactly this purpose (the only string spliced directly
into recall) — quote it, do NOT re-summarize or paraphrase it.

Target latency ~5s: one search call (`include_content:true` returns page bodies so no per-hit
follow-up reads needed) + light formatting.

---

## Procedure

1. **Query + optional `bc`.** Use the user's terms. If they named a domain, set `bc` (must be a
   registered id — `wiki/_meta/bc-registry.yaml`). ⚠️ Recall is weak on pure synonyms with no
   shared CJK characters — prefer the terms the pages actually use; if unsure, search broad
   (no bc) first.
2. **Search** (see call below): `top_k: 8`, `include_content: true`, `bc` if scoping.
3. **Extract per hit** from the returned `content` frontmatter: `summary`, `bc`, `type`, plus
   `path`, `title`, `score`, and `vectorScore`/`titleMatch` (why it matched).
4. **Present** a ranked list. For each:
   - `title` · `type` · `bc` · `path` · score (note keyword-vs-vector: `titleMatch:true` =
     strong lexical; `vectorScore` present = semantic hit).
   - the **`summary` verbatim** as the payload. If a hit has no summary (legacy page), fall back
     to the `snippet` and flag it as "无 summary（建议补）".
5. **If 0 results**: say so plainly, then offer — drop the `bc` filter, retry with terms drawn
   from the actual vault vocabulary, or escalate to `/wiki-find-deep` for graph expansion.

Do not invent pages. If the wiki has nothing, say it has nothing — that itself is a useful answer.

---

## The search call

```bash
BASE="http://127.0.0.1:19828/api/v1"
TOKEN="${LLM_WIKI_API_TOKEN:-}"
AUTH=(); [ -n "$TOKEN" ] && AUTH=(-H "Authorization: Bearer $TOKEN")  # robust in bash AND zsh
# To scope to a domain, add  --arg bc "payment"  and put  bc:$bc  in the object.
jq -n --arg q "支付失败重试机制" \
  '{query:$q, topK:8, includeContent:true}' \
| curl -sS -X POST "$BASE/projects/current/search" \
    -H "Content-Type: application/json" "${AUTH[@]}" \
    -d @-
```

Response: `{ ok, mode, tokenHits, vectorHits, results:[{path,title,snippet,score,titleMatch,
vectorScore?,content?}] }`. `mode` tells you which paths ran (keyword / vector / hybrid).
Parse `summary:` out of each result's `content` frontmatter.

**Alternative (if the llm_wiki MCP server is configured)**: `llm_wiki_search` with
`{query, top_k:8, include_content:true, bc?}` returns the same results; `llm_wiki_read_file`
reads any page by path. Use whichever is available — same data either way.

**Errors**: `401` → set `LLM_WIKI_API_TOKEN` (or allow-unauthenticated in Settings); connection
refused → desktop app not running / API disabled. Note: the search backend dislikes bursts —
call **serially**, one query at a time.

---

## Invariants

- `summary` is quoted **verbatim** — re-summarizing defeats the pre-generated-summary design.
- Recall only; this skill never writes. To persist a finding use `/wiki-add` or `/wiki-distill`.
- A genuine "nothing found" is a valid, honest result — never fabricate a plausible page.
- Single-hop only. The moment the question is "how do X and Y relate / what's upstream of Z",
  hand off to `/wiki-find-deep`.
- Surface the match basis (keyword vs vector) so the user can judge a synonym near-miss.
