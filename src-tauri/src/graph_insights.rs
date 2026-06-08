// DEVWIKI (P3): knowledge-graph insights, ported from the frontend
// (`src/lib/wiki-graph.ts` + `graph-insights.ts`) so the same community
// detection, surprising-connection scoring, and knowledge-gap heuristics are
// available headlessly via the HTTP API / MCP instead of only in the GUI.
//
// The frontend leans on `graphology-communities-louvain`; here Louvain is
// implemented directly (multi-level modularity maximization) so we keep zero
// new dependencies. Exact community ids need not match the GUI — the heuristics
// only need a reasonable partition.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::api_server::{ApiGraphEdge, ApiGraphNode};

const STRUCTURAL_IDS: [&str; 3] = ["index", "log", "overview"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityInfo {
    pub id: usize,
    pub node_count: usize,
    /// Intra-community edge density (actual / possible undirected pairs).
    pub cohesion: f64,
    /// Up to 5 member labels, highest link-count first.
    pub top_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurprisingConnection {
    pub source: String,
    pub target: String,
    pub score: i32,
    pub reasons: Vec<String>,
    /// Stable id (sorted endpoint ids) for client-side dismiss tracking.
    pub key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeGap {
    #[serde(rename = "type")]
    pub gap_type: String,
    pub title: String,
    pub description: String,
    pub node_ids: Vec<String>,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphInsights {
    pub communities: Vec<CommunityInfo>,
    /// node id → community id (so the caller can stamp each returned node).
    pub node_communities: BTreeMap<String, usize>,
    pub surprising_connections: Vec<SurprisingConnection>,
    pub knowledge_gaps: Vec<KnowledgeGap>,
}

/// Compute the full insight bundle for a graph. `nodes`/`edges` are the
/// unfiltered graph (insights describe the whole wiki, not a filtered view).
pub fn compute_insights(nodes: &[ApiGraphNode], edges: &[ApiGraphEdge]) -> GraphInsights {
    if nodes.is_empty() {
        return GraphInsights::default();
    }

    // id → contiguous index for the array-based Louvain core.
    let index: BTreeMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    // Symmetric weighted adjacency (each undirected edge in both endpoints).
    let mut adjacency: Vec<Vec<(usize, f64)>> = vec![Vec::new(); nodes.len()];
    for e in edges {
        let (Some(&s), Some(&t)) = (index.get(e.source.as_str()), index.get(e.target.as_str()))
        else {
            continue;
        };
        if s == t {
            continue;
        }
        let w = if e.weight > 0.0 { e.weight } else { 1.0 };
        adjacency[s].push((t, w));
        adjacency[t].push((s, w));
    }

    let raw_comm = louvain(nodes.len(), &adjacency, 1.0);
    let (communities, node_comm_by_index) = summarize_communities(nodes, edges, &index, &raw_comm);

    let node_communities: BTreeMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), node_comm_by_index[i]))
        .collect();

    let surprising_connections =
        find_surprising_connections(nodes, edges, &node_communities, 5);
    let knowledge_gaps =
        detect_knowledge_gaps(nodes, edges, &communities, &node_communities, 8);

    GraphInsights {
        communities,
        node_communities,
        surprising_connections,
        knowledge_gaps,
    }
}

// ---------------------------------------------------------------------------
// Louvain community detection (multi-level modularity maximization)
// ---------------------------------------------------------------------------

/// A weighted undirected graph for one Louvain level. `adj` is symmetric and
/// self-loop-free; aggregated intra-community weight lives in `self_loops`.
struct Level {
    adj: Vec<Vec<(usize, f64)>>,
    self_loops: Vec<f64>,
}

impl Level {
    /// Weighted degree (incident edge weights; a self-loop counts twice).
    fn degrees(&self) -> Vec<f64> {
        (0..self.adj.len())
            .map(|i| self.adj[i].iter().map(|&(_, w)| w).sum::<f64>() + 2.0 * self.self_loops[i])
            .collect()
    }
}

fn louvain(num_nodes: usize, adjacency: &[Vec<(usize, f64)>], resolution: f64) -> Vec<usize> {
    if num_nodes == 0 {
        return Vec::new();
    }
    // Final community per ORIGINAL node; updated as levels compose.
    let mut result: Vec<usize> = (0..num_nodes).collect();
    let mut level = Level {
        adj: adjacency.to_vec(),
        self_loops: vec![0.0; num_nodes],
    };

    loop {
        let (assignment, moved, num_comms) = one_level(&level, resolution);
        if !moved {
            break;
        }
        for c in result.iter_mut() {
            *c = assignment[*c];
        }
        if num_comms == level.adj.len() {
            break;
        }
        level = aggregate(&level, &assignment, num_comms);
        if level.adj.len() <= 1 {
            break;
        }
    }

    renumber(&mut result);
    result
}

/// Local-moving phase: greedily move each node to the neighboring community
/// that maximizes modularity gain, until a full pass makes no move. Returns the
/// (contiguous) community assignment, whether anything moved, and the count.
fn one_level(level: &Level, resolution: f64) -> (Vec<usize>, bool, usize) {
    let n = level.adj.len();
    let k = level.degrees();
    let m2: f64 = k.iter().sum();
    if m2 <= 0.0 {
        // No edges — every node is its own community.
        return ((0..n).collect(), false, n);
    }

    let mut community: Vec<usize> = (0..n).collect();
    let mut sigma_tot = k.clone();
    let mut improved = false;

    loop {
        let mut moved_this_pass = false;
        for i in 0..n {
            let ci = community[i];
            sigma_tot[ci] -= k[i];

            // Weight from i into each neighboring community.
            let mut neigh: BTreeMap<usize, f64> = BTreeMap::new();
            neigh.entry(ci).or_insert(0.0);
            for &(j, w) in &level.adj[i] {
                if j == i {
                    continue;
                }
                *neigh.entry(community[j]).or_insert(0.0) += w;
            }

            // Pick the community maximizing k_i_in(C) - γ·Σtot(C)·k_i / 2m.
            let mut best_c = ci;
            let mut best_gain = f64::NEG_INFINITY;
            for (&c, &k_i_in) in &neigh {
                let gain = k_i_in - resolution * sigma_tot[c] * k[i] / m2;
                // Prefer lower community id on ties for determinism; staying
                // (ci) wins ties because it is evaluated with equal gain and we
                // only replace on strict improvement.
                if gain > best_gain {
                    best_gain = gain;
                    best_c = c;
                }
            }

            sigma_tot[best_c] += k[i];
            if best_c != ci {
                community[i] = best_c;
                moved_this_pass = true;
                improved = true;
            }
        }
        if !moved_this_pass {
            break;
        }
    }

    let count = renumber(&mut community);
    (community, improved, count)
}

/// Contract each community into a single node, summing edge weights. Intra-
/// community weight folds into `self_loops`; inter-community weight into `adj`.
fn aggregate(level: &Level, assignment: &[usize], num_comms: usize) -> Level {
    let mut self_loops = vec![0.0; num_comms];
    for (i, &c) in assignment.iter().enumerate() {
        self_loops[c] += level.self_loops[i];
    }
    // Each undirected edge once (j > i), routed to self-loop or between-pair.
    let mut between: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for i in 0..level.adj.len() {
        for &(j, w) in &level.adj[i] {
            if j <= i {
                continue;
            }
            let (ci, cj) = (assignment[i], assignment[j]);
            if ci == cj {
                self_loops[ci] += w;
            } else {
                let key = if ci < cj { (ci, cj) } else { (cj, ci) };
                *between.entry(key).or_insert(0.0) += w;
            }
        }
    }
    let mut adj = vec![Vec::new(); num_comms];
    for ((a, b), w) in between {
        adj[a].push((b, w));
        adj[b].push((a, w));
    }
    Level { adj, self_loops }
}

/// Renumber labels to a contiguous 0..k range (first-seen order). Returns k.
fn renumber(labels: &mut [usize]) -> usize {
    let mut remap: BTreeMap<usize, usize> = BTreeMap::new();
    for label in labels.iter_mut() {
        let next = remap.len();
        *label = *remap.entry(*label).or_insert(next);
    }
    remap.len()
}

// ---------------------------------------------------------------------------
// Community info (cohesion + top nodes), renumbered by size like the frontend
// ---------------------------------------------------------------------------

fn summarize_communities(
    nodes: &[ApiGraphNode],
    edges: &[ApiGraphEdge],
    index: &BTreeMap<&str, usize>,
    raw_comm: &[usize],
) -> (Vec<CommunityInfo>, Vec<usize>) {
    // Undirected edge presence set (unweighted), as the frontend cohesion uses.
    let mut edge_set: BTreeSet<(usize, usize)> = BTreeSet::new();
    for e in edges {
        if let (Some(&s), Some(&t)) =
            (index.get(e.source.as_str()), index.get(e.target.as_str()))
        {
            if s != t {
                edge_set.insert(if s < t { (s, t) } else { (t, s) });
            }
        }
    }

    // Group node indices by raw community id.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &c) in raw_comm.iter().enumerate() {
        groups.entry(c).or_default().push(i);
    }

    let mut infos: Vec<(usize, CommunityInfo)> = groups
        .into_iter()
        .map(|(raw_id, members)| {
            let n = members.len();
            let mut intra = 0usize;
            for a in 0..members.len() {
                for b in (a + 1)..members.len() {
                    let (i, j) = (members[a], members[b]);
                    let key = if i < j { (i, j) } else { (j, i) };
                    if edge_set.contains(&key) {
                        intra += 1;
                    }
                }
            }
            let possible = if n > 1 { (n * (n - 1)) / 2 } else { 1 };
            let cohesion = intra as f64 / possible as f64;

            let mut sorted = members.clone();
            sorted.sort_by(|&a, &b| nodes[b].link_count.cmp(&nodes[a].link_count));
            let top_nodes = sorted
                .iter()
                .take(5)
                .map(|&i| nodes[i].label.clone())
                .collect();

            (
                raw_id,
                CommunityInfo {
                    id: 0, // assigned after sorting by size
                    node_count: n,
                    cohesion,
                    top_nodes,
                },
            )
        })
        .collect();

    // Largest community first, then renumber 0..k (matches the frontend).
    infos.sort_by(|a, b| b.1.node_count.cmp(&a.1.node_count));
    let mut raw_to_final: BTreeMap<usize, usize> = BTreeMap::new();
    let communities: Vec<CommunityInfo> = infos
        .into_iter()
        .enumerate()
        .map(|(final_id, (raw_id, mut info))| {
            info.id = final_id;
            raw_to_final.insert(raw_id, final_id);
            info
        })
        .collect();

    let node_comm_by_index: Vec<usize> = raw_comm
        .iter()
        .map(|c| *raw_to_final.get(c).unwrap_or(&0))
        .collect();

    (communities, node_comm_by_index)
}

