//! Database schema: tables with typed column rows, constraint chips and an index compartment, laid out in
//! layers, with every foreign key running from its column row in the child table to the referenced row
//! of the parent table.

use std::collections::HashMap;

use super::uml_class::relabel;
use crate::figure::{Anchor, Figure, LegendKey, Marker, Paint, Section, Swatch, Text, Tone};
use crate::check::too_close_parallel;
use crate::geom::{segments, simplify, snap, snap_up, Point, Rect};
use crate::node::{NodeContent, Treatment, NAME_SIZE};
use crate::params::{Direction, Params};
use crate::scene::{known_edges, layered_with, unique_nodes, Scene, Zone};
use crate::spec::View;
use crate::text::{mono_width, sans_width};

const HEADER: f64 = 28.0;
const HEADER_BASELINE: f64 = 18.0;
const ROW: f64 = 24.0;
const ROW_BASELINE: f64 = 16.0;
const INSET: f64 = 12.0;
const COLUMN_GAP: f64 = 16.0;
const MIN_WIDTH: f64 = 240.0;
const MAX_ROWS: usize = 8;
const CHIPS: [&str; 4] = ["PK", "FK", "UQ", "NN"];
const CHIP_WIDTH: f64 = 20.0;
const CHIP_HEIGHT: f64 = 12.0;
const CHIP_GAP: f64 = 4.0;
const CHIP_SIZE: f64 = 8.0;
const TABLE_TAG: &str = "TABLE";
const TAG_WIDTH: f64 = 40.0;
const TYPE_SIZE: f64 = 9.0;
const INDEX_PREFIXES: [&str; 2] = ["idx_", "uq_"];
const INDEX_EYEBROW: &str = "INDEXES";
const INDEX_TOP: f64 = 20.0;
const INDEX_LINE: f64 = 16.0;
const SHARED_ROW_STEP: f64 = 16.0;
const LOOP_OUT: f64 = 16.0;
const TRACK_STEPS: [f64; 6] = [12.0, -12.0, 24.0, -24.0, 36.0, -36.0];
const TRACK_MARGIN: f64 = 16.0;
const CASCADE: &str = "CASCADE";
const LINE_WIDTH: f64 = 1.0;
const FOCAL_LINE_WIDTH: f64 = 1.2;
const SCHEMA_LABEL_SIZE: f64 = 8.0;
const SCHEMA_LABEL_TRACKING: f64 = 0.14;

/// One column row: `name type [CHIPS]`, the chips being the trailing PK/FK/UQ/NN words.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub sql_type: String,
    pub chips: Vec<String>,
}

impl Column {
    pub fn parse(item: &str) -> Column {
        let mut words: Vec<&str> = item.split_whitespace().collect();
        let mut chips = Vec::new();
        while words.len() > 1 && words.last().is_some_and(|word| CHIPS.contains(word)) {
            chips.insert(0, words.pop().unwrap_or_default().to_string());
        }
        let name = words.first().map_or_else(String::new, |word| word.to_string());
        let sql_type = words.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
        Column { name, sql_type, chips }
    }

    fn has(&self, chip: &str) -> bool {
        self.chips.iter().any(|own| own == chip)
    }

    fn chips_width(&self) -> f64 {
        self.chips.len() as f64 * (CHIP_WIDTH + CHIP_GAP)
    }
}

/// A table box's content: its name, its columns and the indexes worth naming.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub indexes: Vec<String>,
}

impl Table {
    /// The table a node's name and bracket items describe; items starting with `idx_` or `uq_` are indexes.
    pub fn of(content: &NodeContent) -> Table {
        let (indexes, columns): (Vec<&String>, Vec<&String>) = content.fields.iter().partition(|item| is_index(item));
        Table {
            name: content.lines.join(" "),
            columns: columns.into_iter().map(|item| Column::parse(item)).collect(),
            indexes: indexes.into_iter().cloned().collect(),
        }
    }

    fn short_name(&self) -> &str {
        self.name.rsplit('.').next().unwrap_or(&self.name)
    }

    fn overflows(&self) -> bool {
        self.columns.len() > MAX_ROWS
    }

