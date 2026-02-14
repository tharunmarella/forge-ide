use floem::{
    IntoView, View,
    reactive::{create_rw_signal, SignalGet, SignalUpdate},
    views::{Decorators, container, label, stack, scroll},
    peniko::Color,
    kurbo::{Point, Circle, Line},
    event::{EventListener, EventPropagation},
};
use crate::map_client::{MapResponse, MapNode, MapEdge, MapClient, MapRequest};
use std::collections::HashMap;
use petgraph::graph::{NodeIndex, UnGraph};

pub struct ProjectMapData {
    pub response: Option<MapResponse>,
    pub graph: Option<UnGraph<String, String>>,
    pub node_positions: HashMap<String, Point>,
    pub selected_node: Option<String>,
    pub hover_node: Option<String>,
    pub offset: Point,
    pub zoom: f64,
    pub client: MapClient,
    pub workspace_id: String,
}

impl ProjectMapData {
    pub fn new(workspace_id: String, base_url: String) -> Self {
        Self {
            response: None,
            graph: None,
            node_positions: HashMap::new(),
            selected_node: None,
            hover_node: None,
            offset: Point::ZERO,
            zoom: 1.0,
            client: MapClient::new(base_url),
            workspace_id,
        }
    }

    pub fn fetch_map(&mut self, focus_path: Option<String>, focus_symbol: Option<String>) {
        let req = MapRequest {
            workspace_id: self.workspace_id.clone(),
            focus_path,
            focus_symbol,
            depth: 1,
        };
        if let Ok(resp) = self.client.get_map(req) {
            self.build_graph(&resp);
            self.response = Some(resp);
        }
    }

    fn build_graph(&mut self, resp: &MapResponse) {
        let mut g = UnGraph::<String, String>::new_undirected();
        let mut indices = HashMap::new();

        for node in &resp.nodes {
            let idx = g.add_node(node.name.clone());
            indices.insert(node.id.clone(), idx);
        }

        for edge in &resp.edges {
            if let (Some(&from), Some(&to)) = (indices.get(&edge.from_id), indices.get(&edge.to_id)) {
                g.add_edge(from, to, edge.r#type.clone());
            }
        }

        // Simple layout: circular arrangement
        self.node_positions.clear();
        let n = resp.nodes.len();
        if n > 0 {
            let center = Point::new(400.0, 300.0);
            let radius = 200.0;
            for (i, node) in resp.nodes.iter().enumerate() {
                let angle = (i as f64 / n as f64) * 2.0 * std::f64::consts::PI;
                let pos = Point::new(
                    center.x + angle.cos() * radius,
                    center.y + angle.sin() * radius,
                );
                self.node_positions.insert(node.id.clone(), pos);
            }
        }

        self.graph = Some(g);
    }
}
