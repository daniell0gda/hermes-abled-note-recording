//! Kanban: cards in work-state columns. The groups are the columns, a bracketed number in a group's label
//! is the column's WIP limit, and every card shows one of four states. A board has no connectors, so the
//! view's edges are not drawn.

use crate::figure::{Anchor, Figure, LegendKey, Paint, Section, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{name_and_sublabel, NAME_LINE, NAME_SIZE, SUB_LINE, SUB_SIZE};
use crate::params::Params;
use crate::scene::{unique_nodes, Scene, SceneNode, MARGIN};
use crate::spec::{NodeSpec, Shape, View};
use crate::text::{mono_width, sans_width, split_in_two};

pub(super) const TEXT_INSET: f64 = 12.0;
const TITLE_ASCENT: f64 = 11.0;
const COLUMN_WIDTH: f64 = 240.0;
const GUTTER: f64 = 32.0;
const COLUMN_RADIUS: f64 = 8.0;
const HEADER_INSET: f64 = 16.0;
const HEADER_BASELINE: f64 = 20.0;
const HEADER_RULE: f64 = 36.0;
const NAME_CHIP_GAP: f64 = 12.0;
const FIRST_CARD: f64 = 48.0;
const COLUMN_FOOT: f64 = 16.0;
const CHIP_TOP: f64 = 8.0;
const CHIP_HEIGHT: f64 = 16.0;
const CHIP_BASELINE: f64 = 11.0;
const CHIP_MIN_WIDTH: f64 = 40.0;
const CHIP_PADDING: f64 = 8.0;
const CHIP_SIZE: f64 = 8.0;
const CARD_INSET: f64 = 16.0;
const CARD_HEIGHT: f64 = 56.0;
const CARD_ROOM: f64 = 26.0;
const CARD_GAP: f64 = 12.0;
const CARD_RADIUS: f64 = 6.0;
const BAR_WIDTH: f64 = 4.0;
const BARRED_TEXT_INSET: f64 = 16.0;
const OVER_LIMIT: &str = "OVER WIP LIMIT";

/// A card's title and the sublabel of its trailing bracket, as in `Title (TICKET-1 · owner)`.
pub(super) struct CardLabel {
    pub title: String,
    pub sub: Option<String>,
}

impl CardLabel {
    pub(super) fn of(label: &str) -> CardLabel {
        let (title, sub) = name_and_sublabel(label);
        CardLabel { title, sub }
    }

    /// The title lines: one when the title fits `room`, else two split at the middle space.
    pub(super) fn lines(&self, room: f64) -> Vec<String> {
        title_lines(&self.title, room)
    }

    /// The widest text line when the title wraps at `room`.
    pub(super) fn text_width(&self, room: f64) -> f64 {
        let title = self.lines(room).iter().map(|line| sans_width(line, NAME_SIZE)).fold(0.0, f64::max);
        title.max(self.sub.as_deref().map_or(0.0, |sub| mono_width(sub, SUB_SIZE, 0.0)))
    }

    /// The card height for the title wrapped at `room`: `least`, or the text block plus `padding`.
    pub(super) fn height(&self, room: f64, least: f64, padding: f64) -> f64 {
        least.max(snap_up(self.block_height(self.lines(room).len()) + padding))
    }

    fn block_height(&self, lines: usize) -> f64 {
        lines as f64 * NAME_LINE + if self.sub.is_some() { SUB_LINE } else { 0.0 }
    }

    /// The title (Geist 12px semibold) and sublabel (Geist Mono 9px muted), left-aligned `inset` into
    /// `card` and centred vertically, wrapped at the room left of `TEXT_INSET` on the right.
    pub(super) fn figures(&self, card: Rect, inset: f64) -> Vec<Figure> {
        let lines = self.lines(card.w - inset - TEXT_INSET);
        let x = card.x + inset;
        let top = card.y + (card.h - self.block_height(lines.len())) / 2.0;
        let mut baseline = top + TITLE_ASCENT;
        let mut figures = Vec::new();
        for line in lines {
            figures.push(Figure::Text(Text::sans(Point::new(x, baseline), line, NAME_SIZE).weight(600).anchor(Anchor::Start)));
            baseline += NAME_LINE;
        }
        if let Some(sub) = &self.sub {
            let at = Point::new(x, baseline - NAME_LINE + SUB_LINE);
            figures.push(Figure::Text(Text::mono(at, sub.clone(), SUB_SIZE).anchor(Anchor::Start)));
        }
        figures
    }
}

/// The lines of a title: one when it fits `room`, else two split at the space nearest the middle.
pub(super) fn title_lines(title: &str, room: f64) -> Vec<String> {
    if sans_width(title, NAME_SIZE) <= room {
        return vec![title.to_string()];
    }
    split_in_two(title).map_or_else(|| vec![title.to_string()], |(first, second)| vec![first, second])
}

/// The first spec of every distinct node id, in the order of `unique_nodes`.
pub(super) fn first_specs(view: &View) -> Vec<&NodeSpec> {
    let mut specs: Vec<&NodeSpec> = Vec::new();
    for spec in &view.nodes {
        if !specs.iter().any(|known| known.id == spec.id) {
            specs.push(spec);
        }
    }
    specs
}

/// The look of a card, after the diagram-design kanban states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardState {
    Default,
    Blocked,
    Waiting,
    Done,
}