// ---------------------------------------------------------------------------
// Surprising connections (ported from graph-insights.ts findSurprisingConnections)
// ---------------------------------------------------------------------------

fn find_surprising_connections(
    nodes: &[ApiGraphNode],
    edges: &[ApiGraphEdge],
    node_communities: &BTreeMap<String, usize>,
    limit: usize,
) -> Vec<SurprisingConnection> {
    let node_map: BTreeMap<&str, &ApiGraphNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let max_degree = nodes.iter().map(|n| n.link_count).max().unwrap_or(1).max(1);
    let structural: BTreeSet<&str> = STRUCTURAL_IDS.iter().copied().collect();

    // Distant cross-type pairs worth an extra point.
    let distant: BTreeSet<&str> = [
        "source-concept",
        "concept-source",
        "source-synthesis",
        "synthesis-source",
        "query-entity",
        "entity-query",
    ]
    .into_iter()
    .collect();

    let mut scored: Vec<SurprisingConnection> = Vec::new();
    for e in edges {
        let (Some(&source), Some(&target)) = (
            node_map.get(e.source.as_str()),
            node_map.get(e.target.as_str()),
        ) else {
            continue;
        };
        if structural.contains(source.id.as_str()) || structural.contains(target.id.as_str()) {
            continue;
        }

        let mut score = 0i32;
        let mut reasons: Vec<String> = Vec::new();

        // Signal 1: crosses a community boundary (+3).
        let sc = node_communities.get(&source.id);
        let tc = node_communities.get(&target.id);
        if sc != tc {
            score += 3;
            reasons.push("crosses community boundary".to_string());
        }

        // Signal 2: connects different types (+2 if distant, else +1).
        if source.node_type != target.node_type {
            let pair = format!("{}-{}", source.node_type, target.node_type);
            if distant.contains(pair.as_str()) {
                score += 2;
                reasons.push(format!(
                    "connects {} to {}",
                    source.node_type, target.node_type
                ));
            } else {
                score += 1;
                reasons.push("different types".to_string());
            }
        }

        // Signal 3: a peripheral node links to a hub (+2).
        let min_deg = source.link_count.min(target.link_count);
        let max_deg = source.link_count.max(target.link_count);
        if min_deg <= 2 && max_deg as f64 >= max_degree as f64 * 0.5 {
            score += 2;
            reasons.push("peripheral node links to hub".to_string());
        }

        // Signal 4: weak-but-present connection (+1).
        if e.weight < 2.0 && e.weight > 0.0 {
            score += 1;
            reasons.push("weak but present connection".to_string());
        }

        if score >= 3 && !reasons.is_empty() {
            let key = if source.id <= target.id {
                format!("{}:::{}", source.id, target.id)
            } else {
                format!("{}:::{}", target.id, source.id)
            };
            scored.push(SurprisingConnection {
                source: source.id.clone(),
                target: target.id.clone(),
                score,
                reasons,
                key,
            });
        }
    }

    scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.key.cmp(&b.key)));
    scored.truncate(limit);
    scored
}

