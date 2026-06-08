---
name: wiki-find-deep
description: >
  Deep multi-hop retrieval from dev_wiki. Starts from a hybrid-search seed set, then expands
  1-2 hops over the knowledge graph (wikilinks + `related:` frontmatter + the /graph adjacency)
  to pull in connected pages the flat search misses, and synthesizes a connected map clustered
  by bounded context. Slower (~30-60s) and broader than /wiki-find — use when the question is
  about how things RELATE ("what's upstream of X", "what does this decision depend on",
  "everything connected to Y"), not a single lookup.
  Triggers on: "/wiki-find-deep", "wiki-find-deep", "deep search", "how does X relate to",
  "what's connected to", "trace dependencies", "多跳召回", "顺着关系展开", "图谱召回".
allowed-tools: Read, Grep, Glob, Bash
---

# wiki-find-deep: multi-hop graph-expanded recall

Seed with hybrid search, then walk the knowledge graph to gather the neighborhood, then
synthesize. Where `/wiki-find` answers "what do we know about X", this answers "what is X
connected to, and how". Budget ~30-60s.

> **P3 shipped typed edges + a traversal tool.** The graph now carries directed typed edges from
> frontmatter — `prerequisites` (→ "prerequisite"), `supersedes`, `related_decisions` (→
> "related-decision") — alongside untyped `link` (wikilink) edges, each on `edges[].relation`.
> The fastest expansion path is the **`llm_wiki_graph_traverse`** MCP tool (BFS from a seed page,
> `depth` 1-3, optional `edge_type` to follow one relation, e.g. `edge_type: "prerequisite"` to
> pull a learning-order chain). `GET /graph?with_insights=true` also returns Louvain
> `communities`, `surprisingConnections`, and `knowledgeGaps` to enrich the synthesis.

---

## Procedure

1. **Seed search.** Run the hybrid search (`top_k: 5`, `include_content: true`, optional `bc`)
   exactly like `/wiki-find`. These are hop-0 pages. Keep their `summary` + `path` + slug
   (slug = file stem of the path; graph node ids are slugs).
2. **Expand hop-1.** Fastest path: `llm_wiki_graph_traverse` with each seed slug (`depth: 1-2`,
   optional `edge_type` to follow one relation — e.g. `"prerequisite"` for learning order). It
   returns nodes grouped by hop + directed typed connections, so you can skip manual adjacency
   walking. Otherwise, collect neighbors two ways and union them:
   - **In-page links**: parse `[[wikilinks]]` and the `related:` frontmatter list from the seed
     content (already have it from `include_content`).
   - **Graph adjacency**: `GET /graph` → `edges:[{source,target,relation}]`; take edges touching a
     seed slug. `relation` distinguishes `link` (wikilink) from typed `prerequisite`/`supersedes`/
     `related-decision`. (`nodes:[{id,label,nodeType,path,linkCount,community}]` gives each
     neighbor's path/type/cluster.)
   Drop neighbors already in the seed set.
3. **Read hop-1 summaries.** Fetch each new neighbor's page (`GET /files/content?path=` or
   `llm_wiki_read_file`) and extract its `summary` + `bc` + `type`.
4. **Optional hop-2** (only if hop-1 is thin or the user asked to go deep): repeat step 2 from
   the most relevant hop-1 pages. **Hard caps: ≤2 hops, ≤~18 pages total.** Stop early when new
   hops stop adding on-topic pages.
5. **Synthesize a connected map**, not a flat list:
   - Cluster pages by `bc` (bounded context).
   - For each, give title · type · path · **summary verbatim**, and **how it connects** (which
     seed it links from, via `[[wikilink]]` / `related` / shared bc).
   - Call out bridges: a page linking two otherwise-separate clusters is often the key insight.
   - End with a 2-3 line synthesis of what the neighborhood says as a whole.

---

## Calls

```bash
BASE="http://127.0.0.1:19828/api/v1"; TOKEN="${LLM_WIKI_API_TOKEN:-}"
AUTH=(); [ -n "$TOKEN" ] && AUTH=(-H "Authorization: Bearer $TOKEN")  # robust in bash AND zsh

# hop-0 seeds
jq -n --arg q "跨链桥安全" '{query:$q, topK:5, includeContent:true}' \
| curl -sS -X POST "$BASE/projects/current/search" -H "Content-Type: application/json" "${AUTH[@]}" -d @-

# adjacency + insights for hop expansion (edges carry `relation`; insights add communities/gaps)
curl -sS "$BASE/projects/current/graph?limit=1000&with_insights=true" "${AUTH[@]}"

# read a specific neighbor page
curl -sS "$BASE/projects/current/files/content?path=wiki/concepts/atomic-swap.md" "${AUTH[@]}"
```

**MCP alternative**: `llm_wiki_search` (seeds), **`llm_wiki_graph_traverse`** (BFS hop expansion
from a seed slug — `depth`, optional `edge_type`), `llm_wiki_graph` (full adjacency + insights),
`llm_wiki_read_file` (neighbor pages) — same data. Reminder: search backend is burst-sensitive →
fan out **serially**, not in parallel.

**Errors**: identical to `/wiki-find` (401 token / connection refused). If `/graph` is empty,
fall back to wikilink/`related` parsing only — the graph endpoint is just an adjacency cache.

---

## Invariants

- Bounded walk: ≤2 hops, ≤~18 pages, serial calls. Announce when a cap truncated the expansion
  (don't silently present a partial neighborhood as complete).
- `summary` quoted verbatim per page; the synthesis paragraph is the only new prose you add.
- Output is a **connected map** (clusters + edges), not a longer flat ranking — if it reads like
  `/wiki-find` output, you didn't expand.
- Recall only; never writes. Persist conclusions via `/wiki-distill`.
- Edges are currently untyped (wikilink/related) — don't claim a `prerequisites`/`supersedes`
  relationship the data doesn't carry yet (that's P3).
