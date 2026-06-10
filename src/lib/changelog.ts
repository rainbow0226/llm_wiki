/**
 * Changelog shown in Settings → Changelog. Hardcoded rather than
 * pulled from GitHub Releases so it works offline and stays under
 * version control with the code that ships the changes.
 *
 * DEVWIKI: this is dev_wiki's own changelog. The project forked from
 * nashsu/llm_wiki at upstream v0.4.22 and versions independently from
 * v0.0.1; upstream release history lives in the upstream repo, not here.
 *
 * Conventions:
 *   - Newest version first (the UI renders in array order).
 *   - Each entry has both `en` and `zh` highlight lists; the
 *     section picks whichever matches the current i18n language.
 *   - Only user-visible changes belong here. Internal refactors,
 *     CI tweaks, and pure test work go in commit messages, not
 *     here — keep this readable for end users.
 *   - When releasing a new version: prepend a new entry with the
 *     same shape, then bump package.json / tauri.conf.json /
 *     Cargo.toml / Cargo.lock as usual.
 */

export interface ChangelogEntry {
  version: string
  date: string // YYYY-MM-DD
  highlights: {
    en: string[]
    zh: string[]
  }
}

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "0.0.1",
    date: "2026-06-10",
    highlights: {
      en: [
        "First dev_wiki release — a knowledge base for the full software development lifecycle, forked from nashsu/llm_wiki (upstream v0.4.22).",
        "Enterprise knowledge organization: 5 new page types (solution / playbook / decision / learning / bc-readme) alongside the native ones, with bounded-context (bc) registry, controlled vocabulary, and SDLC-phase fields driven by your vault's schema.md.",
        "Write channel: POST /sources HTTP endpoint plus Claude Code skills (wiki-add, wiki-distill, wiki-find, wiki-find-deep, wiki-lint) and SessionStart/Stop hooks with hot.md — capture and recall knowledge without leaving your coding session.",
        "Knowledge graph insights everywhere: Louvain communities, surprising connections, and knowledge gaps are now computed in the Rust backend and exposed via /graph?with_insights, typed semantic edges (prerequisites / supersedes / related_decisions), and an MCP graph_traverse tool for multi-hop exploration.",
        "Retrieval quality overhaul: keyword scoring replaced with Okapi BM25 (field-weighted title / summary / body), SDLC-phase-aware type weighting, per-query retrieval trace, and an offline eval harness (Hit@k / MRR / nDCG) with regression gating.",
        "New identity: galaxy-themed app icon, per-type icons across the knowledge tree, search results, citations, and activity panel.",
      ],
      zh: [
        "dev_wiki 首个版本——面向开发全流程的知识库，fork 自 nashsu/llm_wiki（上游 v0.4.22）。",
        "企业级知识组织：在原生类型之外新增 5 类页面（solution / playbook / decision / learning / bc-readme），配套 bounded context（bc）注册表、受控词表和 SDLC 阶段字段，由 vault 的 schema.md 驱动路由。",
        "写入通道：新增 POST /sources HTTP 端点，配套 Claude Code skills（wiki-add、wiki-distill、wiki-find、wiki-find-deep、wiki-lint）与 SessionStart/Stop hooks + hot.md——在编码会话内直接沉淀和召回知识。",
        "知识图谱洞察全面开放：Louvain 社区、惊奇连接、知识空白移入 Rust 后端，经 /graph?with_insights 暴露；支持有向语义边（prerequisites / supersedes / related_decisions）和 MCP graph_traverse 多跳遍历工具。",
        "召回质量重构：关键词打分替换为 Okapi BM25（标题/摘要/正文字段加权），SDLC 阶段感知的类型权重，每次检索可输出结构化 trace，并配离线评估工具（Hit@k / MRR / nDCG）和回归门禁。",
        "全新视觉标识：银河主题应用图标，知识树、搜索结果、引用列表和活动面板的页面类型均有专属图标。",
      ],
    },
  },
]
