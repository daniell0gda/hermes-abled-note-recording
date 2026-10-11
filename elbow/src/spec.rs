//! The diagram to draw: the live sketch's `Diagram.view()` JSON.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Flow,
    Sequence,
    State,
    Tree,
    Er,
    Swimlane,
    Nested,
    Layers,
    Dependency,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Sequence => "sequence",
            Kind::State => "state",
            Kind::Tree => "tree",
            Kind::Er => "er",
            Kind::Swimlane => "swimlane",
            Kind::Nested => "nested",
            Kind::Layers => "layers",
            Kind::Dependency => "dependency",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Service,
    Database,
    Queue,
    Cache,
    Actor,
    External,
    Client,
    State,
    Decision,
    Note,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeStyle {
    #[default]
    Sync,
    Async,
    Data,
    Transition,
    Dependency,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub focal: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EdgeSpec {
    #[serde(default)]
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub style: EdgeStyle,
    #[serde(default)]
    pub focal: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GroupSpec {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub members: Vec<String>,
}

/// One diagram: what the speaker described, with the shapes and edge styles already chosen.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct View {
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub nodes: Vec<NodeSpec>,
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub groups: Vec<GroupSpec>,
    #[serde(default)]
    pub selected: Vec<String>,
    #[serde(default)]
    pub pointed: Option<String>,
}
