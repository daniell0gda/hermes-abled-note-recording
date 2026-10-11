//! Quadrant: items as small labelled dots in a two-by-two grid. The groups are the quadrants in reading
//! order (top-left, top-right, bottom-left, bottom-right), each a selectable cell with a numbered corner
//! tag; items in no quadrant sit in an `UNPLACED` strip under the grid. A title of the form `A vs B`
//! names the vertical axis (A) and the horizontal axis (B). Edges are not drawn.

use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Section, Swatch, Text, Tone};
use crate::geom::{snap, snap_up, Point, Rect};
use crate::node::{NodeContent, Treatment};
use crate::params::Params;
use crate::scene::{unique_nodes, Scene, SceneNode};
use crate::spec::View;
use crate::text::sans_width;

const DOT: f64 = 4.0;
const FOCAL_DOT: f64 = 6.0;
const ITEM_SIZE: f64 = 11.0;
const LABEL_OFFSET: f64 = 8.0;
const ITEM_HALF_HEIGHT: f64 = 9.0;
const HIT_PADDING: f64 = 2.0;
const ROW: f64 = 28.0;
const COLUMN_GAP: f64 = 24.0;
const ROWS_PER_COLUMN: usize = 4;
const CELL_PADDING: f64 = 20.0;
const TAG_ROOM: f64 = 20.0;
const MIN_CELL_WIDTH: f64 = 240.0;
const MIN_CELL_HEIGHT: f64 = 152.0;
const TAG_SIZE: f64 = 8.0;
const TAG_TRACKING: f64 = 0.18;
const AXIS_SIZE: f64 = 9.0;
const AXIS_TRACKING: f64 = 0.18;
const AXIS_OVERSHOOT: f64 = 28.0;
const AXIS_LABEL_GAP: f64 = 12.0;
const AXIS_WORDS: usize = 3;
const STRIP_GAP: f64 = 40.0;
const STRIP_INDENT: f64 = 24.0;
const QUADRANTS: usize = 4;
const UNPLACED: &str = "UNPLACED";
const SEPARATORS: [&str; 5] = [" versus ", " vs. ", " vs ", "\u{d7}", " x "];
const CLAUSE_BREAKS: [char; 8] = ['\u{b7}', ':', '\u{2014}', '\u{2013}', ',', '(', ')', '|'];
const STOP_WORDS: [&str; 12] = ["by", "of", "for", "on", "in", "across", "with", "to", "at", "per", "from", "the"];

/// Lays out `view` as a quadrant.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let items = items(&contents);
    let quadrants = quadrant_of(view, &ids);
    let tags = tags(view);
    let grid = Grid::fitting(&items, &quadrants, &tags);
    let mut centres = grid.place(&items, &quadrants);
    let mut decor = grid.axes(axis_words(&view.title));
    let unplaced: Vec<&Item> = items.iter().filter(|item| quadrants[item.node].is_none()).collect();
    if !unplaced.is_empty() {
        let (strip_centres, label) = grid.strip(&unplaced);
        unplaced.iter().zip(strip_centres).for_each(|(item, centre)| centres[item.node] = Some(centre));
        decor.push(label);
    }
    let sections = grid.sections(view, &tags);
    let nodes = ids
        .into_iter()
        .zip(contents)
        .zip(&items)
        .zip(centres)
        .map(|(((id, content), item), centre)| item_node(item, centre.unwrap_or(Point::new(0.0, 0.0)), id, content))
        .collect();
    let legend = legend(items.iter().any(|item| item.focal));
    Scene { nodes, sections, decor, legend: Some(legend), ..Scene::empty(view.kind) }.measured()
}

/// The vertical and horizontal axis words of a title like `Value vs effort` or `Impact × Effort`:
/// the phrase before the separator names the vertical axis, the phrase after it the horizontal one.
pub fn axis_words(title: &str) -> Option<(String, String)> {
    let lower = title.to_ascii_lowercase();
    let (at, length) = SEPARATORS
        .iter()
        .filter_map(|separator| lower.find(separator).map(|at| (at, separator.len())))
        .min_by_key(|(at, _)| *at)?;
    let vertical = phrase_before(&title[..at]);
    let horizontal = phrase_after(&title[at + length..]);
    (!vertical.is_empty() && !horizontal.is_empty()).then(|| (vertical.to_uppercase(), horizontal.to_uppercase()))
}

fn phrase_before(text: &str) -> String {
    let clause = text.rsplit(CLAUSE_BREAKS).next().unwrap_or_default();
    let words: Vec<&str> = clause.split_whitespace().collect();
    let start = words.iter().rposition(|word| is_stop_word(word)).map_or(0, |at| at + 1);
    let kept = &words[start..];
    kept[kept.len().saturating_sub(AXIS_WORDS)..].join(" ")
}