    /// The columns with a row of their own; past eight columns the last row says how many more there are.
    fn shown_columns(&self) -> &[Column] {
        let count = if self.overflows() { MAX_ROWS - 1 } else { self.columns.len() };
        &self.columns[..count]
    }

    fn rows(&self) -> usize {
        self.columns.len().min(MAX_ROWS)
    }

    /// The row a column is drawn in: its own, or the overflow row for a hidden one.
    fn row_of(&self, column: usize) -> usize {
        column.min(self.rows().saturating_sub(1))
    }

    fn name_column(&self) -> f64 {
        self.shown_columns().iter().map(|column| sans_width(&column.name, NAME_SIZE)).fold(0.0, f64::max)
    }

    fn chip_left(&self) -> f64 {
        snap_up(INSET + self.name_column() + COLUMN_GAP)
    }

    fn width(&self) -> f64 {
        let chips = self.shown_columns().iter().map(Column::chips_width).fold(0.0, f64::max);
        let types = self.shown_columns().iter().map(|column| mono_width(&column.sql_type, TYPE_SIZE, 0.0)).fold(0.0, f64::max);
        let rows = self.chip_left() + chips + COLUMN_GAP + types + INSET;
        let header = INSET + sans_width(&self.name, NAME_SIZE) + COLUMN_GAP + TAG_WIDTH + INSET;
        let indexes = self.indexes.iter().map(|index| mono_width(index, TYPE_SIZE, 0.0) + 2.0 * INSET).fold(0.0, f64::max);
        snap_up(rows.max(header).max(indexes).max(MIN_WIDTH))
    }

    fn index_height(&self) -> f64 {
        if self.indexes.is_empty() {
            0.0
        } else {
            INDEX_TOP + self.indexes.len() as f64 * INDEX_LINE
        }
    }

    fn height(&self) -> f64 {
        HEADER + self.rows().max(1) as f64 * ROW + self.index_height()
    }

    /// The vertical centre of a column's row in the table drawn at `rect`.
    fn row_centre(&self, rect: &Rect, column: Option<usize>) -> f64 {
        match column {
            Some(column) => rect.y + HEADER + self.row_of(column) as f64 * ROW + ROW / 2.0,
            None => rect.y + HEADER / 2.0,
        }
    }

    fn column_named(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
    }

    /// The column a foreign key to `parent` starts from: the first FK column that names the parent, else
    /// the first FK column, else the first column.
    fn foreign_key_to(&self, parent: &Table) -> Option<usize> {
        let forms = name_forms(parent.short_name());
        let names_parent = |column: &Column| forms.iter().any(|form| column.name.to_lowercase().contains(form.as_str()));
        self.columns
            .iter()
            .position(|column| column.has("FK") && names_parent(column))
            .or_else(|| self.columns.iter().position(|column| column.has("FK")))
            .or_else(|| (!self.columns.is_empty()).then_some(0))
    }

    /// The column foreign keys point at: the first PK column, else the first column.
    fn referenced(&self) -> Option<usize> {
        self.columns.iter().position(|column| column.has("PK")).or_else(|| (!self.columns.is_empty()).then_some(0))
    }
}

fn is_index(item: &str) -> bool {
    INDEX_PREFIXES.iter().any(|prefix| item.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix)))
}

/// A table's short name and its singular forms, so `customer_id` names `customers` and `category_id`
/// names `categories`.
fn name_forms(short: &str) -> Vec<String> {
    let lower = short.to_lowercase();
    let mut forms = vec![lower.clone()];
    forms.extend(lower.strip_suffix("ies").map(|stem| format!("{stem}y")));
    forms.extend(lower.strip_suffix("es").map(str::to_string));
    forms.extend(lower.strip_suffix('s').map(str::to_string));
    forms.retain(|form| form.len() >= 2);
    forms
}

/// What a foreign-key edge joins: the child's column, the parent's referenced column, and the label shown.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignKey {
    pub column: Option<usize>,
    pub referenced: Option<usize>,
    pub text: String,
}

