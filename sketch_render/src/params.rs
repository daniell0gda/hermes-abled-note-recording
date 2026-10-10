//! The layout choices a model may make. Every value has a safe default.

use serde::Deserialize;

use crate::spec::Kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Right,
    Down,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// Layout parameters. `direction` None means the kind's usual direction.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default)]
pub struct Params {
    pub direction: Option<Direction>,
    pub node_gap: f64,
    pub min_channel: f64,
    pub track_gap: f64,
    pub uniform_rank_size: bool,
    pub ordering_sweeps: usize,
    pub theme: Theme,
    pub legend: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            direction: None,
            node_gap: 40.0,
            min_channel: 64.0,
            track_gap: 12.0,
            uniform_rank_size: true,
            ordering_sweeps: 12,
            theme: Theme::Light,
            legend: true,
        }
    }
}

impl Params {
    pub fn direction_for(&self, kind: Kind) -> Direction {
        self.direction.unwrap_or(match kind {
            Kind::Tree | Kind::Layers => Direction::Down,
            _ => Direction::Right,
        })
    }
}
