import assert from "node:assert/strict"
import { test } from "node:test"
import { traverseGraph } from "../src/graph-traverse.js"
import type { ApiGraphNode } from "../src/api-client.js"

// a — b — c — d  (a chain), plus an off-path e linked to a.
const NODES: ApiGraphNode[] = [
  { id: "a", label: "Alpha", type: "concept", linkCount: 2 },
  { id: "b", label: "Beta", type: "concept", linkCount: 2 },
  { id: "c", label: "Gamma", type: "concept", linkCount: 2 },
  { id: "d", label: "Delta", type: "concept", linkCount: 1 },
  { id: "e", label: "Epsilon", type: "entity", linkCount: 1 },
]
const EDGES = [
  { source: "a", target: "b" },
  { source: "b", target: "c" },
  { source: "c", target: "d" },
  { source: "a", target: "e" },
]

test("BFS depth limits how far the traversal expands", () => {
  const out = traverseGraph(NODES, EDGES, "a", 1, 50)
  // depth 1 from a reaches b and e, but not c or d.
  assert.match(out, /## Hop 1/)
  assert.match(out, /Beta/)
  assert.match(out, /Epsilon/)
  assert.doesNotMatch(out, /Gamma/)
  assert.doesNotMatch(out, /Delta/)
})

test("BFS records hop distance for each reached node", () => {
  const out = traverseGraph(NODES, EDGES, "a", 3, 50)
  // a@0, {b,e}@1, c@2, d@3 — all five reached at depth 3.
  assert.match(out, /Reached: 5 nodes/)
  const hop2 = out.indexOf("## Hop 2")
  const hop3 = out.indexOf("## Hop 3")
  assert.ok(hop2 > 0 && hop3 > hop2)
  // Gamma is two hops from a, Delta three.
  assert.ok(out.indexOf("Gamma") > hop2 && out.indexOf("Gamma") < hop3)
  assert.ok(out.indexOf("Delta") > hop3)
})

test("seed resolves by title as well as id", () => {
  const out = traverseGraph(NODES, EDGES, "Gamma", 1, 50)
  assert.match(out, /from "Gamma"/)
  assert.match(out, /Seed: c \(concept\)/)
})

test("max_nodes caps the traversal", () => {
  const out = traverseGraph(NODES, EDGES, "a", 3, 2)
  assert.match(out, /capped at 2/)
})

test("unknown seed returns a helpful message", () => {
  const out = traverseGraph(NODES, EDGES, "nope", 2, 50)
  assert.match(out, /No page matched seed "nope"/)
})

test("edge_type filter expands along one relation only", () => {
  // a —link— b, a —prerequisite→ p, p —prerequisite→ q
  const nodes: ApiGraphNode[] = [
    { id: "a", label: "A", type: "decision", linkCount: 1 },
    { id: "b", label: "B", type: "concept", linkCount: 1 },
    { id: "p", label: "P", type: "concept", linkCount: 1 },
    { id: "q", label: "Q", type: "concept", linkCount: 1 },
  ]
  const edges = [
    { source: "a", target: "b", relation: "link" },
    { source: "a", target: "p", relation: "prerequisite" },
    { source: "p", target: "q", relation: "prerequisite" },
  ]
  const out = traverseGraph(nodes, edges, "a", 2, 50, "prerequisite")
  // Follows prerequisite chain a→p→q, ignores the link to b.
  assert.match(out, /along "prerequisite" edges/)
  assert.match(out, /\bP\b/)
  assert.match(out, /\bQ\b/)
  assert.doesNotMatch(out, /\bB \(concept/)
  // Typed edges render directed with their relation label.
  assert.match(out, /a → p \[prerequisite\]/)
})