const STATES: [CardState; 4] = [CardState::Default, CardState::Blocked, CardState::Waiting, CardState::Done];

impl CardState {
    fn of(spec: &NodeSpec, blocked: bool, in_done_column: bool) -> CardState {
        if blocked {
            CardState::Blocked
        } else if matches!(spec.shape, Shape::External | Shape::Note) {
            CardState::Waiting
        } else if in_done_column {
            CardState::Done
        } else {
            CardState::Default
        }
    }

    fn paint(self) -> Paint {
        match self {
            CardState::Default => Paint::shape(Tone::Fill, Tone::Ink, 1.0),
            CardState::Blocked => Paint::shape(Tone::AccentWash(0.05), Tone::Accent, 1.0).dashed("4,4"),
            CardState::Waiting => Paint::shape(Tone::Wash(0.02), Tone::Wash(0.20), 1.0).dashed("4,3"),
            CardState::Done => Paint::shape(Tone::Wash(0.05), Tone::Muted, 1.0),
        }
    }

    fn legend(self) -> &'static str {
        match self {
            CardState::Default => "DEFAULT",
            CardState::Blocked => "BLOCKED",
            CardState::Waiting => "WAITING / EXTERNAL",
            CardState::Done => "DONE",
        }
    }

    fn text_inset(self) -> f64 {
        if self == CardState::Blocked {
            BARRED_TEXT_INSET
        } else {
            TEXT_INSET
        }
    }
}

/// A column of the board: a group, or the unnamed column of the cards no group lists (empty id).
struct Column {
    id: String,
    name: String,
    limit: Option<usize>,
    cards: Vec<usize>,
}

impl Column {
    fn chip_text(&self) -> String {
        match self.limit {
            Some(limit) => format!("{}/{limit}", self.cards.len()),
            None => self.cards.len().to_string(),
        }
    }

    fn is_over_limit(&self) -> bool {
        self.limit.is_some_and(|limit| self.cards.len() > limit)
    }

    fn header_width(&self) -> f64 {
        let name = if self.name.is_empty() { 0.0 } else { sans_width(&self.name, NAME_SIZE) + NAME_CHIP_GAP };
        name + chip_width(&self.chip_text()) + 2.0 * HEADER_INSET
    }
}

/// The column name and WIP limit of a group label: `In progress (3)` is `In progress` with limit 3. A
/// bracket that is not a number stays part of the name.
fn wip_label(label: &str) -> (String, Option<usize>) {
    let (name, bracket) = name_and_sublabel(label);
    match bracket.and_then(|bracket| bracket.parse().ok()) {
        Some(limit) => (name, Some(limit)),
        None => (label.trim().to_string(), None),
    }
}

fn columns(view: &View, ids: &[String]) -> Vec<Column> {
    let mut placed = vec![false; ids.len()];
    let mut columns = Vec::new();
    for group in &view.groups {
        let label = if group.label.trim().is_empty() { &group.id } else { &group.label };
        let (name, limit) = wip_label(label);
        let mut cards = Vec::new();
        for index in group.members.iter().filter_map(|member| ids.iter().position(|id| id == member)) {
            if !std::mem::replace(&mut placed[index], true) {
                cards.push(index);
            }
        }
        columns.push(Column { id: group.id.clone(), name, limit, cards });
    }
    let loose: Vec<usize> = (0..ids.len()).filter(|&index| !placed[index]).collect();
    if !loose.is_empty() {
        columns.insert(0, Column { id: String::new(), name: String::new(), limit: None, cards: loose });
    }
    columns
}

/// The rightmost column when the board has at least two named columns: its cards are done.
fn done_column(view: &View, columns: &[Column]) -> Option<usize> {
    (view.groups.len() >= 2).then(|| columns.len() - 1)
}