impl ForeignKey {
    /// Reads an (uppercase) edge label: a first word naming a child column is the FK column, the rest the label.
    pub fn of(text: &str, child: &Table, parent: &Table) -> ForeignKey {
        let referenced = parent.referenced();
        let (first, rest) = text.split_once(' ').unwrap_or((text, ""));
        match child.column_named(first) {
            Some(column) => ForeignKey { column: Some(column), referenced, text: rest.trim().to_string() },
            None => ForeignKey { column: child.foreign_key_to(parent), referenced, text: text.to_string() },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Left,
    Right,
}

/// Where one end of a foreign key attaches: table, side and row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Attach {
    node: usize,
    side: Side,
    row: Option<usize>,
}

struct Anchors {
    start: Attach,
    end: Attach,
    start_y: f64,
    end_y: f64,
}

/// Lays out `view` as this kind.
pub fn build(view: &View, params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let tables: Vec<Table> = contents.iter().map(Table::of).collect();
    let sized: Vec<NodeContent> = contents.into_iter().zip(&tables).map(|(content, table)| sized(content, table)).collect();
    let mut scene = layered_with(view, &Params { direction: Some(Direction::Right), ..*params }, sized);
    let inputs = known_edges(view, &ids);
    let cascade = inputs.iter().find(|edge| edge.text.contains(CASCADE));
    let texts: HashMap<&str, &str> = inputs.iter().map(|edge| (edge.id.as_str(), edge.text.as_str())).collect();
    let keys: Vec<ForeignKey> = scene
        .edges
        .iter()
        .map(|edge| {
            let text = texts.get(edge.id.as_str()).copied().unwrap_or_default();
            ForeignKey::of(text, &tables[edge.source], &tables[edge.target])
        })
        .collect();
    anchor(&mut scene, &tables, &keys);
    let focal = cascade.map(|edge| edge.id.as_str());
    for edge in &mut scene.edges {
        edge.focal = focal == Some(edge.id.as_str());
        edge.markers = Some((Marker::None, Marker::Arrow));
        edge.paint = Some(if edge.focal { Paint::line(Tone::Accent, FOCAL_LINE_WIDTH) } else { Paint::line(Tone::Muted, LINE_WIDTH) });
    }
    scene.sections = schema_sections(std::mem::take(&mut scene.zones));
    let schema_marks: Vec<Rect> = scene.sections.iter().flat_map(schema_marks).collect();
    let no_trims = vec![(0.0, 0.0); scene.edges.len()];
    relabel(&mut scene, keys.into_iter().map(|key| key.text).collect(), &no_trims, &schema_marks);
    let cascaded = cascade.map(|edge| edge.source);
    for (index, (node, table)) in scene.nodes.iter_mut().zip(&tables).enumerate() {
        node.figures = table_figures(table, node.rect, cascaded == Some(index));
    }
    scene.legend = Some(legend(!scene.sections.is_empty(), cascaded.is_some()));
    scene.measured()
}

fn sized(content: NodeContent, table: &Table) -> NodeContent {
    NodeContent { lines: vec![table.name.clone()], width: table.width(), height: table.height(), treatment: Treatment::Service, ..content }
}

/// Moves the ends of every foreign key onto the centres of its column rows, on the sides the layered
/// routing chose, spreading ends that share a row and side 16px apart.
fn anchor(scene: &mut Scene, tables: &[Table], keys: &[ForeignKey]) {
    let mut anchors: Vec<Anchors> = scene
        .edges
        .iter()
        .zip(keys)
        .map(|(edge, key)| {
            let (source, target) = (scene.nodes[edge.source].rect, scene.nodes[edge.target].rect);
            let looped = edge.source == edge.target;
            let start_side = if looped { Side::Right } else { side_of(&source, edge.points[0]) };
            let end_side = if looped { Side::Right } else { side_of(&target, edge.points[edge.points.len() - 1]) };
            Anchors {
                start: Attach { node: edge.source, side: start_side, row: key.column.map(|column| tables[edge.source].row_of(column)) },
                end: Attach { node: edge.target, side: end_side, row: key.referenced.map(|column| tables[edge.target].row_of(column)) },
                start_y: tables[edge.source].row_centre(&source, key.column),
                end_y: tables[edge.target].row_centre(&target, key.referenced),
            }
        })
        .collect();
    spread_shared_rows(&mut anchors);
    let mut paths: Vec<Vec<Point>> = scene
        .edges
        .iter()
        .zip(&anchors)
        .map(|(edge, anchors)| {
            if edge.source == edge.target {
                self_loop(scene.nodes[edge.source].rect.right(), anchors.start_y, anchors.end_y)
            } else {
                anchored(&edge.points, anchors.start_y, anchors.end_y)
            }
        })
        .collect();
    separate_tracks(&mut paths);
    for (edge, path) in scene.edges.iter_mut().zip(paths) {
        edge.points = path;
    }
}

/// Moves every vertical run that now lies on top of another connector's run to a free track beside it,
/// inside its channel.
fn separate_tracks(paths: &mut [Vec<Point>]) {
    for edge in 0..paths.len() {
        for run in 1..paths[edge].len().saturating_sub(2) {
            if !clashes(paths, edge, &paths[edge]) {
                break;
            }
            let free = TRACK_STEPS.iter().filter_map(|&step| shifted(&paths[edge], run, step)).find(|moved| !clashes(paths, edge, moved));
            if let Some(moved) = free {
                paths[edge] = moved;
            }
        }
    }
}

fn shifted(path: &[Point], run: usize, step: f64) -> Option<Vec<Point>> {
    let (top, bottom) = (path[run], path[run + 1]);
    let x = top.x + step;
    let (before, after) = (path[run - 1].x, path[run + 2].x);
    let inside = x >= before.min(after) + TRACK_MARGIN && x <= before.max(after) - TRACK_MARGIN;
    if (top.x - bottom.x).abs() > 0.01 || !inside {
        return None;
    }
    let mut moved = path.to_vec();
    moved[run].x = x;
    moved[run + 1].x = x;
    Some(moved)
}

fn clashes(paths: &[Vec<Point>], edge: usize, path: &[Point]) -> bool {
    let own = segments(path);
    paths
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != edge)
        .any(|(_, other)| segments(other).iter().any(|theirs| own.iter().any(|mine| too_close_parallel(mine, theirs))))
}

