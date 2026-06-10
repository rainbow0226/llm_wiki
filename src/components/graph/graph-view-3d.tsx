// DEVWIKI: 3D knowledge-graph renderer (three.js via react-force-graph-3d).
// Lazy-loaded from graph-view.tsx so three.js is only fetched when the user
// switches to 3D. Shares node/community palettes with the 2D sigma renderer
// through graph-colors.ts.
//
// Aesthetic choices:
//   - deep-space background to match the dev_wiki galaxy branding; bloom
//     post-processing makes node spheres glow like stars
//   - links blend their endpoint colors and stay translucent so dense
//     clusters read as nebulae instead of hairballs
//   - hovering a node lights its neighborhood and runs directional
//     particles along the touched links
//   - top-degree nodes carry always-on sprite labels; everything else
//     reveals its label on hover
//   - slow auto-rotate at start (stops on first drag) gives the "3D" feel
//     without the user having to do anything

import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import ForceGraph3D, { type ForceGraphMethods, type NodeObject, type LinkObject } from "react-force-graph-3d"
import * as THREE from "three"
import { UnrealBloomPass } from "three/examples/jsm/postprocessing/UnrealBloomPass.js"
import SpriteText from "three-spritetext"
import type { GraphNode, GraphEdge } from "@/lib/wiki-graph"
import { nodeColor, communityColor, mixColor, hexToRgba, type ColorMode } from "./graph-colors"

interface Node3D extends NodeObject {
  id: string
  label: string
  type: string
  path: string
  linkCount: number
  community: number
}

interface Link3D extends LinkObject {
  source: string | Node3D
  target: string | Node3D
  weight: number
}

interface GraphView3DProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  colorMode: ColorMode
  nodeScale: number
  highlightedNodes: Set<string>
  isDark: boolean
  onNodeClick: (nodeId: string) => void
  onNodeRightClick: (nodeId: string, x: number, y: number) => void
}

/** How many of the highest-degree nodes keep an always-visible label. */
const LABELED_NODE_BUDGET = 28

function endpointId(end: string | Node3D): string {
  return typeof end === "string" ? end : end.id
}

