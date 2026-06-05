---
name: kb-lint
description: >
  Health-check an llm_wiki enterprise vault against the企业级 schema. Validates required
  frontmatter (summary/bc/sdlc_phases), summary length, bc against wiki/_meta/bc-registry.yaml,
  sdlc_phases/type against scope-vocabulary.yaml, type↔directory routing, and dangling wikilinks.
  Read-only; emits a tiered report and only auto-fixes after explicit confirmation.
  Triggers on: "/kb-lint", "kb-lint", "lint the wiki", "knowledge base health check",
  "check frontmatter", "find dangling wikilinks", "wiki audit", "check bc consistency".
allowed-tools: Read, Grep, Glob, Bash
---

# kb-lint: Enterprise Wiki Health Check

Validate every page under `wiki/` against the enterprise knowledge schema. **Read-only by
default** — produce the report first, then ask before fixing anything.

Run after every 10-15 ingests, or before a merge. Scope is the current project/vault
(`<vault>/wiki/`). Authoritative inputs:

- `schema.md` (vault root) — field/type definitions.
- `wiki/_meta/bc-registry.yaml` — legal `bc:` ids (`status: active` or `deprecated`).
- `wiki/_meta/scope-vocabulary.yaml` — legal `type:` and `sdlc_phases:` values; `keyword_to_bc` map.

If `bc-registry.yaml` or `scope-vocabulary.yaml` is missing, report that as a BLOCKER first
(checks that depend on them are skipped, not silently passed).

---

## Procedure

1. **Load registries.** Read `wiki/_meta/bc-registry.yaml` → set of legal bc ids (+ status).
   Read `wiki/_meta/scope-vocabulary.yaml` → legal `page_types`, `sdlc_phases`, `keyword_to_bc`.
2. **Enumerate pages.** `Glob` `wiki/**/*.md`. Exclude: `wiki/index.md`, `wiki/log.md`,
   `wiki/overview.md`, `wiki/_meta/README.md`, and any `wiki/_meta/*.yaml` (not pages).
   `wiki/_meta/<bc-id>.md` are bc-readme pages and ARE linted.
3. **Per page**, parse YAML frontmatter and run the checks below.
4. **Build the wikilink graph** for dangling-link + orphan checks (collect every `[[target]]`,
   strip `|alias` and `#heading`, resolve against the set of page slugs/filenames).
5. **Emit report** to `wiki/_meta/lint-report-YYYY-MM-DD.md`. Tier by severity. Never auto-fix
   before showing it.

---

## Checks (in order; each finding tagged BLOCKER / ERROR / WARN / INFO)

| # | Check | Rule | Severity |
|---|-------|------|----------|
| 1 | **Registry present** | `bc-registry.yaml` + `scope-vocabulary.yaml` exist & parse | BLOCKER if missing |
| 2 | **Required frontmatter** | `type, title, summary, bc, sdlc_phases, tags, related, created, updated` all present | ERROR per missing field |
| 3 | **summary non-empty** | `summary:` present and not blank | ERROR |
| 4 | **summary length** | 中文 60-180 字 (schema 目标 100-150；<60 太薄 WARN，>180 超长 WARN) | WARN |
| 5 | **type legal** | `type` ∈ `page_types` | ERROR |
| 6 | **bc in registry** | `bc` ∈ registry ids | ERROR if absent; WARN if id `status: deprecated` |
| 7 | **sdlc_phases legal** | every element ∈ `sdlc_phases` vocab; value is a list (may be `[]`) | ERROR |
| 8 | **type ↔ directory** | page's directory matches its `type` per schema routing (e.g. `type: decision` ⇒ under `wiki/decisions/`; `bc-readme` ⇒ `wiki/_meta/`) | WARN (mismatch may be intentional alias dir) |
| 9 | **bc plausibility** | scan body/title for `keyword_to_bc` keywords; if a strong keyword maps to a different bc than declared, flag as possible mis-tag | INFO |
| 10 | **dangling wikilink** | every `[[target]]` resolves to an existing page | ERROR |
| 11 | **orphan page** | non-overview/non-_meta page with zero inbound wikilinks | INFO |
| 12 | **bc-readme coverage** | every registry context with a `readme:` path has that page; every active bc used by ≥1 page has a bc-readme | INFO |

Notes:
- **summary** is the most important field (it is the only string spliced directly into recall).
  An empty or missing summary is always ERROR, never INFO.
- Counting 中文字数: count CJK characters + non-CJK words approximately; the goal is to catch
  blank/one-line summaries and runaway 300-字 summaries, not to be exact.
- A `bc` value not in the registry is an ERROR even if it "looks reasonable" — the fix is to
  register it in `bc-registry.yaml`, not to accept ad-hoc domains.

---

## Report Format

Write to `wiki/_meta/lint-report-YYYY-MM-DD.md`:

```markdown
---
type: bc-readme
title: "Lint Report YYYY-MM-DD"
bc: general
sdlc_phases: []
summary: kb-lint 巡检报告，记录本次扫描的页面数、各严重度问题数与明细。
tags: [meta, lint]
created: YYYY-MM-DD
updated: YYYY-MM-DD
---

# Lint Report: YYYY-MM-DD

## Summary
- Pages scanned: N
- BLOCKER: N | ERROR: N | WARN: N | INFO: N

## BLOCKER
- <registry missing / unparseable>

## ERROR
- `wiki/decisions/foo.md`: missing summary.
- `wiki/playbooks/bar.md`: bc `paymnet` not in registry (did you mean `payment`?).
- `wiki/concepts/baz.md`: dangling wikilink [[non-existent-page]].
- `wiki/concepts/baz.md`: sdlc_phases value `deploy` not legal (use design/dev/test/ops).

## WARN
- `wiki/decisions/foo.md`: summary 41 字 (target 100-150, floor 60).
- `wiki/solutions/qux.md`: type=solution but located under wiki/concepts/.

## INFO
- `wiki/entities/quux.md`: orphan (no inbound wikilinks).
- bc `crosschain` is active but has no bc-readme page.
- `wiki/concepts/foo.md`: body mentions "智能合约" (→ethereum) but bc=bitcoin; verify.
```

---

## Acceptance

This skill satisfies the P1 acceptance criterion: **`/kb-lint` can surface pages missing a
`summary:`**. Check #3 reports every such page under ERROR.

---

## Before Auto-Fixing

Show the report first. Then ask: "Fix automatically, or review each?"

Safe to auto-fix (with confirmation):
- Adding missing frontmatter keys with placeholder values (`summary: TODO` is NOT acceptable —
  a real summary must be written; flag for human).
- Removing or correcting a dangling wikilink when the intended target is unambiguous.

Needs human review:
- Writing/condensing a `summary` (semantic — never auto-generate silently).
- Registering a new `bc` (decide id + scope in `bc-registry.yaml`).
- Re-homing a page to match its `type` (may break inbound links).
- Deleting orphans (may be intentional).

## Invariants

- Read-only scan; the report is the only file written without confirmation.
- Registry-dependent checks are **skipped and reported**, never silently passed, when a
  registry file is missing.
- Severities are not collapsed: an empty summary is ERROR even when everything else passes.