fn phrase_after(text: &str) -> String {
    let clause = text.split(CLAUSE_BREAKS).next().unwrap_or_default();
    let words: Vec<&str> = clause.split_whitespace().take_while(|word| !is_stop_word(word)).take(AXIS_WORDS).collect();
    words.join(" ")
}

fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word.to_ascii_lowercase().as_str())
}

struct Item {
    node: usize,
    label: String,
    focal: bool,
}

impl Item {
    fn radius(&self) -> f64 {
        if self.focal {
            FOCAL_DOT
        } else {
            DOT
        }
    }

    /// How far the item reaches right of its dot's centre.
    fn reach(&self) -> f64 {
        self.radius() + LABEL_OFFSET + sans_width(&self.label, ITEM_SIZE)
    }

    /// The width of the item measured from the left edge of the widest dot.
    fn width(&self) -> f64 {
        FOCAL_DOT + self.reach()
    }
}

fn items(contents: &[NodeContent]) -> Vec<Item> {
    let focal = contents.iter().position(|content| content.treatment == Treatment::Focal);
    contents
        .iter()
        .enumerate()
        .map(|(node, content)| Item { node, label: content.lines.join(" "), focal: Some(node) == focal })
        .collect()
}

fn quadrant_of(view: &View, ids: &[String]) -> Vec<Option<usize>> {
    ids.iter().map(|id| view.groups.iter().take(QUADRANTS).position(|group| group.members.contains(id))).collect()
}

/// The corner tag of each quadrant a group names: `01 · QUICK WINS`.
fn tags(view: &View) -> Vec<Option<String>> {
    (0..QUADRANTS)
        .map(|quadrant| {
            view.groups.get(quadrant).map(|group| {
                let name = if group.label.is_empty() { &group.id } else { &group.label };
                format!("{:02} \u{b7} {}", quadrant + 1, name.to_uppercase())
            })
        })
        .collect()
}

/// The items of one quadrant in a small grid of columns, top to bottom, with dot centres relative to
/// the block's top-left corner.
struct Block {
    centres: Vec<Point>,
    width: f64,
    height: f64,
}

impl Block {
    fn of(items: &[&Item]) -> Block {
        if items.is_empty() {
            return Block { centres: Vec::new(), width: 0.0, height: 0.0 };
        }
        let columns = items.len().div_ceil(ROWS_PER_COLUMN);
        let rows = items.len().div_ceil(columns);
        let mut centres = Vec::with_capacity(items.len());
        let mut x = 0.0;
        for column in items.chunks(rows) {
            centres.extend((0..column.len()).map(|row| Point::new(x + FOCAL_DOT, row as f64 * ROW + ITEM_HALF_HEIGHT)));
            x += snap_up(column.iter().map(|item| item.width()).fold(0.0, f64::max) + COLUMN_GAP);
        }
        Block { centres, width: x - COLUMN_GAP, height: (rows - 1) as f64 * ROW + 2.0 * ITEM_HALF_HEIGHT }
    }
}

/// Four equal cells around the axis cross at (`cell_width`, `cell_height`).
struct Grid {
    cell_width: f64,
    cell_height: f64,
}

impl Grid {
    fn fitting(items: &[Item], quadrants: &[Option<usize>], tags: &[Option<String>]) -> Grid {
        let blocks: Vec<Block> = (0..QUADRANTS).map(|quadrant| Block::of(&members(items, quadrants, quadrant))).collect();
        let widest_block = blocks.iter().map(|block| block.width).fold(0.0, f64::max);
        let widest_tag = tags.iter().flatten().map(|tag| tag_text(tag, Point::new(0.0, 0.0), Anchor::Start).width()).fold(0.0, f64::max);
        let tallest = blocks.iter().map(|block| block.height).fold(0.0, f64::max);
        Grid {
            cell_width: snap_up(MIN_CELL_WIDTH.max(widest_block.max(widest_tag) + 2.0 * CELL_PADDING)),
            cell_height: snap_up(MIN_CELL_HEIGHT.max(tallest + 2.0 * CELL_PADDING + TAG_ROOM)),
        }
    }

    fn cell(&self, quadrant: usize) -> Rect {
        let column = (quadrant % 2) as f64;
        let row = (quadrant / 2) as f64;
        Rect::new(column * self.cell_width, row * self.cell_height, self.cell_width, self.cell_height)
    }

    fn is_top(quadrant: usize) -> bool {
        quadrant < 2
    }

