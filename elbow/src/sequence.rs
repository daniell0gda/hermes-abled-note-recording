//! Sequence layout: participants in a row with lifelines, one message per row in spoken order.
//! A message label sits in the gap next to its sender, and each gap is wide enough for its labels.

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

/// Lays out `view` as a sequence diagram.
pub fn build(view: &View) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let messages = known_edges(view, &ids);
    let order: Vec<usize> = (0..ids.len()).collect();
    let column: Vec<usize> = {
        let mut column = vec![0; ids.len()];
        for (position, &node) in order.iter().enumerate() {
            column[node] = position;
        }
        column
    };
    let widths: Vec<f64> = order.iter().map(|&node| contents[node].width).collect();
    let gaps = gap_widths(&widths, &messages, &column);
    let height = contents.iter().map(|content| content.height).fold(0.0, f64::max);
    let mut centres = Vec::with_capacity(order.len());
    let mut x = MARGIN;
    for (position, width) in widths.iter().enumerate() {
        if position > 0 {
            x += gaps[position - 1];
        }
        centres.push(x + width / 2.0);
        x += width;
    }
    let first_row = MARGIN + height + FIRST_ROW;
    let edges: Vec<SceneEdge> = messages
        .iter()
        .enumerate()
        .map(|(row, message)| draw_message(message, first_row + row as f64 * ROW, &centres, &column))
        .collect();
    let bottom = first_row + messages.len().saturating_sub(1) as f64 * ROW + LOOP_DROP + TAIL;
    let lifelines = order
        .iter()
        .enumerate()
        .map(|(position, _)| Segment::new(Point::new(centres[position], MARGIN + height), Point::new(centres[position], bottom)))
        .collect();
    let nodes: Vec<SceneNode> = ids
        .into_iter()
        .zip(contents)
        .enumerate()
        .map(|(node, (id, content))| {
            let centre = centres[column[node]];
            let rect = Rect::new(centre - content.width / 2.0, MARGIN, content.width, height);
            SceneNode::new(id, rect, content)
        })
        .collect();
    let right = x.max(edges.iter().filter_map(|edge| edge.label.as_ref()).map(|label| label.rect.right()).fold(0.0, f64::max));
    Scene { nodes, edges, lifelines, width: snap_up(right + MARGIN), height: snap_up(bottom + MARGIN), ..Scene::empty(view.kind) }
}

fn label_gap(message: &EdgeInput, column: &[usize]) -> usize {
    let from = column[message.source];
    if column[message.target] >= from {
        from
    } else {
        from - 1
    }
}

fn centre_distance_needed(message: &EdgeInput) -> f64 {
    let loop_room = if message.source == message.target { LOOP_WIDTH + LABEL_GAP } else { 0.0 };
    mask_width(&message.text) + loop_room + LABEL_ROOM
}

fn gap_widths(widths: &[f64], messages: &[EdgeInput], column: &[usize]) -> Vec<f64> {
    let mut gaps: Vec<f64> = widths.windows(2).map(|_| MIN_GAP).collect();
    for message in messages.iter().filter(|message| !message.text.is_empty()) {
        let gap = label_gap(message, column);
        if gap < gaps.len() {
            let boxes = (widths[gap] + widths[gap + 1]) / 2.0;
            gaps[gap] = gaps[gap].max(snap_up(centre_distance_needed(message) - boxes));
        }
    }
    gaps
}

fn draw_message(message: &EdgeInput, y: f64, centres: &[f64], column: &[usize]) -> SceneEdge {
    let from = centres[column[message.source]];
    let to = centres[column[message.target]];
    let (points, label) = if message.source == message.target {
        let right = from + LOOP_WIDTH;
        let points = vec![Point::new(from, y), Point::new(right, y), Point::new(right, y + LOOP_DROP), Point::new(from, y + LOOP_DROP)];
        let mask = Rect::new(right + LABEL_GAP, y + LOOP_DROP / 2.0 - MASK_HEIGHT / 2.0, mask_width(&message.text), MASK_HEIGHT);
        (points, mask)
    } else {
        let gap = label_gap(message, column);
        let gap_mid = (centres[gap] + centres[gap + 1]) / 2.0;
        let width = mask_width(&message.text);
        let mask = Rect::new(gap_mid - width / 2.0, y - LABEL_GAP - MASK_HEIGHT, width, MASK_HEIGHT);
        (vec![Point::new(from, y), Point::new(to, y)], mask)
    };
    let label = (!message.text.is_empty()).then(|| LabelBox { rect: label, text: message.text.clone() });
    SceneEdge { label, ..SceneEdge::new(message, points) }
}
