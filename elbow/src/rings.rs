//! Nested containment: groups whose member sets hold each other become rings inside rings. Every node
//! sits in the lane of the innermost group that holds it, and each group's lanes stay next to each other,
//! so a ring can enclose its nodes without touching anything outside it.

use crate::geom::{Rect, Segment};
use crate::spec::View;
use crate::text::mono_width;

pub const RING_INSET: f64 = 20.0;
pub const RING_LABEL_SIZE: f64 = 8.0;
pub const RING_LABEL_TRACKING: f64 = 0.14;
pub const RING_LABEL_PADDING: f64 = 4.0;
const RING_LABEL_HEIGHT: f64 = 16.0;
const RING_LABEL_INSET: f64 = 12.0;
const RING_LABEL_STEP: f64 = 16.0;
const LANE_ROOM: f64 = 12.0;
const LINE_CLEARANCE: f64 = 2.0;

/// A drawn ring: its label, its box, the mask its label sits on (over the top border) and how deep it
/// sits (0 is outermost).
pub struct Ring {
    pub id: String,
    pub label: String,
    pub rect: Rect,
    pub label_rect: Rect,
    pub depth: usize,
}

impl Ring {
    /// Slides the label along the top border to the first spot no connector and no node runs through.
    pub fn clear_label(&mut self, lines: &[Segment], nodes: &[Rect]) {
        let width = self.label_rect.w;
        let last = self.rect.right() - RING_LABEL_INSET - width;
        let spots = (0..).map(|step| self.rect.x + RING_LABEL_INSET + step as f64 * RING_LABEL_STEP).take_while(|&x| x <= last);
        let free = |x: f64| {
            let mask = Rect::new(x, self.label_rect.y, width, RING_LABEL_HEIGHT);
            lines.iter().all(|line| mask.distance_to(line) >= LINE_CLEARANCE) && nodes.iter().all(|node| !mask.overlaps(node))
        };
        if let Some(x) = spots.into_iter().find(|&x| free(x)) {
            self.label_rect.x = x;
        }
    }
}

struct Group {
    id: String,
    label: String,
    members: Vec<usize>,
    parent: Option<usize>,
}

struct RingSpec {
    id: String,
    label: String,
    nodes: Vec<usize>,
    depth: usize,
    inset: f64,
}

/// Which lane each node goes in, the room every lane keeps free for ring borders, and the rings to draw.
pub struct RingPlan {
    pub of_node: Vec<usize>,
    pub lane_room: Vec<(f64, f64)>,
    rings: Vec<RingSpec>,
}

impl RingPlan {
    /// The rings around the placed nodes, outermost first.
    pub fn rings(&self, rects: &[Rect]) -> Vec<Ring> {
        self.rings
            .iter()
            .filter_map(|spec| {
                let bounds = spec.nodes.iter().map(|&node| rects[node]).reduce(|a, b| {
                    let x = a.x.min(b.x);
                    let y = a.y.min(b.y);
                    Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
                })?;
                let rect = bounds.inflate(spec.inset);
                let width = mono_width(&spec.label, RING_LABEL_SIZE, RING_LABEL_TRACKING) + 2.0 * RING_LABEL_PADDING;
                let label_rect = Rect::new(rect.x + RING_LABEL_INSET, rect.y - RING_LABEL_HEIGHT / 2.0, width, RING_LABEL_HEIGHT);
                Some(Ring { id: spec.id.clone(), label: spec.label.clone(), rect, label_rect, depth: spec.depth })
            })
            .collect()
    }
}

/// The ring plan for a nested view with groups; None when no group holds a known node.
pub fn plan(view: &View, ids: &[String]) -> Option<RingPlan> {
    let groups = containment(view, ids);
    if groups.is_empty() {
        return None;
    }
    let home = homes(&groups, ids.len());
    let children = children(&groups);
    let roots: Vec<usize> = (0..groups.len()).filter(|&group| groups[group].parent.is_none()).collect();
    let outsiders: Vec<usize> = (0..ids.len()).filter(|&node| home[node].is_none()).collect();
    let mut lanes = Vec::new();
    if !outsiders.is_empty() {
        lanes.push(LaneSpan { nodes: outsiders, opens: 0.0, closes: 0.0 });
    }
    let mut walk = Walk { groups: &groups, children: &children, home: &home, lanes, rings: Vec::new() };
    for &root in &roots {
        walk.visit(root, 0);
    }
    let Walk { lanes, rings, .. } = walk;
    let mut of_node = vec![0; ids.len()];
    for (lane, span) in lanes.iter().enumerate() {
        for &node in &span.nodes {
            of_node[node] = lane;
        }
    }
    let lane_room = lanes.iter().map(|span| (LANE_ROOM + span.opens, LANE_ROOM + span.closes)).collect();
    Some(RingPlan { of_node, lane_room, rings })
}

struct LaneSpan {
    nodes: Vec<usize>,
    opens: f64,
    closes: f64,
}

struct Walk<'a> {
    groups: &'a [Group],
    children: &'a [Vec<usize>],
    home: &'a [Option<usize>],
    lanes: Vec<LaneSpan>,
    rings: Vec<RingSpec>,
}

