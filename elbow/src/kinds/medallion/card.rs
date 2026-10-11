//! A medallion tier card: the tier name in a tinted header band, the bucket under it, and labelled field
//! rows (tool, format, writer, example) from the label's bracket items, the example row at the card's foot.

use std::f64::consts::{FRAC_PI_2, PI};

use crate::figure::{Anchor, Figure, Paint, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::NodeContent;
use crate::text::{mono_width, sans_width, split_in_two};

pub const MIN_WIDTH: f64 = 172.0;
pub const FOCAL_STROKE: f64 = 1.6;
pub const COLD_DASH: &str = "5,3";
const MIN_HEIGHT: f64 = 200.0;
const INSET: f64 = 16.0;
const RADIUS: f64 = 6.0;
const CORNER_STEPS: usize = 4;
const TITLE_SIZE: f64 = 13.0;
const TITLE_LINE: f64 = 16.0;
const TITLE_BASELINE: f64 = 25.0;
const BAND_ROOM: f64 = 24.0;
const BUCKET_SIZE: f64 = 9.0;
const BUCKET_DROP: f64 = 22.0;
const ROWS_DROP: f64 = 40.0;
const EYEBROW_SIZE: f64 = 8.0;
const EYEBROW_TRACKING: f64 = 0.14;
const VALUE_SIZE: f64 = 11.0;
const VALUE_DROP: f64 = 16.0;
const VALUE_LINE: f64 = 14.0;
const DESCENT: f64 = 4.0;
const ROW_GAP: f64 = 12.0;
const ROW_LABELS: [&str; 4] = ["TOOL", "FORMAT", "WRITER", "EXAMPLE"];
const EXAMPLE: &str = "EXAMPLE";

/// The look of a tier: the default card, the dashed cold tier (an archive), or the focal tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Default,
    Cold,
    Focal,
}

impl Style {
    pub fn card_paint(self) -> Paint {
        match self {
            Style::Default => Paint::shape(Tone::Fill, Tone::Ink, 1.0),
            Style::Cold => Paint::shape(Tone::Wash(0.03), Tone::Muted, 1.0).dashed(COLD_DASH),
            Style::Focal => Paint::shape(Tone::AccentTint, Tone::Accent, FOCAL_STROKE),
        }
    }

    fn band(self) -> Tone {
        match self {
            Style::Default => Tone::Wash(0.06),
            Style::Cold => Tone::MutedWash(0.18),
            Style::Focal => Tone::AccentWash(0.14),
        }
    }

    fn highlight(self) -> Tone {
        if self == Style::Focal {
            Tone::Accent
        } else {
            Tone::Muted
        }
    }
}

/// One labelled field row: an eyebrow and its value, wrapped onto as many lines as it needs.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub label: &'static str,
    pub values: Vec<String>,
}

impl Row {
    fn height(&self) -> f64 {
        EYEBROW_SIZE + VALUE_DROP + self.values.len().saturating_sub(1) as f64 * VALUE_LINE + DESCENT
    }
}

pub struct Card {
    pub title: Vec<String>,
    pub bucket: Option<String>,
    pub rows: Vec<Row>,
    pub style: Style,
}

impl Card {
    /// The card of a tier node: its name, and its bracket items as the bucket and the field rows.
    pub fn of(content: &NodeContent, style: Style) -> Card {
        let (bucket, items) = match content.fields.split_first() {
            Some((bucket, items)) => (Some(bucket.clone()), items),
            None => (None, &[][..]),
        };
        Card { title: wrapped(&content.lines.join(" "), TITLE_SIZE), bucket, rows: field_rows(items), style }
    }

    fn band_height(&self) -> f64 {
        BAND_ROOM + self.title.len() as f64 * TITLE_LINE
    }

    /// The height the card's content needs, at least the tall-card minimum.
    pub fn height(&self) -> f64 {
        let rows: f64 = self.rows.iter().map(|row| row.height() + ROW_GAP).sum();
        snap_up(self.band_height() + ROWS_DROP + rows - ROW_GAP + INSET).max(MIN_HEIGHT)
    }