    fn is_left(quadrant: usize) -> bool {
        quadrant % 2 == 0
    }

    /// The part of a cell the items may use: inside the padding and clear of the corner tag, which
    /// sits on the cell's outer side.
    fn item_area(&self, quadrant: usize) -> Rect {
        let cell = self.cell(quadrant);
        let top = cell.y + CELL_PADDING + if Grid::is_top(quadrant) { TAG_ROOM } else { 0.0 };
        Rect::new(cell.x + CELL_PADDING, top, cell.w - 2.0 * CELL_PADDING, cell.h - 2.0 * CELL_PADDING - TAG_ROOM)
    }

    fn place(&self, items: &[Item], quadrants: &[Option<usize>]) -> Vec<Option<Point>> {
        let mut centres = vec![None; items.len()];
        for quadrant in 0..QUADRANTS {
            let members = members(items, quadrants, quadrant);
            let block = Block::of(&members);
            let area = self.item_area(quadrant);
            let left = snap(area.x + (area.w - block.width) / 2.0);
            let top = snap(area.y + (area.h - block.height) / 2.0);
            for (item, centre) in members.iter().zip(block.centres) {
                centres[item.node] = Some(Point::new(left + centre.x, top + centre.y));
            }
        }
        centres
    }

    fn sections(&self, view: &View, tags: &[Option<String>]) -> Vec<Section> {
        view.groups
            .iter()
            .take(QUADRANTS)
            .zip(tags.iter().flatten())
            .enumerate()
            .map(|(quadrant, (group, tag))| Section {
                id: group.id.clone(),
                figures: vec![Figure::rect(self.cell(quadrant), 0.0, Paint::filled(Tone::Transparent)), Figure::Text(self.tag(quadrant, tag))],
            })
            .collect()
    }

    fn tag(&self, quadrant: usize, tag: &str) -> Text {
        let cell = self.cell(quadrant);
        let (x, anchor) = if Grid::is_left(quadrant) { (cell.x + CELL_PADDING, Anchor::Start) } else { (cell.right() - CELL_PADDING, Anchor::End) };
        let y = if Grid::is_top(quadrant) { cell.y + CELL_PADDING + TAG_SIZE } else { cell.bottom() - CELL_PADDING };
        tag_text(tag, Point::new(x, y), anchor)
    }

    /// The axis cross through the centre, single-ended arrows up and right, and the axis words
    /// beyond the arrow tips.
    fn axes(&self, words: Option<(String, String)>) -> Vec<Figure> {
        let (centre_x, centre_y) = (self.cell_width, self.cell_height);
        let top_tip = Point::new(centre_x, -AXIS_OVERSHOOT);
        let right_tip = Point::new(2.0 * self.cell_width + AXIS_OVERSHOOT, centre_y);
        let axis = |from: Point, to: Point| Figure::Line { points: vec![from, to], paint: Paint::line(Tone::Ink, 1.0), end: Marker::Arrow };
        let mut figures = vec![
            axis(Point::new(centre_x, 2.0 * self.cell_height), top_tip),
            axis(Point::new(0.0, centre_y), right_tip),
        ];
        if let Some((vertical, horizontal)) = words {
            let word = |at: Point, text: String, anchor: Anchor| {
                Figure::Text(Text::mono(at, text, AXIS_SIZE).tracking(AXIS_TRACKING).tone(Tone::Ink).anchor(anchor))
            };
            figures.push(word(Point::new(top_tip.x, top_tip.y - AXIS_LABEL_GAP), vertical, Anchor::Middle));
            figures.push(word(Point::new(right_tip.x + AXIS_LABEL_GAP, right_tip.y + AXIS_SIZE / 3.0), horizontal, Anchor::Start));
        }
        figures
    }

    /// The items in no quadrant, in rows under the grid after an `UNPLACED` label.
    fn strip(&self, items: &[&Item]) -> (Vec<Point>, Figure) {
        let top = 2.0 * self.cell_height + STRIP_GAP;
        let label = tag_text(UNPLACED, Point::new(0.0, top + ITEM_HALF_HEIGHT + TAG_SIZE / 3.0), Anchor::Start);
        let start = snap_up(label.width() + STRIP_INDENT);
        let limit = 2.0 * self.cell_width;
        let mut centres = Vec::with_capacity(items.len());
        let (mut x, mut y) = (start, top + ITEM_HALF_HEIGHT);
        for item in items {
            if x > start && x + item.width() > limit {
                (x, y) = (start, y + ROW);
            }
            centres.push(Point::new(x + FOCAL_DOT, y));
            x += snap_up(item.width() + COLUMN_GAP);
        }
        (centres, Figure::Text(label))
    }
}

