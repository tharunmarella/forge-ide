use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapRequest {
    pub workspace_id: String,
    pub focus_path: Option<String>,
    pub focus_symbol: Option<String>,
    pub depth: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_path: Option<String>,
    pub signature: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapEdge {
    #[serde(rename = "from")]
    pub from_id: String,
    #[serde(rename = "to")]
    pub to_id: String,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MapResponse {
    pub workspace_id: String,
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
    pub focus_path: Option<String>,
    pub focus_symbol: Option<String>,
}

pub struct MapClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl MapClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn get_map(&self, req: MapRequest) -> Result<MapResponse> {
        let url = format!("{}/map", self.base_url);
        let resp = self.client.post(url)
            .json(&req)
            .send()?
            .json::<MapResponse>()?;
        Ok(resp)
    }
}
