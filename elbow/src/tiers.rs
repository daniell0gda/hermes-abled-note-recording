//! Layer stacks: every group is one full-width layer, listed from top to bottom, and every node sits in
//! the first layer that names it. Nodes no layer names share one unnamed layer at the bottom.

use crate::geom::{snap_up, Rect};
use crate::node::NAME_SIZE;
use crate::scene::Lane;
use crate::spec::{GroupSpec, View};
use crate::text::sans_width;

pub const LAYER_PADDING: f64 = 24.0;
const GUTTER_INSET: f64 = 16.0;
const MIN_GUTTER: f64 = 96.0;

/// The layer ids and names, top first, and the layer (tier) of every node.
pub struct TierPlan {
    pub ids: Vec<String>,
    pub labels: Vec<String>,
    pub of_node: Vec<usize>,
}

impl TierPlan {
    /// The width of the column left of the stack that holds the layer names.
    pub fn gutter(&self) -> f64 {
        let widest = self.labels.iter().map(|label| sans_width(label, NAME_SIZE)).fold(0.0, f64::max);
        snap_up(MIN_GUTTER.max(widest + 2.0 * GUTTER_INSET))
    }

    /// The layer bands around the placed nodes: each boundary lies halfway between two layers.
    pub fn bands(&self, rects: &[Rect]) -> Vec<Lane> {
        let spans: Vec<(f64, f64)> = (0..self.labels.len())
            .map(|tier| {
                let members = rects.iter().zip(&self.of_node).filter(|(_, &own)| own == tier).map(|(rect, _)| rect);
                members.fold((f64::INFINITY, f64::NEG_INFINITY), |(top, bottom), rect| (top.min(rect.y), bottom.max(rect.bottom())))
            })
            .collect();
        let mut lanes: Vec<Lane> = Vec::with_capacity(spans.len());
        for (tier, &(top, bottom)) in spans.iter().enumerate() {
            let start = match tier {
                0 => top - LAYER_PADDING,
                _ => (spans[tier - 1].1 + top) / 2.0,
            };
            let end = spans.get(tier + 1).map_or(bottom + LAYER_PADDING, |next| (bottom + next.0) / 2.0);
            lanes.push(Lane { id: self.ids[tier].clone(), label: self.labels[tier].clone(), top: start, bottom: end });
        }
        lanes
    }
}

/// The tier plan for a layers view with groups; None when no group holds a known node.
pub fn plan(view: &View, ids: &[String]) -> Option<TierPlan> {
    let layers: Vec<(&GroupSpec, Vec<&String>)> = view
        .groups
        .iter()
        .map(|group| (group, group.members.iter().filter(|member| ids.contains(member)).collect::<Vec<&String>>()))
        .filter(|(_, members)| !members.is_empty())
        .collect();
    if layers.is_empty() {
        return None;
    }
    let mut layer_ids: Vec<String> = layers.iter().map(|(group, _)| group.id.clone()).collect();
    let mut labels: Vec<String> = layers
        .iter()
        .map(|(group, _)| if group.label.is_empty() { group.id.clone() } else { group.label.clone() })
        .collect();
    let outside = labels.len();
    let of_node: Vec<usize> = ids
        .iter()
        .map(|id| layers.iter().position(|(_, members)| members.contains(&id)).unwrap_or(outside))
        .collect();
    if of_node.contains(&outside) {
        labels.push(String::new());
        layer_ids.push(String::new());
    }
    Some(TierPlan { ids: layer_ids, labels, of_node })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_keep_their_order_and_strangers_go_to_the_bottom() {
        let view = View {
            groups: vec![
                GroupSpec { id: "ui".into(), label: "UI".into(), members: vec!["web".into()] },
                GroupSpec { id: "empty".into(), label: "Empty".into(), members: vec!["ghost".into()] },
                GroupSpec { id: "data".into(), label: "Data".into(), members: vec!["db".into()] },
            ],
            ..View::default()
        };
        let ids: Vec<String> = ["db", "web", "loner"].iter().map(|id| id.to_string()).collect();
        let plan = plan(&view, &ids).expect("a plan");
        assert_eq!(plan.labels, vec!["UI".to_string(), "Data".to_string(), String::new()]);
        assert_eq!(plan.ids, vec!["ui".to_string(), "data".to_string(), String::new()]);
        assert_eq!(plan.of_node, vec![1, 0, 2]);
    }

    #[test]
    fn a_band_boundary_lies_halfway_between_two_layers() {
        let plan = TierPlan { ids: vec!["top".into(), "bottom".into()], labels: vec!["Top".into(), "Bottom".into()], of_node: vec![0, 1] };
        let bands = plan.bands(&[Rect::new(0.0, 0.0, 80.0, 40.0), Rect::new(0.0, 100.0, 80.0, 40.0)]);
        assert_eq!((bands[0].top, bands[0].bottom), (-LAYER_PADDING, 70.0));
        assert_eq!((bands[1].top, bands[1].bottom), (70.0, 140.0 + LAYER_PADDING));
    }
}
