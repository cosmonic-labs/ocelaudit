use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    Individual,
    Entity,
    Vessel,
    Aircraft,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CslEntry {
    pub id: String,
    pub source_list: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub entity_type: EntityType,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub nationalities: Vec<String>,
    #[serde(default)]
    pub programs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tlp {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub entry_id: String,
    pub score: f32,
    pub matched_fields: Vec<String>,
    pub snippet: String,
    pub tlp: Tlp,
}