export default function GraphView3D({
  nodes,
  edges,
  colorMode,
  nodeScale,
  highlightedNodes,
  isDark,
  onNodeClick,
  onNodeRightClick,
}: GraphView3DProps) {
  const fgRef = useRef<ForceGraphMethods<Node3D, Link3D> | undefined>(undefined)
  const containerRef = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState<{ width: number; height: number }>({ width: 0, height: 0 })
  const [hoverNode, setHoverNode] = useState<Node3D | null>(null)

  // Track container size — ForceGraph3D needs explicit pixel dimensions.
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const sync = () => setSize({ width: el.clientWidth, height: el.clientHeight })
    sync()
    const ro = new ResizeObserver(sync)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // react-force-graph mutates the data objects it receives (layout x/y/z),
  // so hand it copies keyed by the upstream graph identity.
  const data = useMemo(() => {
    const nodeSet = new Set(nodes.map((n) => n.id))
    return {
      nodes: nodes.map((n) => ({ ...n })) as Node3D[],
      links: edges
        .filter((e) => nodeSet.has(e.source) && nodeSet.has(e.target))
        .map((e) => ({ ...e })) as Link3D[],
    }
  }, [nodes, edges])

  // Hover neighborhood: the hovered node, its neighbors, and touched links.
  const hoverNeighborhood = useMemo(() => {
    if (!hoverNode) return null
    const nodeIds = new Set<string>([hoverNode.id])
    const linkKeys = new Set<Link3D>()
    for (const link of data.links) {
      const s = endpointId(link.source)
      const t = endpointId(link.target)
      if (s === hoverNode.id || t === hoverNode.id) {
        nodeIds.add(s)
        nodeIds.add(t)
        linkKeys.add(link)
      }
    }
    return { nodeIds, links: linkKeys }
  }, [hoverNode, data.links])

  const labeledNodeIds = useMemo(() => {
    return new Set(
      [...nodes]
        .sort((a, b) => b.linkCount - a.linkCount)
        .slice(0, LABELED_NODE_BUDGET)
        .map((n) => n.id),
    )
  }, [nodes])

  const baseColor = useCallback(
    (node: Node3D) => (colorMode === "community" ? communityColor(node.community) : nodeColor(node.type)),
    [colorMode],
  )

  // Bloom pass: makes emissive node spheres glow. Softer in light mode
  // where a strong bloom washes the scene out.
  useEffect(() => {
    const fg = fgRef.current
    if (!fg || !size.width) return
    const composer = fg.postProcessingComposer()
    const bloom = new UnrealBloomPass(
      new THREE.Vector2(size.width, size.height),
      isDark ? 1.1 : 0.45, // strength
      0.65, // radius
      isDark ? 0.08 : 0.35, // luminance threshold
    )
    composer.addPass(bloom)
    return () => {
      composer.removePass(bloom)
      bloom.dispose()
    }
  }, [isDark, size.width, size.height])

  // Gentle auto-rotate until the user grabs the scene.
  useEffect(() => {
    const fg = fgRef.current
    if (!fg) return
    const controls = fg.controls() as { autoRotate?: boolean; autoRotateSpeed?: number; addEventListener?: (e: string, cb: () => void) => void }
    if (!controls) return
    controls.autoRotate = true
    controls.autoRotateSpeed = 0.55
    const stop = () => {
      controls.autoRotate = false
    }
    controls.addEventListener?.("start", stop)
    // No removeEventListener needed: controls are torn down with the canvas.
  }, [size.width])

  // Node = glowing sphere (+ sprite label for hubs). Memoized per
  // (colorMode, highlight, hover) via accessor identity — react-force-graph
  // rebuilds objects when nodeThreeObject changes.
  const nodeThreeObject = useCallback(
    (node: Node3D) => {
      const color = baseColor(node)
      const highlighted = highlightedNodes.size > 0 && highlightedNodes.has(node.id)
      const inHover = hoverNeighborhood?.nodeIds.has(node.id) ?? false
      const dimmed =
        (hoverNeighborhood !== null && !inHover) ||
        (highlightedNodes.size > 0 && !highlighted)

      const r = (3 + Math.sqrt(node.linkCount + 1) * 1.6) * nodeScale
      const group = new THREE.Group()
      const sphere = new THREE.Mesh(
        new THREE.SphereGeometry(r, 24, 24),
        new THREE.MeshLambertMaterial({
          color,
          transparent: true,
          opacity: dimmed ? 0.18 : 0.95,
          emissive: new THREE.Color(color),
          emissiveIntensity: dimmed ? 0.08 : highlighted || inHover ? 0.95 : 0.45,
        }),
      )
      group.add(sphere)

      const showLabel = !dimmed && (labeledNodeIds.has(node.id) || inHover || highlighted)
      if (showLabel) {
        const sprite = new SpriteText(node.label)
        sprite.color = isDark ? "#e2e8f0" : "#1e293b"
        sprite.backgroundColor = isDark ? "rgba(2,2,10,0.55)" : "rgba(248,250,252,0.7)"
        sprite.padding = 1.5
        sprite.borderRadius = 2
        sprite.textHeight = 3.4
        sprite.position.set(0, -(r + 4.5), 0)
        group.add(sprite)
      }
      return group
    },
    [baseColor, highlightedNodes, hoverNeighborhood, labeledNodeIds, nodeScale, isDark],
  )

  const linkColor = useCallback(
    (link: Link3D) => {
      const s = data.nodes.find((n) => n.id === endpointId(link.source))
      const t = data.nodes.find((n) => n.id === endpointId(link.target))
      const blend = s && t ? mixColor(baseColor(s), baseColor(t), 0.5) : "#64748b"
      const active = hoverNeighborhood?.links.has(link) ?? false
      if (hoverNeighborhood && !active) return hexToRgba(blend, 0.06)
      const alpha = active ? 0.85 : Math.min(0.16 + link.weight * 0.05, 0.38)
      return hexToRgba(blend, alpha)
    },
    [data.nodes, baseColor, hoverNeighborhood],
  )

  return (
    <div ref={containerRef} className="absolute inset-0">
      {size.width > 0 && (
        <ForceGraph3D<Node3D, Link3D>
          ref={fgRef}
          width={size.width}
          height={size.height}
          graphData={data}
          backgroundColor={isDark ? "#03030a" : "#f8fafc"}
          showNavInfo={false}
          nodeThreeObject={nodeThreeObject}
          nodeLabel={() => ""}
          linkColor={linkColor}
          linkWidth={(link) => ((hoverNeighborhood?.links.has(link) ?? false) ? 1.4 : 0.4)}
          linkOpacity={1}
          linkDirectionalParticles={(link) => ((hoverNeighborhood?.links.has(link) ?? false) ? 3 : 0)}
          linkDirectionalParticleWidth={1.6}
          linkDirectionalParticleSpeed={0.0065}
          warmupTicks={60}
          cooldownTime={6000}
          onNodeHover={(node) => setHoverNode((node as Node3D | null) ?? null)}
          onNodeClick={(node) => onNodeClick((node as Node3D).id)}
          onNodeRightClick={(node, event) => {
            const ev = event as MouseEvent
            onNodeRightClick((node as Node3D).id, ev.clientX, ev.clientY)
          }}
          onBackgroundClick={() => setHoverNode(null)}
        />
      )}
    </div>
  )
}