impl Walk<'_> {
    /// Adds the lanes of `group` and its children, and returns the ring levels at and below it.
    fn visit(&mut self, group: usize, depth: usize) -> usize {
        let first_lane = self.lanes.len();
        let ring = self.rings.len();
        let spec = &self.groups[group];
        self.rings.push(RingSpec { id: spec.id.clone(), label: spec.label.clone(), nodes: Vec::new(), depth, inset: 0.0 });
        let own: Vec<usize> = (0..self.home.len()).filter(|&node| self.home[node] == Some(group)).collect();
        if !own.is_empty() {
            self.lanes.push(LaneSpan { nodes: own, opens: 0.0, closes: 0.0 });
        }
        let mut levels = 0;
        for &child in self.children[group].iter() {
            levels = levels.max(self.visit(child, depth + 1));
        }
        let held: Vec<usize> = self.lanes[first_lane..].iter().flat_map(|span| span.nodes.iter().copied()).collect();
        if held.is_empty() {
            self.rings.remove(ring);
            return levels;
        }
        let inset = (levels + 1) as f64 * RING_INSET;
        self.rings[ring].nodes = held;
        self.rings[ring].inset = inset;
        self.lanes[first_lane].opens = self.lanes[first_lane].opens.max(inset);
        let last = self.lanes.len() - 1;
        self.lanes[last].closes = self.lanes[last].closes.max(inset);
        levels + 1
    }
}

fn containment(view: &View, ids: &[String]) -> Vec<Group> {
    let mut groups: Vec<Group> = view
        .groups
        .iter()
        .map(|group| {
            let members: Vec<usize> = ids.iter().enumerate().filter(|(_, id)| group.members.contains(id)).map(|(node, _)| node).collect();
            let label = if group.label.is_empty() { group.id.clone() } else { group.label.clone() };
            Group { id: group.id.clone(), label: label.to_uppercase(), members, parent: None }
        })
        .filter(|group| !group.members.is_empty())
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.members.len()));
    for inner in 0..groups.len() {
        groups[inner].parent = (0..inner)
            .rev()
            .find(|&outer| groups[inner].members.iter().all(|node| groups[outer].members.contains(node)));
    }
    groups
}

fn homes(groups: &[Group], count: usize) -> Vec<Option<usize>> {
    (0..count).map(|node| (0..groups.len()).rev().find(|&group| groups[group].members.contains(&node))).collect()
}

fn children(groups: &[Group]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); groups.len()];
    for (group, spec) in groups.iter().enumerate() {
        if let Some(parent) = spec.parent {
            children[parent].push(group);
        }
    }
    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;
    use crate::spec::GroupSpec;

    fn view(groups: &[(&str, &[&str])]) -> View {
        View {
            groups: groups
                .iter()
                .map(|(id, members)| GroupSpec { id: id.to_string(), label: String::new(), members: members.iter().map(|m| m.to_string()).collect() })
                .collect(),
            ..View::default()
        }
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn an_inner_group_gets_its_own_lane_and_a_ring_inside_the_outer_one() {
        let nodes = ids(&["a", "b", "c", "d"]);
        let plan = plan(&view(&[("inner", &["c", "d"]), ("outer", &["a", "b", "c", "d"])]), &nodes).expect("a plan");
        assert_eq!(plan.of_node, vec![0, 0, 1, 1]);
        let rects: Vec<Rect> = (0..4).map(|node| Rect::new(0.0, node as f64 * 100.0, 80.0, 40.0)).collect();
        let rings = plan.rings(&rects);
        assert_eq!(rings.len(), 2);
        assert_eq!((rings[0].label.as_str(), rings[0].depth), ("OUTER", 0));
        assert_eq!((rings[1].label.as_str(), rings[1].depth), ("INNER", 1));
        assert!(rings[0].rect.x < rings[1].rect.x && rings[0].rect.bottom() > rings[1].rect.bottom());
    }

    #[test]
    fn the_room_before_a_lane_fits_every_ring_that_opens_there() {
        let nodes = ids(&["a", "b"]);
        let plan = plan(&view(&[("outer", &["a", "b"]), ("inner", &["a", "b"])]), &nodes).expect("a plan");
        assert_eq!(plan.lane_room, vec![(LANE_ROOM + 2.0 * RING_INSET, LANE_ROOM + 2.0 * RING_INSET)]);
    }

    #[test]
    fn a_ring_label_moves_off_a_connector_that_crosses_its_border() {
        let nodes = ids(&["a"]);
        let plan = plan(&view(&[("box", &["a"])]), &nodes).expect("a plan");
        let mut ring = plan.rings(&[Rect::new(0.0, 0.0, 200.0, 40.0)]).remove(0);
        let start = ring.label_rect.x;
        let line = Segment::new(Point::new(start + 8.0, -100.0), Point::new(start + 8.0, 100.0));
        ring.clear_label(&[line], &[]);
        assert!(ring.label_rect.x > start + 8.0 + LINE_CLEARANCE);
    }

    #[test]
    fn nodes_outside_every_group_share_the_first_lane() {
        let nodes = ids(&["a", "b", "c"]);
        let plan = plan(&view(&[("only", &["b"])]), &nodes).expect("a plan");
        assert_eq!(plan.of_node, vec![0, 1, 0]);
    }
}