fn members<'a>(items: &'a [Item], quadrants: &[Option<usize>], quadrant: usize) -> Vec<&'a Item> {
    items.iter().filter(|item| quadrants[item.node] == Some(quadrant)).collect()
}

fn tag_text(text: &str, at: Point, anchor: Anchor) -> Text {
    Text::mono(at, text, TAG_SIZE).tracking(TAG_TRACKING).anchor(anchor)
}

fn item_node(item: &Item, centre: Point, id: String, content: NodeContent) -> SceneNode {
    let radius = item.radius();
    let rect = Rect::new(
        centre.x - radius - HIT_PADDING,
        centre.y - ITEM_HALF_HEIGHT,
        radius + item.reach() + 2.0 * HIT_PADDING,
        2.0 * ITEM_HALF_HEIGHT,
    );
    let dot = Figure::circle(centre, radius, Paint::filled(if item.focal { Tone::Accent } else { Tone::Ink }));
    let label = Text::sans(Point::new(centre.x + radius + LABEL_OFFSET, centre.y + ITEM_SIZE / 3.0), item.label.clone(), ITEM_SIZE)
        .anchor(Anchor::Start);
    let label = if item.focal { label.weight(600) } else { label.tone(Tone::Muted) };
    SceneNode { figures: vec![Figure::hit_area(rect), dot, Figure::Text(label)], ..SceneNode::new(id, rect, content) }
}

