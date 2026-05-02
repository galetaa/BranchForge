use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphInputCommit {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    pub author: String,
    pub time: String,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRef {
    pub oid: String,
    pub label: GraphRefLabel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRefLabel {
    pub name: String,
    pub kind: GraphRefKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCommit {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    pub author: String,
    pub time: String,
    pub parents: Vec<String>,
    pub refs: Vec<GraphRefLabel>,
    pub lane: usize,
    pub row: usize,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from_oid: String,
    pub to_oid: String,
    pub from_lane: usize,
    pub to_lane: usize,
    pub edge_kind: GraphEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEdgeKind {
    Parent,
    Merge,
}

pub fn build_graph(commits: &[GraphInputCommit], refs: &[GraphRef]) -> Vec<GraphCommit> {
    let refs_by_oid = refs.iter().fold(
        HashMap::<String, Vec<GraphRefLabel>>::new(),
        |mut labels, graph_ref| {
            labels
                .entry(graph_ref.oid.clone())
                .or_default()
                .push(graph_ref.label.clone());
            labels
        },
    );

    let mut active_lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for (row, commit) in commits.iter().enumerate() {
        let lane = ensure_lane_for_oid(&mut active_lanes, &commit.oid);
        active_lanes[lane] = None;

        let mut edges = Vec::with_capacity(commit.parents.len());
        if let Some(first_parent) = commit.parents.first() {
            active_lanes[lane] = Some(first_parent.clone());
            edges.push(GraphEdge {
                from_oid: commit.oid.clone(),
                to_oid: first_parent.clone(),
                from_lane: lane,
                to_lane: lane,
                edge_kind: GraphEdgeKind::Parent,
            });
        }

        for parent in commit.parents.iter().skip(1) {
            let parent_lane = ensure_lane_for_oid(&mut active_lanes, parent);
            active_lanes[parent_lane] = Some(parent.clone());
            edges.push(GraphEdge {
                from_oid: commit.oid.clone(),
                to_oid: parent.clone(),
                from_lane: lane,
                to_lane: parent_lane,
                edge_kind: GraphEdgeKind::Merge,
            });
        }

        rows.push(GraphCommit {
            oid: commit.oid.clone(),
            short_oid: commit.short_oid.clone(),
            summary: commit.summary.clone(),
            author: commit.author.clone(),
            time: commit.time.clone(),
            parents: commit.parents.clone(),
            refs: refs_by_oid.get(&commit.oid).cloned().unwrap_or_default(),
            lane,
            row,
            edges,
        });
    }

    rows
}

fn ensure_lane_for_oid(active_lanes: &mut Vec<Option<String>>, oid: &str) -> usize {
    if let Some(index) = active_lanes
        .iter()
        .position(|active_oid| active_oid.as_deref() == Some(oid))
    {
        return index;
    }

    if let Some(index) = active_lanes.iter().position(Option::is_none) {
        active_lanes[index] = Some(oid.to_string());
        return index;
    }

    active_lanes.push(Some(oid.to_string()));
    active_lanes.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(oid: &str, parents: &[&str]) -> GraphInputCommit {
        GraphInputCommit {
            oid: oid.to_string(),
            short_oid: oid.chars().take(8).collect(),
            summary: format!("commit {oid}"),
            author: "Dev".to_string(),
            time: "now".to_string(),
            parents: parents.iter().map(|parent| parent.to_string()).collect(),
        }
    }

    #[test]
    fn linear_history_stays_on_one_lane() {
        let graph = build_graph(
            &[
                commit("c3", &["c2"]),
                commit("c2", &["c1"]),
                commit("c1", &[]),
            ],
            &[],
        );

        assert_eq!(
            graph.iter().map(|row| row.lane).collect::<Vec<_>>(),
            [0, 0, 0]
        );
        assert_eq!(graph[0].edges[0].to_oid, "c2");
        assert_eq!(graph[0].edges[0].edge_kind, GraphEdgeKind::Parent);
    }

    #[test]
    fn branch_history_uses_deterministic_free_lane() {
        let graph = build_graph(
            &[
                commit("feature-tip", &["feature-base"]),
                commit("main-tip", &["root"]),
                commit("feature-base", &["root"]),
                commit("root", &[]),
            ],
            &[],
        );

        assert_eq!(graph[0].lane, 0);
        assert_eq!(graph[1].lane, 1);
        assert_eq!(graph[2].lane, 0);
        assert_eq!(graph[3].lane, 0);
    }

    #[test]
    fn merge_commit_allocates_parent_lanes_and_keeps_ref_labels() {
        let graph = build_graph(
            &[
                commit("merge", &["main", "topic"]),
                commit("main", &["root"]),
                commit("topic", &["root"]),
                commit("root", &[]),
            ],
            &[GraphRef {
                oid: "merge".to_string(),
                label: GraphRefLabel {
                    name: "main".to_string(),
                    kind: GraphRefKind::LocalBranch,
                },
            }],
        );

        assert_eq!(graph[0].lane, 0);
        assert_eq!(graph[0].edges.len(), 2);
        assert_eq!(graph[0].edges[1].to_lane, 1);
        assert_eq!(graph[0].edges[1].edge_kind, GraphEdgeKind::Merge);
        assert_eq!(graph[0].refs[0].name, "main");
    }
}