    /// The width the widest line needs, at least the standard tier width.
    pub fn width(&self) -> f64 {
        let titles = self.title.iter().map(|line| sans_width(line, TITLE_SIZE));
        let bucket = self.bucket.iter().map(|bucket| mono_width(bucket, BUCKET_SIZE, 0.0));
        let values = self.rows.iter().flat_map(|row| &row.values).map(|value| sans_width(value, VALUE_SIZE));
        let widest = titles.chain(bucket).chain(values).fold(0.0, f64::max);
        snap_up(widest + 2.0 * INSET).max(MIN_WIDTH)
    }

    /// The card drawn in `rect`; the first figure is the card itself.
    pub fn figures(&self, rect: Rect) -> Vec<Figure> {
        let band = Rect::new(rect.x, rect.y, rect.w, self.band_height());
        let mut figures = vec![
            Figure::rect(rect, RADIUS, self.style.card_paint()),
            Figure::Polygon { points: rounded_top(band, RADIUS), paint: Paint::filled(self.style.band()) },
        ];
        figures.extend(self.title.iter().enumerate().map(|(index, line)| {
            let at = Point::new(rect.cx(), rect.y + TITLE_BASELINE + index as f64 * TITLE_LINE);
            Figure::Text(Text::sans(at, line.clone(), TITLE_SIZE).weight(600))
        }));
        figures.extend(self.bucket.iter().map(|bucket| {
            let at = Point::new(rect.cx(), band.bottom() + BUCKET_DROP);
            Figure::Text(Text::mono(at, bucket.clone(), BUCKET_SIZE).tone(self.style.highlight()))
        }));
        let mut top = band.bottom() + ROWS_DROP;
        for row in &self.rows {
            let row_top = if row.label == EXAMPLE { rect.bottom() - INSET - row.height() } else { top };
            figures.extend(self.row_figures(row, rect.x + INSET, row_top));
            top += row.height() + ROW_GAP;
        }
        figures
    }

    fn row_figures(&self, row: &Row, x: f64, top: f64) -> Vec<Figure> {
        let eyebrow = Text::mono(Point::new(x, top + EYEBROW_SIZE), row.label, EYEBROW_SIZE).tracking(EYEBROW_TRACKING).anchor(Anchor::Start);
        let tone = if row.label == EXAMPLE { self.style.highlight() } else { Tone::Ink };
        let values = row.values.iter().enumerate().map(move |(index, value)| {
            let at = Point::new(x, top + EYEBROW_SIZE + VALUE_DROP + index as f64 * VALUE_LINE);
            Text::sans(at, value.clone(), VALUE_SIZE).anchor(Anchor::Start).tone(tone)
        });
        std::iter::once(eyebrow).chain(values).map(Figure::Text).collect()
    }
}

/// The field rows of a tier's items after the bucket, labelled in order; items past the fourth continue
/// the example row.
pub fn field_rows(items: &[String]) -> Vec<Row> {
    let (named, extra) = items.split_at(items.len().min(ROW_LABELS.len()));
    let mut rows: Vec<Row> = ROW_LABELS.iter().zip(named).map(|(&label, value)| Row { label, values: wrapped(value, VALUE_SIZE) }).collect();
    if let Some(example) = rows.last_mut().filter(|row| row.label == EXAMPLE) {
        example.values.extend(extra.iter().flat_map(|value| wrapped(value, VALUE_SIZE)));
    }
    rows
}

fn wrapped(text: &str, size: f64) -> Vec<String> {
    if sans_width(text, size) <= MIN_WIDTH - 2.0 * INSET {
        return vec![text.to_string()];
    }
    split_in_two(text).map_or_else(|| vec![text.to_string()], |(first, second)| vec![first, second])
}

