//! Drawing primitives for the kinds that are not boxes and elbow connectors: dots, circles, trapezoids,
//! axes, curves and free text. Colours are tones of the theme palette, resolved when the SVG is written.

use crate::geom::{Point, Rect};
use crate::text::{mono_width, sans_width};

/// A colour of the theme palette. `Wash`, `MutedWash` and `AccentWash` are the ink, muted and accent
/// colours at the given opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    None,
    Transparent,
    Paper,
    Fill,
    Ink,
    Muted,
    Soft,
    Accent,
    AccentTint,
    Link,
    Wash(f64),
    MutedWash(f64),
    AccentWash(f64),
}

/// Fill, stroke, stroke width and dash pattern of a shape or line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    pub fill: Tone,
    pub stroke: Tone,
    pub width: f64,
    pub dash: Option<&'static str>,
}

impl Paint {
    /// A filled shape without a stroke.
    pub fn filled(fill: Tone) -> Paint {
        Paint { fill, stroke: Tone::None, width: 0.0, dash: None }
    }

    /// A line or an unfilled outline.
    pub fn line(stroke: Tone, width: f64) -> Paint {
        Paint { fill: Tone::None, stroke, width, dash: None }
    }

    /// A filled shape with an outline.
    pub fn shape(fill: Tone, stroke: Tone, width: f64) -> Paint {
        Paint { fill, stroke, width, dash: None }
    }

    pub fn dashed(self, dash: &'static str) -> Paint {
        Paint { dash: Some(dash), ..self }
    }
}

/// The end of a line or edge. `Arrow` takes the line's stroke colour; the UML ends are drawn in ink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Marker {
    #[default]
    None,
    Arrow,
    Triangle,
    Diamond,
    HollowDiamond,
    Open,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Sans,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// One line of text; `at` is the baseline point the anchor refers to.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub at: Point,
    pub text: String,
    pub size: f64,
    pub font: Font,
    pub weight: u16,
    pub anchor: Anchor,
    pub tone: Tone,
    pub tracking: f64,
    pub italic: bool,
}

impl Text {
    /// Geist text in ink, regular weight, centred on `at`.
    pub fn sans(at: Point, text: impl Into<String>, size: f64) -> Text {
        Text { at, text: text.into(), size, font: Font::Sans, weight: 400, anchor: Anchor::Middle, tone: Tone::Ink, tracking: 0.0, italic: false }
    }

    /// Geist Mono text in muted ink, centred on `at`.
    pub fn mono(at: Point, text: impl Into<String>, size: f64) -> Text {
        Text { font: Font::Mono, tone: Tone::Muted, ..Text::sans(at, text, size) }
    }

    pub fn weight(self, weight: u16) -> Text {
        Text { weight, ..self }
    }

    pub fn anchor(self, anchor: Anchor) -> Text {
        Text { anchor, ..self }
    }

    pub fn tone(self, tone: Tone) -> Text {
        Text { tone, ..self }
    }

    pub fn tracking(self, tracking: f64) -> Text {
        Text { tracking, ..self }
    }

    pub fn italic(self) -> Text {
        Text { italic: true, ..self }
    }

    pub fn width(&self) -> f64 {
        match self.font {
            Font::Sans => sans_width(&self.text, self.size),
            Font::Mono => mono_width(&self.text, self.size, self.tracking),
        }
    }

    /// The box the text covers: from its cap height down to its baseline plus a small descent.
    pub fn bounds(&self) -> Rect {
        let width = self.width();
        let left = match self.anchor {
            Anchor::Start => self.at.x,
            Anchor::Middle => self.at.x - width / 2.0,
            Anchor::End => self.at.x - width,
        };
        Rect::new(left, self.at.y - self.size, width, self.size * 1.25)
    }
}

/// A drawn primitive in page coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Figure {
    Rect { rect: Rect, radius: f64, paint: Paint },
    Circle { centre: Point, radius: f64, paint: Paint },
    Polygon { points: Vec<Point>, paint: Paint },
    Line { points: Vec<Point>, paint: Paint, end: Marker },
    Text(Text),
}