fn side_of(rect: &Rect, point: Point) -> Side {
    if (point.x - rect.x).abs() < (point.x - rect.right()).abs() {
        Side::Left
    } else {
        Side::Right
    }
}

fn spread_shared_rows(anchors: &mut [Anchors]) {
    let mut shared: HashMap<Attach, Vec<(usize, bool, f64)>> = HashMap::new();
    for (index, own) in anchors.iter().enumerate() {
        shared.entry(own.start).or_default().push((index, true, own.end_y));
        shared.entry(own.end).or_default().push((index, false, own.start_y));
    }
    for mut ends in shared.into_values().filter(|ends| ends.len() > 1) {
        ends.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        let middle = (ends.len() - 1) as f64 / 2.0;
        for (order, (index, is_start, _)) in ends.into_iter().enumerate() {
            let offset = (order as f64 - middle) * SHARED_ROW_STEP;
            if is_start {
                anchors[index].start_y += offset;
            } else {
                anchors[index].end_y += offset;
            }
        }
    }
}

/// `path` with its first run moved to `start_y` and its last run to `end_y`, keeping it orthogonal;
/// a straight path gets a bend halfway.
fn anchored(path: &[Point], start_y: f64, end_y: f64) -> Vec<Point> {
    let (first, last) = (path[0], path[path.len() - 1]);
    let mut points = if path.len() < 4 {
        let middle = snap((first.x + last.x) / 2.0);
        vec![first, Point::new(middle, first.y), Point::new(middle, last.y), last]
    } else {
        path.to_vec()
    };
    let count = points.len();
    points[0].y = start_y;
    points[1].y = start_y;
    points[count - 2].y = end_y;
    points[count - 1].y = end_y;
    simplify(&points)
}

fn self_loop(side: f64, start_y: f64, end_y: f64) -> Vec<Point> {
    let out = side + LOOP_OUT;
    vec![Point::new(side, start_y), Point::new(out, start_y), Point::new(out, end_y), Point::new(side, end_y)]
}

