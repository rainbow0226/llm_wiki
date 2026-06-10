// DEVWIKI: shared graph palettes + color helpers, extracted from
// graph-view.tsx so the 2D (sigma) and 3D (three.js) renderers stay in
// sync on node/community colors.

export const NODE_TYPE_COLORS: Record<string, string> = {
  entity: "#60a5fa",    // blue-400
  concept: "#c084fc",   // purple-400
  source: "#fb923c",    // orange-400
  query: "#4ade80",     // green-400
  synthesis: "#f87171", // red-400
  overview: "#facc15",  // yellow-400
  comparison: "#2dd4bf", // teal-400
  finding: "#a855f7",    // purple-500
  thesis: "#f43f5e",     // rose-500
  methodology: "#14b8a6", // teal-500
  // DEVWIKI: enterprise page types (match wiki-type-style.ts hues)
  solution: "#8b5cf6",   // violet-500
  playbook: "#0ea5e9",   // sky-500
  decision: "#f97316",   // orange-500
  learning: "#84cc16",   // lime-500
  "bc-readme": "#d946ef", // fuchsia-500
  meta: "#d946ef",        // fuchsia-500
  other: "#94a3b8",     // slate-400
}

export const CUSTOM_NODE_COLORS = [
  "#38bdf8",
  "#34d399",
  "#fbbf24",
  "#fb7185",
  "#a78bfa",
  "#22d3ee",
  "#f97316",
  "#84cc16",
]

export const COMMUNITY_COLORS = [
  "#60a5fa",  // blue-400
  "#4ade80",  // green-400
  "#fb923c",  // orange-400
  "#c084fc",  // purple-400
  "#f87171",  // red-400
  "#2dd4bf",  // teal-400
  "#facc15",  // yellow-400
  "#f472b6",  // pink-400
  "#a78bfa",  // violet-400
  "#38bdf8",  // sky-400
  "#34d399",  // emerald-400
  "#fbbf24",  // amber-400
]

export type ColorMode = "type" | "community"

export function nodeColor(type: string): string {
  if (NODE_TYPE_COLORS[type]) return NODE_TYPE_COLORS[type]
  let hash = 0
  for (const char of type) hash = (hash * 31 + char.charCodeAt(0)) >>> 0
  return CUSTOM_NODE_COLORS[hash % CUSTOM_NODE_COLORS.length] ?? NODE_TYPE_COLORS.other
}

export function communityColor(community: number): string {
  return COMMUNITY_COLORS[((community % COMMUNITY_COLORS.length) + COMMUNITY_COLORS.length) % COMMUNITY_COLORS.length] ?? "#94a3b8"
}

export function hexToRgba(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  return `rgba(${r},${g},${b},${alpha})`
}

export function mixColor(color1: string, color2: string, ratio: number): string {
  const hex = (c: string) => parseInt(c, 16)
  const r1 = hex(color1.slice(1, 3)), g1 = hex(color1.slice(3, 5)), b1 = hex(color1.slice(5, 7))
  const r2 = hex(color2.slice(1, 3)), g2 = hex(color2.slice(3, 5)), b2 = hex(color2.slice(5, 7))
  const r = Math.round(r1 + (r2 - r1) * ratio)
  const g = Math.round(g1 + (g2 - g1) * ratio)
  const b = Math.round(b1 + (b2 - b1) * ratio)
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`
}