fn legend(has_focal: bool) -> Vec<LegendKey> {
    let focal = has_focal.then(|| LegendKey::new(Swatch::Dot(Paint::filled(Tone::Accent)), "DO FIRST"));
    focal.into_iter().chain([LegendKey::new(Swatch::Dot(Paint::filled(Tone::Ink)), "ITEM")]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::build as build_scene;

    fn view(groups: serde_json::Value, nodes: serde_json::Value) -> View {
        serde_json::from_value(serde_json::json!({
            "title": "Backlog by impact vs effort",
            "kind": "quadrant",
            "nodes": nodes,
            "groups": groups,
        }))
        .expect("a view")
    }

    fn four_quadrants() -> View {
        view(
            serde_json::json!([
                {"id": "wins", "label": "Quick wins", "members": ["a", "b"]},
                {"id": "big", "label": "Major projects", "members": ["c"]},
                {"id": "fill", "label": "Fill-ins", "members": ["d", "e", "f"]},
                {"id": "avoid", "label": "Avoid", "members": ["g"]}
            ]),
            serde_json::json!([
                {"id": "a", "label": "Fix signup bug", "focal": true},
                {"id": "b", "label": "Saved carts"},
                {"id": "c", "label": "Offline mode"},
                {"id": "d", "label": "Update docs"},
                {"id": "e", "label": "Tweak onboarding copy"},
                {"id": "f", "label": "Rename settings page"},
                {"id": "g", "label": "Custom reporting engine"},
                {"id": "h", "label": "SSO login"}
            ]),
        )
    }

    fn axis_cross(scene: &Scene) -> Point {
        let Figure::Line { points, .. } = &scene.decor[0] else { panic!("the vertical axis comes first") };
        let Figure::Line { points: across, .. } = &scene.decor[1] else { panic!("then the horizontal axis") };
        Point::new(points[0].x, across[0].y)
    }

    fn node<'a>(scene: &'a Scene, id: &str) -> &'a SceneNode {
        scene.nodes.iter().find(|node| node.id == id).expect("a node")
    }

    #[test]
    fn the_groups_fill_the_quadrants_in_reading_order() {
        let scene = build_scene(&four_quadrants(), &Params::default());
        let cross = axis_cross(&scene);
        let side = |id: &str| {
            let rect = node(&scene, id).rect;
            (rect.bottom() < cross.y, rect.right() < cross.x)
        };
        assert_eq!(side("a"), (true, true), "first group: top left");
        assert_eq!(side("c"), (true, false), "second group: top right");
        assert_eq!(side("d"), (false, true), "third group: bottom left");
        assert_eq!(side("g"), (false, false), "fourth group: bottom right");
    }

    #[test]
    fn each_quadrant_is_a_section_with_a_numbered_corner_tag() {
        let scene = build_scene(&four_quadrants(), &Params::default());
        let ids: Vec<&str> = scene.sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids, vec!["wins", "big", "fill", "avoid"]);
        let Figure::Text(tag) = &scene.sections[0].figures[1] else { panic!("a tag") };
        assert_eq!(tag.text, "01 \u{b7} QUICK WINS");
    }

    #[test]
    fn a_missing_quadrant_stays_empty() {
        let mut two = four_quadrants();
        two.groups.truncate(2);
        let scene = build_scene(&two, &Params::default());
        assert_eq!(scene.sections.len(), 2);
        let cross = axis_cross(&scene);
        let below_cross = scene.nodes.iter().filter(|node| node.rect.y > cross.y).count();
        assert_eq!(below_cross, 5, "the members of the dropped groups and the ungrouped item go to the strip");
        assert!(scene.nodes.iter().filter(|node| node.rect.y > cross.y).all(|node| node.rect.y > grid_bottom(&scene)));
    }

    fn grid_bottom(scene: &Scene) -> f64 {
        let Figure::Line { points, .. } = &scene.decor[0] else { panic!("the vertical axis comes first") };
        points[0].y
    }

    #[test]
    fn the_axis_words_come_from_the_title() {
        let words = |title: &str| axis_words(title);
        let pair = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        assert_eq!(words("Value vs effort"), pair("VALUE", "EFFORT"));
        assert_eq!(words("Impact \u{d7} Effort"), pair("IMPACT", "EFFORT"));
        assert_eq!(words("Reach x Frequency"), pair("REACH", "FREQUENCY"));
        assert_eq!(words("Content ideas \u{b7} Impact vs. effort for Q3"), pair("IMPACT", "EFFORT"));
        assert_eq!(words("Backlog by customer value versus build cost"), pair("CUSTOMER VALUE", "BUILD COST"));
        assert_eq!(words("Box of tricks"), None);
        assert_eq!(words("Our roadmap"), None);
    }

    #[test]
    fn a_title_without_a_separator_draws_no_axis_words() {
        let mut untitled = four_quadrants();
        untitled.title = "Our roadmap".into();
        let scene = build_scene(&untitled, &Params::default());
        assert!(scene.decor.iter().all(|figure| !matches!(figure, Figure::Text(text) if text.size == AXIS_SIZE)));
    }

    #[test]
    fn items_stay_inside_their_cell_and_off_the_axes() {
        let scene = build_scene(&four_quadrants(), &Params::default());
        let cross = axis_cross(&scene);
        for (section, group) in scene.sections.iter().zip(&four_quadrants().groups) {
            let cell = section.figures[0].bounds();
            for node in scene.nodes.iter().filter(|node| group.members.contains(&node.id)) {
                let rect = node.rect;
                assert!(rect.x > cell.x && rect.right() < cell.right() && rect.y > cell.y && rect.bottom() < cell.bottom(), "{}", node.id);
                assert!(rect.x > cross.x || rect.right() < cross.x, "{} on the vertical axis", node.id);
                assert!(rect.y > cross.y || rect.bottom() < cross.y, "{} on the horizontal axis", node.id);
            }
        }
    }

    #[test]
    fn items_and_tags_never_overlap() {
        let scene = build_scene(&four_quadrants(), &Params::default());
        let tags: Vec<Rect> = scene.sections.iter().map(|section| section.figures[1].bounds()).collect();
        for (index, node) in scene.nodes.iter().enumerate() {
            assert!(scene.nodes[index + 1..].iter().all(|other| !node.rect.overlaps(&other.rect)), "{}", node.id);
            assert!(tags.iter().all(|tag| !node.rect.overlaps(tag)), "{} under a tag", node.id);
        }
    }

    #[test]
    fn an_item_in_no_group_sits_in_the_unplaced_strip_under_the_grid() {
        let scene = build_scene(&four_quadrants(), &Params::default());
        assert!(node(&scene, "h").rect.y > grid_bottom(&scene));
        assert!(scene.decor.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == UNPLACED)));
    }

    #[test]
    fn only_the_focal_item_is_in_the_accent_colour() {
        let mut two_focal = four_quadrants();
        two_focal.nodes[2].focal = true;
        let scene = build_scene(&two_focal, &Params::default());
        let accent = |node: &SceneNode| node.figures.iter().any(|figure| matches!(figure, Figure::Circle { paint, .. } if paint.fill == Tone::Accent));
        let accented: Vec<&str> = scene.nodes.iter().filter(|node| accent(node)).map(|node| node.id.as_str()).collect();
        assert_eq!(accented, vec!["a"]);
    }

    #[test]
    fn edges_are_not_drawn() {
        let mut linked = four_quadrants();
        linked.edges = serde_json::from_value(serde_json::json!([{"source": "a", "target": "b"}])).expect("edges");
        assert!(build_scene(&linked, &Params::default()).edges.is_empty());
    }
}