fn table_figures(table: &Table, rect: Rect, cascaded: bool) -> Vec<Figure> {
    let mut figures = vec![Figure::rect(rect, 6.0, Paint::shape(Tone::Fill, Tone::Ink, 1.0))];
    figures.extend(header_figures(table, rect, cascaded));
    let chip_left = rect.x + table.chip_left();
    for (row, column) in table.shown_columns().iter().enumerate() {
        let top = rect.y + HEADER + row as f64 * ROW;
        figures.extend(row_tint(rect, row));
        figures.push(Figure::Text(Text::sans(Point::new(rect.x + INSET, top + ROW_BASELINE), column.name.as_str(), NAME_SIZE).anchor(Anchor::Start)));
        for (index, chip) in column.chips.iter().enumerate() {
            figures.extend(chip_figures(chip, Point::new(chip_left + index as f64 * (CHIP_WIDTH + CHIP_GAP), top + (ROW - CHIP_HEIGHT) / 2.0)));
        }
        let type_at = Point::new(rect.right() - INSET, top + ROW_BASELINE);
        figures.push(Figure::Text(Text::mono(type_at, column.sql_type.as_str(), TYPE_SIZE).anchor(Anchor::End)));
    }
    if table.overflows() {
        let row = MAX_ROWS - 1;
        let top = rect.y + HEADER + row as f64 * ROW;
        let more = format!("+ {} more columns", table.columns.len() - row);
        figures.extend(row_tint(rect, row));
        figures.push(Figure::Text(Text::mono(Point::new(rect.x + INSET, top + ROW_BASELINE), more, TYPE_SIZE).anchor(Anchor::Start)));
    }
    figures.extend(index_figures(table, rect));
    figures
}

fn header_figures(table: &Table, rect: Rect, cascaded: bool) -> Vec<Figure> {
    let tint = if cascaded { Tone::AccentTint } else { Tone::Wash(0.04) };
    let tag = Rect::new(rect.right() - INSET - TAG_WIDTH, rect.y + 8.0, TAG_WIDTH, CHIP_HEIGHT);
    vec![
        Figure::rect(Rect::new(rect.x, rect.y, rect.w, HEADER), 6.0, Paint::filled(tint)),
        Figure::rect(Rect::new(rect.x, rect.y + HEADER - 8.0, rect.w, 8.0), 0.0, Paint::filled(tint)),
        Figure::line(Point::new(rect.x, rect.y + HEADER), Point::new(rect.right(), rect.y + HEADER), Paint::line(Tone::Wash(0.22), 1.0)),
        Figure::Text(Text::sans(Point::new(rect.x + INSET, rect.y + HEADER_BASELINE), table.name.as_str(), NAME_SIZE).weight(600).anchor(Anchor::Start)),
        Figure::rect(tag, 2.0, Paint::line(Tone::Wash(0.4), 0.8)),
        Figure::Text(Text::mono(Point::new(tag.cx(), tag.y + 9.0), TABLE_TAG, CHIP_SIZE).tracking(0.08).tone(Tone::Wash(0.8))),
    ]
}

fn row_tint(rect: Rect, row: usize) -> Option<Figure> {
    (row % 2 == 0).then(|| Figure::rect(Rect::new(rect.x, rect.y + HEADER + row as f64 * ROW, rect.w, ROW), 0.0, Paint::filled(Tone::Wash(0.02))))
}

fn chip_figures(chip: &str, at: Point) -> [Figure; 2] {
    let rect = Rect::new(at.x, at.y, CHIP_WIDTH, CHIP_HEIGHT);
    [
        Figure::rect(rect, 2.0, Paint::line(Tone::Wash(0.35), 0.8)),
        Figure::Text(Text::mono(Point::new(rect.cx(), rect.y + 9.0), chip, CHIP_SIZE).tone(Tone::Wash(0.75))),
    ]
}

