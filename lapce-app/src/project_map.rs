use floem::{
    IntoView, View,
    reactive::{create_rw_signal, SignalGet, SignalUpdate},
    views::{Decorators, container, label, stack, scroll},
    peniko::Color,
    kurbo::{Point, Circle, Line},
    event::{EventListener, EventPropagation},
};
use crate::map_client::{MapResponse, MapNode, MapEdge, MapClient, MapRequest};
use anyhow::Result;
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
    pub fn new(workspace_id: String, base_url: String, token: Option<String>) -> Self {
        Self {
            response: None,
            graph: None,
            node_positions: HashMap::new(),
            selected_node: None,
            hover_node: None,
            offset: Point::ZERO,
            zoom: 1.0,
            client: MapClient::new(base_url, token),
            workspace_id,
        }
    }

    pub fn fetch_map(&mut self, focus_path: Option<String>, focus_symbol: Option<String>) -> Result<()> {
        let req = MapRequest {
            workspace_id: self.workspace_id.clone(),
            focus_path,
            focus_symbol,
            depth: 1,
        };
        let resp = self.client.get_map(req)?;
        self.build_graph(&resp);
        self.response = Some(resp);
        Ok(())
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

        // Hierarchical layout based on file paths and node types
        self.node_positions.clear();
        self.create_hierarchical_layout(&resp.nodes);

        self.graph = Some(g);
    }

    fn create_hierarchical_layout(&mut self, nodes: &[MapNode]) {
        // Group nodes by type with architecture layers first
        let mut architecture_layer_nodes = Vec::new();
        let mut component_nodes = Vec::new();
        let mut service_nodes = Vec::new();
        let mut file_nodes = Vec::new();
        let mut function_nodes = Vec::new();
        let mut class_nodes = Vec::new();
        let mut other_nodes = Vec::new();

        for node in nodes {
            match node.kind.as_str() {
                "architecture_layer" => architecture_layer_nodes.push(node),
                "component" => component_nodes.push(node),
                "service" => service_nodes.push(node),
                "file" => file_nodes.push(node),
                "function" => function_nodes.push(node),
                "class" => class_nodes.push(node),
                _ => other_nodes.push(node),
            }
        }

        // Layout parameters
        let start_x = 50.0;
        let start_y = 50.0;
        let component_spacing = 120.0;  // More space for components
        let vertical_spacing = 70.0;
        let horizontal_spacing = 300.0;
        let indent_per_level = 180.0;

        let mut current_y = start_y;

        // Layout architecture layers first (highest level)
        if !architecture_layer_nodes.is_empty() {
            // Use a 2x2 grid layout for architecture layers
            let layers_per_row = 2;
            for (i, node) in architecture_layer_nodes.iter().enumerate() {
                let row = i / layers_per_row;
                let col = i % layers_per_row;
                let x = start_x + (col as f64 * horizontal_spacing * 1.2);
                let y = current_y + (row as f64 * component_spacing * 1.2);
                self.node_positions.insert(node.id.clone(), Point::new(x, y));
            }
            
            let rows = (architecture_layer_nodes.len() + layers_per_row - 1) / layers_per_row;
            current_y += (rows as f64 * component_spacing * 1.2) + 80.0;
        }

        // Layout architecture components (second level)
        if !component_nodes.is_empty() {
            // Use a grid layout for components for better visibility
            let components_per_row = 3;
            for (i, node) in component_nodes.iter().enumerate() {
                let row = i / components_per_row;
                let col = i % components_per_row;
                let x = start_x + (col as f64 * horizontal_spacing);
                let y = current_y + (row as f64 * component_spacing);
                self.node_positions.insert(node.id.clone(), Point::new(x, y));
            }
            
            let rows = (component_nodes.len() + components_per_row - 1) / components_per_row;
            current_y += (rows as f64 * component_spacing) + 60.0;
        }

        // Layout services (if any)
        for (i, node) in service_nodes.iter().enumerate() {
            let x = start_x;
            let y = current_y + (i as f64 * vertical_spacing);
            self.node_positions.insert(node.id.clone(), Point::new(x, y));
        }

        if !service_nodes.is_empty() {
            current_y += (service_nodes.len() as f64 * vertical_spacing) + 40.0;
        }

        // Layout files (indented from components/services)
        for (i, node) in file_nodes.iter().enumerate() {
            let x = start_x + indent_per_level * 0.5;
            let y = current_y + (i as f64 * vertical_spacing);
            self.node_positions.insert(node.id.clone(), Point::new(x, y));
        }

        if !file_nodes.is_empty() {
            current_y += (file_nodes.len() as f64 * vertical_spacing) + 40.0;
        }

        // Layout classes (more indented)
        for (i, node) in class_nodes.iter().enumerate() {
            let x = start_x + indent_per_level;
            let y = current_y + (i as f64 * vertical_spacing);
            self.node_positions.insert(node.id.clone(), Point::new(x, y));
        }

        if !class_nodes.is_empty() {
            current_y += (class_nodes.len() as f64 * vertical_spacing) + 40.0;
        }

        // Layout functions (most indented)
        for (i, node) in function_nodes.iter().enumerate() {
            let x = start_x + (indent_per_level * 1.5);
            let y = current_y + (i as f64 * vertical_spacing);
            self.node_positions.insert(node.id.clone(), Point::new(x, y));
        }

        if !function_nodes.is_empty() {
            current_y += (function_nodes.len() as f64 * vertical_spacing) + 40.0;
        }

        // Layout other nodes
        for (i, node) in other_nodes.iter().enumerate() {
            let x = start_x + (indent_per_level * 2.0);
            let y = current_y + (i as f64 * vertical_spacing);
            self.node_positions.insert(node.id.clone(), Point::new(x, y));
        }
    }
}
