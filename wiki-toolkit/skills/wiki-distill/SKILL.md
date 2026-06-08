---
name: wiki-distill
description: >
  Distill the key findings of the current Claude Code session/research into a `learning` page
  and persist it to dev_wiki via the write channel (POST /api/v1/projects/{id}/sources). Use at
  the END of a working session to capture what was discovered — the non-obvious decisions,
  gotchas, and conclusions — as one self-contained experience page under wiki/learnings/.
  Distinct from /wiki-add (which writes a single piece of knowledge you already have);
  /wiki-distill synthesizes a whole session and dedups against existing learnings.
  Triggers on: "/wiki-distill", "wiki-distill", "distill this session", "capture what we learned",
  "沉淀这次会话", "总结这次调研写进知识库", "save session learnings".
allowed-tools: Read, Grep, Glob, Bash
---

# wiki-distill: session → learning page

Turn the session's discoveries into one `learning` page and post it through the write channel.
`learning` pages are **source-only (not actively recalled), but the `summary` is still required**
— write it as if it will be the only thing a future reader sees.

This skill WRITES. Show the user the drafted page (title + summary + key points) before posting.

---

## What makes a good distillation

Capture the things that were NOT obvious at the start and would save the next person time:

- Decisions made and the trade-off axis behind them (link to / or suggest a `decision` page).
- Gotchas, dead-ends, and "it actually works like X not Y" corrections.
- The concrete conclusion + its conditions of validity (when does this apply / not apply).

Do NOT dump the whole transcript. A distillation is a tight synthesis, not a log.

---

## Procedure

1. **Gather findings** from the session — the key conclusions, corrections, and decisions.
   If the user named specific takeaways, lead with those.
2. **Dedup** against existing learnings: `Glob wiki/learnings/*.md` and `Grep` for the topic.
   If a near-duplicate exists, prefer **updating** it (re-POST with `overwrite:true` after
   confirmation) over creating a second page.
3. **Pick `bc`** from `wiki/_meta/bc-registry.yaml` (via `keyword_to_bc`); must be a registered
   active id. Cross-cutting session → `general`.
4. **Draft the page** from `wiki-toolkit/templates/learning.md`:
   - `type: learning`, `sdlc_phases: []` (unless the learning is phase-specific).
   - `summary`: 100-150 中文字, self-contained — the finding + when it applies.
   - `sources`: the session/research origin (e.g. "Claude Code 会话 2026-06-08：<topic>").
   - body: a short "结论 / 关键发现 / 适用边界 / 踩过的坑" structure; `[[wikilink]]` related pages.
5. **Show the draft** (title + summary + bullet points) to the user. Adjust on feedback.
6. **POST** to `wiki/learnings/<slug>.md` (same write channel as `/wiki-add`).

---

## Posting (same channel as /wiki-add)

```bash
BASE="http://127.0.0.1:19828/api/v1"
TOKEN="${LLM_WIKI_API_TOKEN:-}"
REL="wiki/learnings/$(date +%F)-my-topic.md"
PAGE="$(mktemp)"; cat > "$PAGE" <<'MD'
---
type: learning
title: "经验：<一句话结论>"
summary: …(100-150 中文字，含适用条件)…
bc: general
sdlc_phases: []
tags: [<标签>, 经验沉淀]
related: []
created: 2026-06-08
updated: 2026-06-08
sources: ["Claude Code 会话 2026-06-08：<topic>"]
---

# 经验：<一句话结论>

## 结论
## 关键发现
## 适用边界
## 踩过的坑
MD

jq -n --arg path "$REL" --rawfile content "$PAGE" \
  '{path:$path, content:$content, overwrite:false}' \
| curl -sS -X POST "$BASE/projects/current/sources" \
    -H "Content-Type: application/json" \
    ${TOKEN:+-H "Authorization: Bearer $TOKEN"} \
    -d @-
rm -f "$PAGE"
```

Response/error handling is identical to `/wiki-add` (409 → update vs new slug; 401 → token;
connection refused → app not running). On success the learning is on disk immediately; vector
embedding runs in the app.

---

## Invariants

- Synthesize, don't transcribe. One tight page beats a transcript dump.
- `summary` mandatory even though learnings aren't actively recalled — never blank/`TODO`.
- Prefer updating a near-duplicate learning over spawning a second one.
- Filename convention: `YYYY-MM-DD-<topic-slug>.md` so learnings sort chronologically.
- Post through `POST /sources`; don't hand-write into `wiki/learnings/` (loses the embed trigger).
- A new `bc` is a registry decision, not an ad-hoc value — fall back to `general` if unsure.
