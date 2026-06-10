export const GENERATION_WIKI_TYPES = [
  "source",
  "entity",
  "concept",
  "comparison",
  "query",
  "synthesis",
  "thesis",
  "methodology",
  "finding",
] as const

const WIKI_TYPE_DIRS: Array<{ dir: string; type: string }> = [
  { dir: "entities", type: "entity" },
  { dir: "concepts", type: "concept" },
  { dir: "sources", type: "source" },
  { dir: "queries", type: "query" },
  { dir: "comparisons", type: "comparison" },
  { dir: "synthesis", type: "synthesis" },
  { dir: "findings", type: "finding" },
  { dir: "thesis", type: "thesis" },
  { dir: "methodology", type: "methodology" },
  // DEVWIKI: enterprise page types (P0) — singular type from plural dir,
  // so path inference matches the frontmatter `type:` instead of falling
  // through to the raw directory name.
  { dir: "solutions", type: "solution" },
  { dir: "playbooks", type: "playbook" },
  { dir: "decisions", type: "decision" },
  { dir: "learnings", type: "learning" },
  // _meta/ pages (bc-readme overviews, registry README) → "meta", which the
  // style registry maps to the Boxes icon; the raw dir name "_meta" matched
  // the custom-dir fallback before and styled as a plain document.
  { dir: "_meta", type: "meta" },
]

export function inferWikiTypeFromPath(path: string, fileName?: string): string | null {
  const normalized = path.replace(/\\/g, "/").toLowerCase()
  for (const { dir, type } of WIKI_TYPE_DIRS) {
    if (normalized.includes(`/wiki/${dir}/`) || normalized.includes(`/${dir}/`) || normalized.startsWith(`wiki/${dir}/`)) {
      return type
    }
  }
  const name = (fileName ?? normalized.split("/").pop() ?? "").toLowerCase()
  if (name === "overview.md" || normalized.includes("/overview.md")) return "overview"
  const customDir = normalized.match(/(?:^|\/)wiki\/([^/.][^/]*)\/[^/]+\.md$/)?.[1]
  if (customDir) return customDir
  return null
}

export function wikiTypeLabel(type: string): string {
  if (type === "thesis") return "Thesis"
  if (type === "methodology") return "Methodology"
  if (type === "finding") return "Finding"
  return type
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ")
}
