// throwaway harness: render GraphView3D with fake data in a plain browser
import { createRoot } from "react-dom/client"
import GraphView3D from "./components/graph/graph-view-3d"

const nodes = Array.from({ length: 14 }, (_, i) => ({
  id: `n${i}`,
  label: `Node ${i}`,
  type: ["concept", "entity", "playbook", "decision"][i % 4],
  path: `wiki/x/n${i}.md`,
  linkCount: (i * 7) % 13,
  community: i % 3,
}))
const edges = Array.from({ length: 22 }, (_, i) => ({
  source: `n${i % 14}`,
  target: `n${(i * 3 + 1) % 14}`,
  weight: (i % 5) + 1,
}))

createRoot(document.getElementById("root")!).render(
  <GraphView3D
    nodes={nodes}
    edges={edges}
    colorMode="community"
    nodeScale={1}
    highlightedNodes={new Set()}
    isDark={true}
    onNodeClick={(id) => console.log("click", id)}
    onNodeRightClick={(id) => console.log("rclick", id)}
  />,
)