// ---------------------------------------------------------------------------
// Knowledge gaps (ported from graph-insights.ts detectKnowledgeGaps)
// ---------------------------------------------------------------------------

fn detect_knowledge_gaps(
    nodes: &[ApiGraphNode],
    edges: &[ApiGraphEdge],
    communities: &[CommunityInfo],
    node_communities: &BTreeMap<String, usize>,
    limit: usize,
) -> Vec<KnowledgeGap> {
    let mut gaps: Vec<KnowledgeGap> = Vec::new();
    let node_map: BTreeMap<&str, &ApiGraphNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // 1. Isolated nodes (degree ≤ 1; exclude overview/index/log).
    let isolated: Vec<&ApiGraphNode> = nodes
        .iter()
        .filter(|n| {
            n.link_count <= 1 && n.node_type != "overview" && n.id != "index" && n.id != "log"
        })
        .collect();
    if !isolated.is_empty() {
        let top: Vec<&str> = isolated.iter().take(5).map(|n| n.label.as_str()).collect();
        let more = isolated.len().saturating_sub(5);
        let description = if more > 0 {
            format!("{} and {} more", top.join(", "), more)
        } else {
            top.join(", ")
        };
        gaps.push(KnowledgeGap {
            gap_type: "isolated-node".to_string(),
            title: format!(
                "{} isolated page{}",
                isolated.len(),
                if isolated.len() > 1 { "s" } else { "" }
            ),
            description,
            node_ids: isolated.iter().map(|n| n.id.clone()).collect(),
            suggestion: "These pages have few or no connections. Consider adding [[wikilinks]] to related pages, or research to expand their content.".to_string(),
        });
    }

    // 2. Sparse communities (cohesion < 0.15 with ≥ 3 nodes).
    for comm in communities {
        if comm.cohesion < 0.15 && comm.node_count >= 3 {
            let member_ids: Vec<String> = nodes
                .iter()
                .filter(|n| node_communities.get(&n.id) == Some(&comm.id))
                .map(|n| n.id.clone())
                .collect();
            let lead = comm
                .top_nodes
                .first()
                .cloned()
                .unwrap_or_else(|| format!("Community {}", comm.id));
            gaps.push(KnowledgeGap {
                gap_type: "sparse-community".to_string(),
                title: format!("Sparse cluster: {lead}"),
                description: format!(
                    "{} pages with cohesion {:.2} — internal connections are weak.",
                    comm.node_count, comm.cohesion
                ),
                node_ids: member_ids,
                suggestion: "This knowledge area lacks internal cross-references. Consider adding links between these pages or researching to fill gaps.".to_string(),
            });
        }
    }

    // 3. Bridge nodes (neighbors span ≥ 3 communities).
    let structural: BTreeSet<&str> = STRUCTURAL_IDS.iter().copied().collect();
    let mut neighbor_comms: BTreeMap<&str, BTreeSet<usize>> =
        nodes.iter().map(|n| (n.id.as_str(), BTreeSet::new())).collect();
    for e in edges {
        let (Some(&s), Some(&t)) = (
            node_map.get(e.source.as_str()),
            node_map.get(e.target.as_str()),
        ) else {
            continue;
        };
        if let Some(&tc) = node_communities.get(&t.id) {
            neighbor_comms.get_mut(s.id.as_str()).map(|set| set.insert(tc));
        }
        if let Some(&sc) = node_communities.get(&s.id) {
            neighbor_comms.get_mut(t.id.as_str()).map(|set| set.insert(sc));
        }
    }

    let mut bridges: Vec<&ApiGraphNode> = nodes
        .iter()
        .filter(|n| {
            !structural.contains(n.id.as_str())
                && neighbor_comms.get(n.id.as_str()).map_or(0, |s| s.len()) >= 3
        })
        .collect();
    bridges.sort_by(|a, b| {
        let bc = neighbor_comms.get(b.id.as_str()).map_or(0, |s| s.len());
        let ac = neighbor_comms.get(a.id.as_str()).map_or(0, |s| s.len());
        bc.cmp(&ac).then_with(|| a.id.cmp(&b.id))
    });
    for bridge in bridges.into_iter().take(3) {
        let count = neighbor_comms.get(bridge.id.as_str()).map_or(0, |s| s.len());
        gaps.push(KnowledgeGap {
            gap_type: "bridge-node".to_string(),
            title: format!("Key bridge: {}", bridge.label),
            description: format!(
                "Connects {count} different knowledge clusters. This is a critical junction in your wiki."
            ),
            node_ids: vec![bridge.id.clone()],
            suggestion: "This page bridges multiple knowledge areas. Ensure it's well-maintained — if it's thin, expanding it will strengthen your entire wiki.".to_string(),
        });
    }

    gaps.truncate(limit);
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, node_type: &str, link_count: usize) -> ApiGraphNode {
        ApiGraphNode {
            id: id.to_string(),
            label: id.to_string(),
            node_type: node_type.to_string(),
            path: format!("wiki/{id}.md"),
            link_count,
            community: None,
        }
    }

    fn edge(source: &str, target: &str) -> ApiGraphEdge {
        ApiGraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            weight: 1.0,
        }
    }

    #[test]
    fn louvain_splits_two_triangles_joined_by_one_edge() {
        // Two triangles {a,b,c} and {d,e,f}, bridged by a single c–d edge.
        let nodes = vec![
            node("a", "concept", 2),
            node("b", "concept", 2),
            node("c", "concept", 3),
            node("d", "concept", 3),
            node("e", "concept", 2),
            node("f", "concept", 2),
        ];
        let edges = vec![
            edge("a", "b"),
            edge("b", "c"),
            edge("a", "c"),
            edge("d", "e"),
            edge("e", "f"),
            edge("d", "f"),
            edge("c", "d"),
        ];
        let insights = compute_insights(&nodes, &edges);

        // Two communities, and the two triangles land in different ones.
        assert_eq!(insights.communities.len(), 2);
        let g = &insights.node_communities;
        assert_eq!(g["a"], g["b"]);
        assert_eq!(g["b"], g["c"]);
        assert_eq!(g["d"], g["e"]);
        assert_eq!(g["e"], g["f"]);
        assert_ne!(g["a"], g["d"]);
    }

    #[test]
    fn surprising_flags_cross_community_bridge_edge() {
        let nodes = vec![
            node("a", "concept", 2),
            node("b", "concept", 2),
            node("c", "concept", 3),
            node("d", "entity", 3),
            node("e", "entity", 2),
            node("f", "entity", 2),
        ];
        let edges = vec![
            edge("a", "b"),
            edge("b", "c"),
            edge("a", "c"),
            edge("d", "e"),
            edge("e", "f"),
            edge("d", "f"),
            edge("c", "d"), // cross-community + cross-type bridge
        ];
        let insights = compute_insights(&nodes, &edges);

        let bridge = insights
            .surprising_connections
            .iter()
            .find(|s| s.key == "c:::d")
            .expect("c–d should be surprising");
        // Cross-community (+3), different types (+1), weak edge (+1) ⇒ ≥ 5.
        assert!(bridge.score >= 4);
        assert!(bridge
            .reasons
            .iter()
            .any(|r| r.contains("community")));
    }

    #[test]
    fn gaps_report_isolated_node() {
        let nodes = vec![
            node("a", "concept", 2),
            node("b", "concept", 2),
            node("c", "concept", 2),
            node("lonely", "concept", 0),
        ];
        let edges = vec![edge("a", "b"), edge("b", "c"), edge("a", "c")];
        let insights = compute_insights(&nodes, &edges);

        let isolated = insights
            .knowledge_gaps
            .iter()
            .find(|g| g.gap_type == "isolated-node")
            .expect("lonely node should surface as a gap");
        assert!(isolated.node_ids.contains(&"lonely".to_string()));
    }

    #[test]
    fn empty_graph_yields_empty_insights() {
        let insights = compute_insights(&[], &[]);
        assert!(insights.communities.is_empty());
        assert!(insights.knowledge_gaps.is_empty());
        assert!(insights.surprising_connections.is_empty());
    }
}
