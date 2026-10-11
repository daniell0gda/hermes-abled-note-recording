//! Sequence layout: participants in a row with lifelines, one message per row in spoken order.
//! A message label sits in the gap next to its sender, and each gap is wide enough for its labels.
//! An activation bar covers the rows in which a participant holds control: a call to an idle participant
//! opens its bar and the caller's, a self-message opens the sender's, a message back to the caller closes it, and the caller's bar closes with
//! it when the caller only started the exchange. A bar still open at the end closes at its last message.

use crate::figure::{Figure, Paint, Tone};
use crate::geom::{snap_up, Point, Rect, Segment};
use crate::label::{LABEL_GAP, MASK_HEIGHT};
use crate::scene::{known_edges, mask_width, unique_nodes, EdgeInput, LabelBox, Scene, SceneEdge, SceneNode, MARGIN};
use crate::spec::View;

const MIN_GAP: f64 = 48.0;
const LABEL_ROOM: f64 = 24.0;
const FIRST_ROW: f64 = 40.0;
const ROW: f64 = 40.0;
const LOOP_WIDTH: f64 = 28.0;
const LOOP_DROP: f64 = 16.0;
const TAIL: f64 = 24.0;
const BAR_HALF: f64 = 4.0;
const BAR_TAIL: f64 = 8.0;

/// Lays out `view` as a sequence diagram.
pub fn build(view: &View) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let messages = known_edges(view, &ids);
    let widths: Vec<f64> = contents.iter().map(|content| content.width).collect();
    let gaps = gap_widths(&widths, &messages);
    let height = contents.iter().map(|content| content.height).fold(0.0, f64::max);
    let mut centres = Vec::with_capacity(widths.len());
    let mut x = MARGIN;
    for (node, width) in widths.iter().enumerate() {
        if node > 0 {
            x += gaps[node - 1];
        }
        centres.push(x + width / 2.0);
        x += width;
    }
    let first_row = MARGIN + height + FIRST_ROW;
    let row_y = |row: usize| first_row + row as f64 * ROW;
    let bars = activations(&messages);
    let inset = |node: usize, row: usize| if bars.iter().any(|bar| bar.covers(node, row)) { BAR_HALF } else { 0.0 };
    let edges: Vec<SceneEdge> = messages
        .iter()
        .enumerate()
        .map(|(row, message)| draw_message(message, row_y(row), &centres, (inset(message.source, row), inset(message.target, row))))
        .collect();
    let decor = bars.iter().flat_map(|bar| bar.figures(&messages, &centres, row_y)).collect();
    let bottom = row_y(messages.len().saturating_sub(1)) + LOOP_DROP + TAIL;
    let lifelines = centres.iter().map(|&centre| Segment::new(Point::new(centre, MARGIN + height), Point::new(centre, bottom))).collect();
    let nodes: Vec<SceneNode> = ids
        .into_iter()
        .zip(contents)
        .zip(&centres)
        .map(|((id, content), &centre)| {
            let rect = Rect::new(centre - content.width / 2.0, MARGIN, content.width, height);
            SceneNode::new(id, rect, content)
        })
        .collect();
    let right = x.max(edges.iter().filter_map(|edge| edge.label.as_ref()).map(|label| label.rect.right()).fold(0.0, f64::max));
    Scene { nodes, edges, lifelines, decor, width: snap_up(right + MARGIN), height: snap_up(bottom + MARGIN), ..Scene::empty(view.kind) }
}

struct Activation {
    node: usize,
    first: usize,
    last: usize,
}

impl Activation {
    fn covers(&self, node: usize, row: usize) -> bool {
        self.node == node && (self.first..=self.last).contains(&row)
    }

    fn figures(&self, messages: &[EdgeInput], centres: &[f64], row_y: impl Fn(usize) -> f64) -> [Figure; 2] {
        let last = &messages[self.last];
        let ends_in_loop = last.source == self.node && last.target == self.node;
        let top = row_y(self.first);
        let bottom = row_y(self.last) + BAR_TAIL + if ends_in_loop { LOOP_DROP } else { 0.0 };
        let rect = Rect::new(centres[self.node] - BAR_HALF, top, 2.0 * BAR_HALF, bottom - top);
        [Figure::rect(rect, 0.0, Paint::filled(Tone::Paper)), Figure::rect(rect, 0.0, Paint::shape(Tone::Wash(0.06), Tone::Muted, 0.8))]
    }
}

struct OpenBar {
    node: usize,
    first: usize,
    last: usize,
    caller: Option<usize>,
}

#[derive(Default)]
struct Bars {
    open: Vec<OpenBar>,
    closed: Vec<Activation>,
}

fn activations(messages: &[EdgeInput]) -> Vec<Activation> {
    let mut bars = Bars::default();
    for (row, message) in messages.iter().enumerate() {
        bars.add(row, message.source, message.target);
    }
    bars.finished()
}

impl Bars {
    fn add(&mut self, row: usize, source: usize, target: usize) {
        for bar in self.open.iter_mut().filter(|bar| bar.node == source || bar.node == target) {
            bar.last = row;
        }
        if source != target && self.close_reply(source, target) {
            return;
        }
        self.open_if_idle(source, None, row);
        if source == target {
            return;
        }
        self.open_if_idle(target, Some(source), row);
    }