impl Figure {
    pub fn rect(rect: Rect, radius: f64, paint: Paint) -> Figure {
        Figure::Rect { rect, radius, paint }
    }

    pub fn circle(centre: Point, radius: f64, paint: Paint) -> Figure {
        Figure::Circle { centre, radius, paint }
    }

    pub fn line(a: Point, b: Point, paint: Paint) -> Figure {
        Figure::Line { points: vec![a, b], paint, end: Marker::None }
    }

    /// An invisible rectangle that still takes clicks: the hit area of a node or section drawn as
    /// something other than a box.
    pub fn hit_area(rect: Rect) -> Figure {
        Figure::Rect { rect, radius: 0.0, paint: Paint::filled(Tone::Transparent) }
    }

    pub fn bounds(&self) -> Rect {
        match self {
            Figure::Rect { rect, .. } => *rect,
            Figure::Circle { centre, radius, .. } => Rect::new(centre.x - radius, centre.y - radius, 2.0 * radius, 2.0 * radius),
            Figure::Polygon { points, .. } | Figure::Line { points, .. } => bounding_points(points),
            Figure::Text(text) => text.bounds(),
        }
    }

    pub fn shift(&mut self, dx: f64, dy: f64) {
        let move_point = |point: &mut Point| *point = Point::new(point.x + dx, point.y + dy);
        match self {
            Figure::Rect { rect, .. } => *rect = Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
            Figure::Circle { centre, .. } => move_point(centre),
            Figure::Polygon { points, .. } | Figure::Line { points, .. } => points.iter_mut().for_each(move_point),
            Figure::Text(text) => move_point(&mut text.at),
        }
    }
}

fn bounding_points(points: &[Point]) -> Rect {
    let (low_x, low_y, high_x, high_y) = points.iter().fold(
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        |(low_x, low_y, high_x, high_y), point| (low_x.min(point.x), low_y.min(point.y), high_x.max(point.x), high_y.max(point.y)),
    );
    if low_x.is_finite() {
        Rect::new(low_x, low_y, high_x - low_x, high_y - low_y)
    } else {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    }
}

/// The bounding box of several figures; None when there are none.
pub fn bounds_of<'a>(figures: impl IntoIterator<Item = &'a Figure>) -> Option<Rect> {
    figures.into_iter().map(Figure::bounds).reduce(|a, b| {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
    })
}

/// A selectable area that is not a node: a column, a set circle, a band. The first figure is its
/// shape, the one the selection and pointer looks change.
#[derive(Clone, Debug)]
pub struct Section {
    pub id: String,
    pub figures: Vec<Figure>,
}

/// The sample in a legend entry.
#[derive(Clone, Debug)]
pub enum Swatch {
    Box(Paint),
    Dot(Paint),
    Line(Paint, Marker),
}

/// One legend entry for a kind that names its own looks.
#[derive(Clone, Debug)]
pub struct LegendKey {
    pub swatch: Swatch,
    pub text: String,
}

impl LegendKey {
    pub fn new(swatch: Swatch, text: impl Into<String>) -> LegendKey {
        LegendKey { swatch, text: text.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_text_covers_both_sides_of_its_anchor() {
        let text = Text::mono(Point::new(100.0, 50.0), "ABCD", 10.0);
        let bounds = text.bounds();
        assert_eq!(bounds.cx(), 100.0);
        assert_eq!(bounds.w, 24.0);
        assert!(bounds.y < 50.0 && bounds.bottom() > 50.0);
    }

    #[test]
    fn a_shifted_polygon_keeps_its_shape() {
        let mut figure = Figure::Polygon { points: vec![Point::new(0.0, 0.0), Point::new(10.0, 20.0)], paint: Paint::filled(Tone::Ink) };
        figure.shift(5.0, 5.0);
        assert_eq!(figure.bounds(), Rect::new(5.0, 5.0, 10.0, 20.0));
    }
}