/// The outline of `rect` with its two top corners rounded by `radius`, as a polygon.
fn rounded_top(rect: Rect, radius: f64) -> Vec<Point> {
    let left = corner(Point::new(rect.x + radius, rect.y + radius), radius, PI, FRAC_PI_2);
    let right = corner(Point::new(rect.right() - radius, rect.y + radius), radius, FRAC_PI_2, 0.0);
    std::iter::once(Point::new(rect.x, rect.bottom()))
        .chain(left)
        .chain(right)
        .chain(std::iter::once(Point::new(rect.right(), rect.bottom())))
        .collect()
}

fn corner(centre: Point, radius: f64, from: f64, to: f64) -> impl Iterator<Item = Point> {
    (0..=CORNER_STEPS).map(move |step| {
        let angle = from + (to - from) * step as f64 / CORNER_STEPS as f64;
        Point::new(rounded(centre.x + radius * angle.cos()), rounded(centre.y - radius * angle.sin()))
    })
}

fn rounded(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{content, Links};
    use crate::spec::{Kind, NodeSpec, Shape};

    fn tier(label: &str) -> Card {
        let spec = NodeSpec { id: "tier".into(), label: label.into(), shape: Shape::Service, focal: false };
        Card::of(&content(&spec, Kind::Medallion, Links::default()), Style::Default)
    }

    fn row(label: &'static str, value: &str) -> Row {
        Row { label, values: vec![value.to_string()] }
    }

    #[test]
    fn the_first_bracket_item_is_the_bucket_and_the_rest_are_field_rows_in_order() {
        let card = tier("Raw (raw-bucket; NiFi; CSV · Parquet; Data Engineer)");
        assert_eq!(card.title, vec!["Raw".to_string()]);
        assert_eq!(card.bucket.as_deref(), Some("raw-bucket"));
        assert_eq!(card.rows, vec![row("TOOL", "NiFi"), row("FORMAT", "CSV · Parquet"), row("WRITER", "Data Engineer")]);
    }

    #[test]
    fn items_past_the_fourth_continue_the_example_row() {
        let card = tier("Raw (raw-bucket, NiFi, JSON, Data Engineer, page views, cart events)");
        let example = card.rows.last().expect("an example row");
        assert_eq!(card.rows.len(), 4);
        assert_eq!(example.label, "EXAMPLE");
        assert_eq!(example.values, vec!["page views".to_string(), "cart events".to_string()]);
    }

    #[test]
    fn a_tier_without_a_bracket_has_no_bucket_and_no_rows() {
        let card = tier("Archive");
        assert_eq!(card.bucket, None);
        assert!(card.rows.is_empty());
        assert_eq!(card.height(), MIN_HEIGHT);
    }

    #[test]
    fn a_long_value_wraps_onto_two_lines_and_the_card_grows() {
        let short = tier("Raw (raw; NiFi)");
        let long = tier("Raw (raw; NiFi ingest with schema registry checks)");
        assert_eq!(long.rows[0].values.len(), 2);
        assert!(long.height() >= short.height());
    }

    #[test]
    fn the_example_row_sits_at_the_foot_of_the_card() {
        let card = tier("Raw (raw-bucket; NiFi; JSON; Data Engineer; page views)");
        let rect = Rect::new(0.0, 0.0, MIN_WIDTH, 320.0);
        let figures = card.figures(rect);
        let lowest = figures.iter().map(|figure| figure.bounds().bottom()).filter(|&bottom| bottom < rect.bottom()).fold(0.0, f64::max);
        assert!(rect.bottom() - lowest <= INSET + DESCENT,"the example ends {} above the foot", rect.bottom() - lowest);
    }

    #[test]
    fn the_header_band_keeps_inside_the_rounded_card() {
        let points = rounded_top(Rect::new(0.0, 0.0, 100.0, 40.0), RADIUS);
        assert!(points.iter().all(|point| point.x >= 0.0 && point.x <= 100.0 && point.y >= 0.0 && point.y <= 40.0));
        assert!(points.iter().all(|point| point.y > 0.0 || (point.x >= RADIUS - 0.01 && point.x <= 100.0 - RADIUS + 0.01)));
    }
}