fn card_states(view: &View, specs: &[&NodeSpec], columns: &[Column]) -> Vec<CardState> {
    let blocked = specs.iter().position(|spec| spec.focal);
    let done = done_column(view, columns);
    let mut states = vec![CardState::Default; specs.len()];
    for (index, column) in columns.iter().enumerate() {
        for &card in &column.cards {
            states[card] = CardState::of(specs[card], blocked == Some(card), done == Some(index));
        }
    }
    states
}

fn card_room(column_width: f64, state: CardState) -> f64 {
    column_width - 2.0 * CARD_INSET - state.text_inset() - TEXT_INSET
}

/// One width for every column: 240, or wider when a card title or a header would not fit.
fn column_width(columns: &[Column], labels: &[CardLabel], states: &[CardState]) -> f64 {
    let cards = labels.iter().zip(states).map(|(label, &state)| {
        label.text_width(card_room(COLUMN_WIDTH, state)) + state.text_inset() + TEXT_INSET + 2.0 * CARD_INSET
    });
    snap_up(cards.chain(columns.iter().map(Column::header_width)).fold(COLUMN_WIDTH, f64::max))
}

fn chip_width(text: &str) -> f64 {
    snap_up(mono_width(text, CHIP_SIZE, 0.0) + 2.0 * CHIP_PADDING).max(CHIP_MIN_WIDTH)
}

fn chip_paint(over_limit: bool) -> (Paint, Tone) {
    if over_limit {
        (Paint::shape(Tone::None, Tone::Accent, 0.8), Tone::Accent)
    } else {
        (Paint::shape(Tone::None, Tone::Wash(0.40), 0.8), Tone::Wash(0.8))
    }
}

fn chip(column: &Column, rect: Rect) -> [Figure; 2] {
    let text = column.chip_text();
    let width = chip_width(&text);
    let chip = Rect::new(rect.right() - HEADER_INSET - width, rect.y + CHIP_TOP, width, CHIP_HEIGHT);
    let (paint, tone) = chip_paint(column.is_over_limit());
    let at = Point::new(chip.cx(), chip.y + CHIP_BASELINE);
    [Figure::rect(chip, 2.0, paint), Figure::Text(Text::mono(at, text, CHIP_SIZE).tone(tone))]
}

fn column_section(column: &Column, rect: Rect) -> Section {
    let mut figures = vec![Figure::rect(rect, COLUMN_RADIUS, Paint::filled(Tone::Wash(0.02)))];
    if !column.name.is_empty() {
        let at = Point::new(rect.x + HEADER_INSET, rect.y + HEADER_BASELINE);
        figures.push(Figure::Text(Text::sans(at, column.name.clone(), NAME_SIZE).weight(600).anchor(Anchor::Start)));
    }
    figures.extend(chip(column, rect));
    let rule = rect.y + HEADER_RULE;
    figures.push(Figure::line(Point::new(rect.x, rule), Point::new(rect.right(), rule), Paint::line(Tone::Wash(0.20), 1.0)));
    Section { id: column.id.clone(), figures }
}

fn card_figures(card: Rect, label: &CardLabel, state: CardState) -> Vec<Figure> {
    let mut figures = vec![Figure::rect(card, CARD_RADIUS, state.paint())];
    if state == CardState::Blocked {
        figures.push(Figure::rect(Rect::new(card.x, card.y, BAR_WIDTH, card.h), 0.0, Paint::filled(Tone::Accent)));
    }
    figures.extend(label.figures(card, state.text_inset()));
    figures
}

fn legend(states: &[CardState], columns: &[Column]) -> Vec<LegendKey> {
    let used = STATES.into_iter().filter(|state| states.contains(state));
    let mut keys: Vec<LegendKey> = used.map(|state| LegendKey::new(Swatch::Box(state.paint()), state.legend())).collect();
    if columns.iter().any(Column::is_over_limit) {
        keys.push(LegendKey::new(Swatch::Box(chip_paint(true).0), OVER_LIMIT));
    }
    if keys.len() < 2 {
        keys.clear();
    }
    keys
}