fn index_figures(table: &Table, rect: Rect) -> Vec<Figure> {
    if table.indexes.is_empty() {
        return Vec::new();
    }
    let top = rect.y + HEADER + table.rows() as f64 * ROW;
    let mut figures = vec![
        Figure::line(Point::new(rect.x, top), Point::new(rect.right(), top), Paint::line(Tone::Wash(0.12), 0.8)),
        Figure::Text(Text::mono(Point::new(rect.x + INSET, top + 14.0), INDEX_EYEBROW, 8.0).anchor(Anchor::Start).tracking(0.14)),
    ];
    for (line, index) in table.indexes.iter().enumerate() {
        let at = Point::new(rect.x + INSET, top + INDEX_TOP + 12.0 + line as f64 * INDEX_LINE);
        figures.push(Figure::Text(Text::mono(at, index.as_str(), TYPE_SIZE).anchor(Anchor::Start)));
    }
    figures
}

/// Every schema group the layered layout could draw as a zone, as a dashed containment section.
fn schema_sections(zones: Vec<Zone>) -> Vec<Section> {
    zones
        .into_iter()
        .map(|zone| {
            let label = Text::mono(Point::new(zone.rect.x + INSET, zone.rect.y + 13.0), zone.label, SCHEMA_LABEL_SIZE)
                .anchor(Anchor::Start)
                .tracking(SCHEMA_LABEL_TRACKING);
            let area = Paint::shape(Tone::Wash(0.02), Tone::Wash(0.20), 0.8).dashed("4,4");
            Section { id: zone.id, figures: vec![Figure::rect(zone.rect, 8.0, area), Figure::Text(label)] }
        })
        .collect()
}

/// What an edge label must not cover on a schema section: its name.
fn schema_marks(section: &Section) -> Vec<Rect> {
    section.figures.iter().skip(1).map(Figure::bounds).collect()
}