    fn close_reply(&mut self, source: usize, target: usize) -> bool {
        let Some(reply) = self.open.iter().position(|bar| bar.node == source && bar.caller == Some(target)) else {
            return false;
        };
        self.close(reply);
        let waiting = self.open.iter().any(|bar| bar.caller == Some(target));
        if let Some(starter) = self.open.iter().position(|bar| bar.node == target && bar.caller.is_none()).filter(|_| !waiting) {
            self.close(starter);
        }
        true
    }

    fn open_if_idle(&mut self, node: usize, caller: Option<usize>, row: usize) {
        if !self.open.iter().any(|bar| bar.node == node) {
            self.open.push(OpenBar { node, first: row, last: row, caller });
        }
    }

    fn close(&mut self, index: usize) {
        let bar = self.open.remove(index);
        self.closed.push(Activation { node: bar.node, first: bar.first, last: bar.last });
    }

    fn finished(mut self) -> Vec<Activation> {
        while !self.open.is_empty() {
            self.close(0);
        }
        self.closed
    }
}

fn label_gap(message: &EdgeInput) -> usize {
    if message.target >= message.source {
        message.source
    } else {
        message.source - 1
    }
}

fn centre_distance_needed(message: &EdgeInput) -> f64 {
    let loop_room = if message.source == message.target { LOOP_WIDTH + LABEL_GAP } else { 0.0 };
    mask_width(&message.text) + loop_room + LABEL_ROOM
}

fn gap_widths(widths: &[f64], messages: &[EdgeInput]) -> Vec<f64> {
    let mut gaps: Vec<f64> = widths.windows(2).map(|_| MIN_GAP).collect();
    for message in messages.iter().filter(|message| !message.text.is_empty()) {
        let gap = label_gap(message);
        if gap < gaps.len() {
            let boxes = (widths[gap] + widths[gap + 1]) / 2.0;
            gaps[gap] = gaps[gap].max(snap_up(centre_distance_needed(message) - boxes));
        }
    }
    gaps
}

fn draw_message(message: &EdgeInput, y: f64, centres: &[f64], insets: (f64, f64)) -> SceneEdge {
    let centre = centres[message.source];
    let (points, label) = if message.source == message.target {
        let from = centre + insets.0;
        let right = centre + LOOP_WIDTH;
        let points = vec![Point::new(from, y), Point::new(right, y), Point::new(right, y + LOOP_DROP), Point::new(from, y + LOOP_DROP)];
        let mask = Rect::new(right + LABEL_GAP, y + LOOP_DROP / 2.0 - MASK_HEIGHT / 2.0, mask_width(&message.text), MASK_HEIGHT);
        (points, mask)
    } else {
        let to = centres[message.target];
        let direction = (to - centre).signum();
        let gap = label_gap(message);
        let gap_mid = (centres[gap] + centres[gap + 1]) / 2.0;
        let width = mask_width(&message.text);
        let mask = Rect::new(gap_mid - width / 2.0, y - LABEL_GAP - MASK_HEIGHT, width, MASK_HEIGHT);
        (vec![Point::new(centre + direction * insets.0, y), Point::new(to - direction * insets.1, y)], mask)
    };
    let label = (!message.text.is_empty()).then(|| LabelBox { rect: label, text: message.text.clone() });
    SceneEdge { label, ..SceneEdge::new(message, points) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(json: &str) -> View {
        serde_json::from_str(json).expect("fixture parses")
    }

    fn bars_of(view: &View) -> Vec<(String, usize, usize)> {
        let (ids, _) = unique_nodes(view);
        let mut bars: Vec<_> = activations(&known_edges(view, &ids)).into_iter().map(|bar| (ids[bar.node].clone(), bar.first, bar.last)).collect();
        bars.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        bars
    }

    #[test]
    fn a_reply_closes_the_bar_its_call_opened_and_the_bar_of_who_started_the_exchange() {
        let bars = bars_of(&view(include_str!("../tests/runner-services.json")));
        let expected = [("agent", 0, 8), ("runner", 0, 8), ("stack", 1, 3), ("worker", 4, 7), ("stack", 5, 6), ("agent", 9, 9), ("runner", 9, 10), ("stack", 10, 10)];
        assert_eq!(bars, expected.map(|(id, first, last)| (id.to_string(), first, last)));
    }

    #[test]
    fn a_self_message_of_an_idle_participant_opens_its_bar_until_its_call_is_answered() {
        let bars = bars_of(&view(include_str!("../tests/login.json")));
        assert_eq!(bars.first(), Some(&("browser".to_string(), 0, 3)), "{bars:?}");
    }

    #[test]
    fn a_message_starts_and_ends_at_the_edge_of_an_activation_bar() {
        let scene = build(&view(include_str!("../tests/runner-services.json")));
        let centre = |id: &str| scene.nodes.iter().find(|node| node.id == id).expect("a node").rect.cx();
        let call = scene.edges.iter().find(|edge| edge.id == "runner->stack").expect("the call");
        assert_eq!(call.points[0].x, centre("runner") + BAR_HALF);
        assert_eq!(call.points[1].x, centre("stack") - BAR_HALF);
        assert_eq!(scene.decor.len(), 2 * 8);
    }
}