/// Lays out `view` as a kanban board: one column per group, left to right, with its cards stacked under
/// the header. The view's edges are not drawn.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let specs = first_specs(view);
    let columns = columns(view, &ids);
    let states = card_states(view, &specs, &columns);
    let labels: Vec<CardLabel> = specs.iter().map(|spec| CardLabel::of(&spec.label)).collect();
    let width = column_width(&columns, &labels, &states);
    let heights: Vec<f64> =
        labels.iter().zip(&states).map(|(label, &state)| label.height(card_room(width, state), CARD_HEIGHT, CARD_ROOM)).collect();
    let stack = |column: &Column| column.cards.iter().map(|&card| heights[card] + CARD_GAP).sum::<f64>() - CARD_GAP;
    let deepest = columns.iter().map(stack).fold(CARD_HEIGHT, f64::max);
    let height = FIRST_CARD + deepest + COLUMN_FOOT;
    let mut nodes = Vec::new();
    let mut sections = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let rect = Rect::new(MARGIN + index as f64 * (width + GUTTER), MARGIN, width, height);
        sections.push(column_section(column, rect));
        let mut y = rect.y + FIRST_CARD;
        for &card in &column.cards {
            let card_rect = Rect::new(rect.x + CARD_INSET, y, width - 2.0 * CARD_INSET, heights[card]);
            let figures = card_figures(card_rect, &labels[card], states[card]);
            nodes.push(SceneNode { figures, ..SceneNode::new(ids[card].clone(), card_rect, contents[card].clone()) });
            y += card_rect.h + CARD_GAP;
        }
    }
    let legend = legend(&states, &columns);
    Scene { nodes, sections, legend: Some(legend), ..Scene::empty(view.kind) }.measured()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn board(groups: serde_json::Value, nodes: serde_json::Value) -> View {
        serde_json::from_value(json!({"kind": "kanban", "nodes": nodes, "groups": groups})).expect("a view")
    }

    fn card(id: &str) -> serde_json::Value {
        json!({"id": id, "label": format!("Card {id} (AVA-1 · sam)")})
    }

    fn fixture() -> View {
        serde_json::from_str(include_str!("../../tests/kanban.json")).expect("fixture parses")
    }

    fn shape_paint(node: &SceneNode) -> Paint {
        match &node.figures[0] {
            Figure::Rect { paint, .. } => *paint,
            other => panic!("a card starts with its box, not {other:?}"),
        }
    }

    fn chip_stroke(section: &Section) -> Tone {
        let chips: Vec<Tone> = section
            .figures
            .iter()
            .filter_map(|figure| match figure {
                Figure::Rect { radius, paint, .. } if *radius == 2.0 => Some(paint.stroke),
                _ => None,
            })
            .collect();
        assert_eq!(chips.len(), 1, "one chip per column");
        chips[0]
    }

    fn texts(figures: &[Figure]) -> Vec<String> {
        figures.iter().filter_map(|figure| if let Figure::Text(text) = figure { Some(text.text.clone()) } else { None }).collect()
    }

    #[test]
    fn a_bracketed_number_in_a_column_label_is_its_wip_limit() {
        assert_eq!(wip_label("In progress (3)"), ("In progress".to_string(), Some(3)));
        assert_eq!(wip_label("Backlog"), ("Backlog".to_string(), None));
        assert_eq!(wip_label("Review (team B)"), ("Review (team B)".to_string(), None));
    }

    #[test]
    fn a_header_shows_the_name_without_the_bracket_and_the_count_against_the_limit() {
        let view = board(json!([{"id": "doing", "label": "In progress (3)", "members": ["a", "b"]}]), json!([card("a"), card("b")]));
        let scene = build(&view, &Params::default());
        assert_eq!(texts(&scene.sections[0].figures), vec!["In progress", "2/3"]);
    }

    #[test]
    fn a_column_without_a_limit_shows_a_bare_count() {
        let view = board(json!([{"id": "todo", "label": "Backlog", "members": ["a", "b", "c"]}]), json!([card("a"), card("b"), card("c")]));
        assert_eq!(texts(&build(&view, &Params::default()).sections[0].figures), vec!["Backlog", "3"]);
    }

    #[test]
    fn only_a_column_over_its_limit_has_an_accent_chip() {
        let view = board(
            json!([
                {"id": "doing", "label": "Doing (1)", "members": ["a", "b"]},
                {"id": "review", "label": "Review (2)", "members": ["c", "d"]}
            ]),
            json!([card("a"), card("b"), card("c"), card("d")]),
        );
        let scene = build(&view, &Params::default());
        assert_eq!(chip_stroke(&scene.sections[0]), Tone::Accent);
        assert_eq!(chip_stroke(&scene.sections[1]), Tone::Wash(0.40));
        let legend: Vec<String> = scene.legend.expect("a legend").iter().map(|key| key.text.clone()).collect();
        assert!(legend.contains(&OVER_LIMIT.to_string()));
    }

    #[test]
    fn a_card_is_blocked_when_focal_waiting_when_external_and_done_in_the_last_column() {
        let scene = build(&fixture(), &Params::default());
        let paint_of = |id: &str| shape_paint(scene.nodes.iter().find(|node| node.id == id).expect("a card"));
        assert_eq!(paint_of("pg16"), CardState::Blocked.paint());
        assert_eq!(paint_of("sso"), CardState::Waiting.paint());
        assert_eq!(paint_of("retention"), CardState::Done.paint());
        assert_eq!(paint_of("ratelimit"), CardState::Default.paint());
    }

    #[test]
    fn a_blocked_card_has_an_accent_bar_on_its_left_edge() {
        let scene = build(&fixture(), &Params::default());
        let blocked = scene.nodes.iter().find(|node| node.id == "pg16").expect("the blocked card");
        let bar = Figure::rect(Rect::new(blocked.rect.x, blocked.rect.y, BAR_WIDTH, blocked.rect.h), 0.0, Paint::filled(Tone::Accent));
        assert_eq!(blocked.figures[1], bar);
    }

    #[test]
    fn only_the_first_focal_card_is_blocked() {
        let mut nodes = vec![card("a"), card("b")];
        nodes.iter_mut().for_each(|node| node["focal"] = json!(true));
        let view = board(json!([{"id": "doing", "label": "Doing", "members": ["a", "b"]}]), json!(nodes));
        let scene = build(&view, &Params::default());
        assert_eq!(shape_paint(&scene.nodes[0]), CardState::Blocked.paint());
        assert_eq!(shape_paint(&scene.nodes[1]), CardState::Default.paint());
    }

    #[test]
    fn a_single_column_is_not_done() {
        let view = board(json!([{"id": "doing", "label": "Doing", "members": ["a"]}]), json!([card("a"), card("b")]));
        let scene = build(&view, &Params::default());
        assert!(scene.nodes.iter().all(|node| shape_paint(node) == CardState::Default.paint()));
    }

    #[test]
    fn cards_in_no_column_go_to_an_unnamed_column_at_the_left() {
        let view = board(
            json!([{"id": "doing", "label": "Doing", "members": ["a"]}, {"id": "done", "label": "Done", "members": ["b"]}]),
            json!([card("a"), card("b"), card("loose")]),
        );
        let scene = build(&view, &Params::default());
        let ids: Vec<&str> = scene.sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids, vec!["", "doing", "done"]);
        let loose = scene.nodes.iter().find(|node| node.id == "loose").expect("the loose card");
        assert!(loose.rect.right() < scene.sections[1].figures[0].bounds().x);
    }

    #[test]
    fn a_card_sits_in_the_first_column_that_lists_it() {
        let view = board(
            json!([{"id": "doing", "label": "Doing", "members": ["a"]}, {"id": "done", "label": "Done", "members": ["a", "b"]}]),
            json!([card("a"), card("b")]),
        );
        let scene = build(&view, &Params::default());
        assert_eq!(scene.nodes.len(), 2);
        assert_eq!(texts(&scene.sections[1].figures), vec!["Done", "1"]);
    }

    #[test]
    fn every_card_sits_inside_its_column_below_the_header_rule() {
        let view = fixture();
        let scene = build(&view, &Params::default());
        for (section, group) in scene.sections.iter().zip(&view.groups) {
            let column = section.figures[0].bounds();
            for node in scene.nodes.iter().filter(|node| group.members.contains(&node.id)) {
                let inside = node.rect.x > column.x && node.rect.right() < column.right() && node.rect.bottom() < column.bottom();
                assert!(inside && node.rect.y > column.y + HEADER_RULE, "{} outside {}", node.id, group.id);
            }
        }
        for (a, b) in scene.nodes.iter().zip(scene.nodes.iter().skip(1)) {
            assert!(!a.rect.overlaps(&b.rect), "{} overlaps {}", a.id, b.id);
        }
    }

    #[test]
    fn a_long_title_wraps_to_two_lines_on_a_taller_card() {
        let scene = build(&fixture(), &Params::default());
        let long = scene.nodes.iter().find(|node| node.id == "push").expect("the long card");
        assert_eq!(texts(&long.figures).len(), 3, "two title lines and the sublabel");
        assert!(long.rect.h > CARD_HEIGHT);
        assert_eq!(scene.nodes.iter().find(|node| node.id == "ratelimit").expect("a short card").rect.h, CARD_HEIGHT);
    }

    #[test]
    fn a_board_draws_no_edges_and_names_its_card_states() {
        let mut view = fixture();
        view.edges = serde_json::from_value(json!([{"source": "ratelimit", "target": "pg16"}])).expect("edges");
        let scene = build(&view, &Params::default());
        assert!(scene.edges.is_empty());
        let legend: Vec<String> = scene.legend.expect("a legend").iter().map(|key| key.text.clone()).collect();
        assert_eq!(legend, vec!["DEFAULT", "BLOCKED", "WAITING / EXTERNAL", "DONE", OVER_LIMIT]);
    }
}
