import type { ApiGraphNode } from "./api-client.js"

// DEVWIKI (P3): breadth-first multi-hop traversal over the wiki graph. Edges
// are treated as undirected (wikilinks are deduped to one entry per pair).
// Returns the reached sub-graph grouped by hop distance, capped at maxNodes.
export function traverseGraph(
  nodes: ApiGraphNode[],
  edges: Array<{ source: string; target: string }>,
  seedQuery: string,
  depth: number,
  maxNodes: number,
): string {
  const byId = new Map(nodes.map((n) => [n.id, n]))

  // Resolve seed: exact id, then case-insensitive id/label.
  const q = seedQuery.toLowerCase()
  const seed =
    byId.get(seedQuery) ??
    nodes.find((n) => n.id.toLowerCase() === q || n.label.toLowerCase() === q)
  if (!seed) {
    return `# Graph traversal\n\nNo page matched seed "${seedQuery}". Use the page id (file stem) or exact title.`
  }

  // Undirected adjacency.
  const adj = new Map<string, Set<string>>()
  for (const n of nodes) adj.set(n.id, new Set())
  for (const e of edges) {
    if (adj.has(e.source) && adj.has(e.target)) {
      adj.get(e.source)!.add(e.target)
      adj.get(e.target)!.add(e.source)
    }
  }

  // BFS, recording hop distance; stop expanding past `depth` and cap node count.
  const hop = new Map<string, number>([[seed.id, 0]])
  let frontier = [seed.id]
  for (let d = 1; d <= depth && hop.size < maxNodes; d++) {
    const next: string[] = []
    for (const id of frontier) {
      for (const nbr of adj.get(id) ?? []) {
        if (!hop.has(nbr)) {
          hop.set(nbr, d)
          next.push(nbr)
          if (hop.size >= maxNodes) break
        }
      }
      if (hop.size >= maxNodes) break
    }
    frontier = next
  }

  const reached = [...hop.keys()]
  const reachedSet = new Set(reached)
  const subEdges = edges.filter((e) => reachedSet.has(e.source) && reachedSet.has(e.target))

  const lines = [
    `# Graph traversal from "${seed.label}"`,
    "",
    `Seed: ${seed.id} (${seed.type})`,
    `Depth: ${depth}   Reached: ${reached.length} nodes, ${subEdges.length} edges` +
      (hop.size >= maxNodes ? ` (capped at ${maxNodes})` : ""),
    "",
  ]
  for (let d = 0; d <= depth; d++) {
    const atHop = reached
      .filter((id) => hop.get(id) === d)
      .map((id) => byId.get(id))
      .filter((n): n is ApiGraphNode => Boolean(n))
      .sort((a, b) => (b.linkCount ?? 0) - (a.linkCount ?? 0))
    if (atHop.length === 0) continue
    lines.push(`## Hop ${d}${d === 0 ? " (seed)" : ""}`)
    for (const n of atHop) {
      const comm = n.community !== undefined ? `, community ${n.community}` : ""
      lines.push(`- ${n.label} (${n.type}, ${n.linkCount ?? 0} links${comm}) — ${n.id}`)
    }
    lines.push("")
  }
  if (subEdges.length > 0) {
    lines.push("## Connections")
    for (const e of subEdges) lines.push(`- ${e.source} — ${e.target}`)
  }
  return lines.join("\n").trimEnd()
}