fn legend(schemas: bool, cascade: bool) -> Vec<LegendKey> {
    let mut keys = vec![LegendKey::new(Swatch::Box(Paint::shape(Tone::Fill, Tone::Ink, 1.0)), "TABLE")];
    if schemas {
        keys.push(LegendKey::new(Swatch::Box(Paint::shape(Tone::Wash(0.02), Tone::Wash(0.35), 0.8).dashed("3,3")), "SCHEMA"));
    }
    keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, LINE_WIDTH), Marker::Arrow), "FOREIGN KEY"));
    if cascade {
        keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::Accent, FOCAL_LINE_WIDTH), Marker::Arrow), "ON DELETE CASCADE"));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;
    use crate::spec::{EdgeSpec, GroupSpec, Kind, NodeSpec};

    fn table(label: &str) -> Table {
        let spec = NodeSpec { id: "t".into(), label: label.into(), shape: Default::default(), focal: false };
        Table::of(&crate::node::content(&spec, Kind::DbSchema, Default::default()))
    }

    fn node(id: &str, label: &str) -> NodeSpec {
        NodeSpec { id: id.into(), label: label.into(), shape: Default::default(), focal: true }
    }

    fn edge(source: &str, target: &str, label: &str) -> EdgeSpec {
        EdgeSpec { id: format!("{source}->{target}"), source: source.into(), target: target.into(), label: label.into(), style: Default::default(), focal: true }
    }

    fn shop() -> View {
        View {
            kind: Kind::DbSchema,
            nodes: vec![
                node("customers", "public.customers (id uuid PK, email text UQ NN)"),
                node("orders", "public.orders (id uuid PK, customer_id uuid FK NN, total numeric(12,2), idx_orders_customer)"),
                node("items", "public.order_items (id uuid PK, order_id uuid FK NN, qty integer NN)"),
                node("invoices", "billing.invoices (id uuid PK, order_id uuid FK NN)"),
            ],
            edges: vec![
                edge("orders", "customers", "ON DELETE RESTRICT"),
                edge("items", "orders", "order_id ON DELETE CASCADE"),
                edge("invoices", "orders", "on delete restrict"),
            ],
            groups: vec![GroupSpec { id: "billing".into(), label: "billing".into(), members: vec!["invoices".into()] }],
            ..View::default()
        }
    }

    fn rect_of(scene: &Scene, id: &str) -> Rect {
        scene.nodes.iter().find(|node| node.id == id).expect("a table").rect
    }

    fn scene_edge<'a>(scene: &'a Scene, id: &str) -> &'a crate::scene::SceneEdge {
        scene.edges.iter().find(|edge| edge.id == id).expect("a known edge")
    }

    #[test]
    fn a_column_splits_into_name_type_and_trailing_chips() {
        assert_eq!(
            Column::parse("customer_id uuid FK NN"),
            Column { name: "customer_id".into(), sql_type: "uuid".into(), chips: vec!["FK".into(), "NN".into()] }
        );
        assert_eq!(Column::parse("total numeric(12,2)"), Column { name: "total".into(), sql_type: "numeric(12,2)".into(), chips: vec![] });
        assert_eq!(Column::parse("created_at timestamp with time zone NN").sql_type, "timestamp with time zone");
    }

    #[test]
    fn index_items_go_to_the_indexes_compartment() {
        let orders = table("public.orders (id uuid PK, total numeric(12,2) NN, idx_orders_customer, uq_orders_number)");
        assert_eq!(orders.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), vec!["id", "total"]);
        assert_eq!(orders.columns[1].sql_type, "numeric(12,2)");
        assert_eq!(orders.indexes, vec!["idx_orders_customer", "uq_orders_number"]);
        assert_eq!(orders.height(), HEADER + 2.0 * ROW + INDEX_TOP + 2.0 * INDEX_LINE);
    }

    #[test]
    fn more_than_eight_columns_end_in_an_overflow_row() {
        let wide = table("public.products (a int, b int, c int, d int, e int, f int, g int, h int, i int, j int)");
        assert_eq!(wide.shown_columns().len(), 7);
        assert_eq!(wide.rows(), 8);
        assert_eq!(wide.row_of(9), 7);
        let figures = table_figures(&wide, Rect::new(0.0, 0.0, wide.width(), wide.height()), false);
        assert!(figures.iter().any(|figure| matches!(figure, Figure::Text(text) if text.text == "+ 3 more columns")));
    }

    #[test]
    fn the_fk_column_comes_from_the_label_or_names_the_parent() {
        let customers = table("public.customers (id uuid PK, email text)");
        let categories = table("public.categories (code text, id uuid PK)");
        let orders = table("public.orders (id uuid PK, category_id uuid FK, customer_id uuid FK NN)");
        assert_eq!(ForeignKey::of("CUSTOMER_ID ON DELETE CASCADE", &orders, &customers), ForeignKey { column: Some(2), referenced: Some(0), text: "ON DELETE CASCADE".into() });
        assert_eq!(ForeignKey::of("ON DELETE SET NULL", &orders, &customers).column, Some(2));
        assert_eq!(ForeignKey::of("", &orders, &categories), ForeignKey { column: Some(1), referenced: Some(1), text: String::new() });
        let loose = table("public.notes (id uuid PK, body text)");
        assert_eq!(ForeignKey::of("ON DELETE RESTRICT", &loose, &customers).column, Some(0));
        let unkeyed = table("public.stats (day date, hits int, ref_id uuid FK)");
        assert_eq!(ForeignKey::of("", &unkeyed, &customers).column, Some(2));
        assert_eq!(ForeignKey::of("", &customers, &unkeyed).referenced, Some(0));
    }

    #[test]
    fn a_connector_starts_and_ends_at_its_column_row_centres() {
        let scene = build(&shop(), &Params::default());
        let order_ref = scene_edge(&scene, "orders->customers");
        let (orders, customers) = (rect_of(&scene, "orders"), rect_of(&scene, "customers"));
        assert_eq!(order_ref.points[0].y, orders.y + HEADER + ROW + ROW / 2.0);
        assert_eq!(order_ref.points.last().map(|point| point.y), Some(customers.y + HEADER + ROW / 2.0));
        assert!(order_ref.points.windows(2).all(|pair| pair[0].x == pair[1].x || pair[0].y == pair[1].y));
        assert!(check(&scene).0.is_empty(), "{:#?}", check(&scene).0);
    }

    #[test]
    fn two_keys_on_one_row_and_side_are_offset_eight_px_each_way() {
        let scene = build(&shop(), &Params::default());
        let orders = rect_of(&scene, "orders");
        let centre = orders.y + HEADER + ROW / 2.0;
        let mut ends: Vec<f64> = ["items->orders", "invoices->orders"].iter().map(|id| scene_edge(&scene, id).points.last().expect("points").y).collect();
        ends.sort_by(f64::total_cmp);
        assert_eq!(ends, vec![centre - 8.0, centre + 8.0]);
    }

    #[test]
    fn the_first_cascade_is_the_accent_and_tints_only_the_header_it_cascades_into() {
        let scene = build(&shop(), &Params::default());
        let accent: Vec<&str> = scene.edges.iter().filter(|edge| edge.paint.is_some_and(|paint| paint.stroke == Tone::Accent)).map(|edge| edge.id.as_str()).collect();
        assert_eq!(accent, vec!["items->orders"]);
        assert_eq!(scene_edge(&scene, "items->orders").label.as_ref().map(|label| label.text.as_str()), Some("ON DELETE CASCADE"));
        let tinted = |id: &str| {
            let node = scene.nodes.iter().find(|node| node.id == id).expect("a table");
            node.figures.iter().any(|figure| matches!(figure, Figure::Rect { paint, .. } if paint.fill == Tone::AccentTint))
        };
        assert!(tinted("items"));
        assert!(!tinted("orders") && !tinted("customers") && !tinted("invoices"));
        let items = scene.nodes.iter().find(|node| node.id == "items").expect("items");
        assert!(matches!(items.figures[0], Figure::Rect { paint, .. } if paint.fill == Tone::Fill && paint.stroke == Tone::Ink));
    }

    #[test]
    fn without_a_cascade_nothing_is_accent() {
        let mut calm = shop();
        calm.edges[1].label = "order_id ON DELETE RESTRICT".into();
        let scene = build(&calm, &Params::default());
        assert!(scene.edges.iter().all(|edge| edge.paint.is_some_and(|paint| paint.stroke == Tone::Muted)));
        let tints = scene.nodes.iter().flat_map(|node| &node.figures).filter(|figure| matches!(figure, Figure::Rect { paint, .. } if paint.fill == Tone::AccentTint));
        assert_eq!(tints.count(), 0);
    }

    #[test]
    fn a_schema_group_is_a_dashed_section_around_its_tables() {
        let scene = build(&shop(), &Params::default());
        assert_eq!(scene.sections.len(), 1);
        let section = &scene.sections[0];
        assert_eq!(section.id, "billing");
        let Figure::Rect { rect, paint, .. } = section.figures[0] else { panic!("a rect first") };
        assert_eq!(paint.dash, Some("4,4"));
        let invoices = rect_of(&scene, "invoices");
        assert!(rect.x < invoices.x && rect.right() > invoices.right() && rect.y < invoices.y && rect.bottom() > invoices.bottom());
    }

    #[test]
    fn a_run_left_on_another_connectors_track_moves_to_a_free_one() {
        let mut paths = vec![
            vec![Point::new(0.0, 100.0), Point::new(48.0, 100.0), Point::new(48.0, 200.0), Point::new(120.0, 200.0)],
            vec![Point::new(0.0, 124.0), Point::new(48.0, 124.0), Point::new(48.0, 20.0), Point::new(120.0, 20.0)],
        ];
        separate_tracks(&mut paths);
        assert!(!clashes(&paths, 0, &paths[0]));
        assert!(paths.iter().all(|path| path[1].x == path[2].x && path[1].y == path[0].y));
    }

    #[test]
    fn a_self_reference_loops_out_of_the_right_side_between_its_rows() {
        let view = View {
            kind: Kind::DbSchema,
            nodes: vec![node("staff", "public.staff (id uuid PK, name text, manager_id uuid FK)")],
            edges: vec![edge("staff", "staff", "manager_id ON DELETE SET NULL")],
            ..View::default()
        };
        let scene = build(&view, &Params::default());
        let staff = rect_of(&scene, "staff");
        let points = &scene.edges[0].points;
        assert_eq!((points[0].x, points[0].y), (staff.right(), staff.y + HEADER + 2.0 * ROW + ROW / 2.0));
        assert_eq!(points.last().map(|point| (point.x, point.y)), Some((staff.right(), staff.y + HEADER + ROW / 2.0)));
    }
}
