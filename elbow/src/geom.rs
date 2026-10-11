//! Points, rectangles and axis-aligned segments.

pub const GRID: f64 = 4.0;
const EPS: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn cx(&self) -> f64 {
        self.x + self.w / 2.0
    }


    pub fn inflate(&self, by: f64) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }

    /// True when the two rectangles share an area, not just an edge.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right() - EPS
            && other.x < self.right() - EPS
            && self.y < other.bottom() - EPS
            && other.y < self.bottom() - EPS
    }

    /// True when the segment runs through the inside of the rectangle, not only along or up to its border.
    pub fn crossed_by(&self, segment: &Segment) -> bool {
        let inner = self.inflate(-EPS * 10.0);
        let (lo, hi) = segment.span();
        if segment.is_horizontal() {
            let y = segment.a.y;
            y > inner.y && y < inner.bottom() && lo < inner.right() && hi > inner.x
        } else {
            let x = segment.a.x;
            x > inner.x && x < inner.right() && lo < inner.bottom() && hi > inner.y
        }
    }

    /// The shortest distance between the rectangle and the segment; 0 when they touch or overlap.
    pub fn distance_to(&self, segment: &Segment) -> f64 {
        let (lo, hi) = segment.span();
        let (dx, dy) = if segment.is_horizontal() {
            (gap(lo, hi, self.x, self.right()), gap_point(segment.a.y, self.y, self.bottom()))
        } else {
            (gap_point(segment.a.x, self.x, self.right()), gap(lo, hi, self.y, self.bottom()))
        };
        (dx * dx + dy * dy).sqrt()
    }
}

fn gap(lo: f64, hi: f64, start: f64, end: f64) -> f64 {
    (start - hi).max(lo - end).max(0.0)
}

fn gap_point(value: f64, start: f64, end: f64) -> f64 {
    gap(value, value, start, end)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

impl Segment {
    pub fn new(a: Point, b: Point) -> Self {
        Self { a, b }
    }

    pub fn is_horizontal(&self) -> bool {
        (self.a.y - self.b.y).abs() < EPS
    }

    pub fn is_vertical(&self) -> bool {
        (self.a.x - self.b.x).abs() < EPS
    }

    pub fn length(&self) -> f64 {
        (self.a.x - self.b.x).abs() + (self.a.y - self.b.y).abs()
    }

    /// The extent along the segment's own axis, lowest first.
    pub fn span(&self) -> (f64, f64) {
        if self.is_horizontal() {
            (self.a.x.min(self.b.x), self.a.x.max(self.b.x))
        } else {
            (self.a.y.min(self.b.y), self.a.y.max(self.b.y))
        }
    }

    pub fn mid(&self) -> Point {
        Point::new((self.a.x + self.b.x) / 2.0, (self.a.y + self.b.y) / 2.0)
    }

    /// Where a horizontal and a vertical segment cross inside both (not at an end of either).
    pub fn crossing(&self, other: &Segment) -> Option<Point> {
        let (h, v) = match (self.is_horizontal(), other.is_horizontal()) {
            (true, false) => (self, other),
            (false, true) => (other, self),
            _ => return None,
        };
        let (h_lo, h_hi) = h.span();
        let (v_lo, v_hi) = v.span();
        let x = v.a.x;
        let y = h.a.y;
        let inside = x > h_lo + 1.0 && x < h_hi - 1.0 && y > v_lo + 1.0 && y < v_hi - 1.0;
        inside.then(|| Point::new(x, y))
    }
}

/// The consecutive segments of a polyline.
pub fn segments(points: &[Point]) -> Vec<Segment> {
    points.windows(2).map(|pair| Segment::new(pair[0], pair[1])).collect()
}

/// Drops repeated points and points in the middle of a straight run.
pub fn simplify(points: &[Point]) -> Vec<Point> {
    let mut kept: Vec<Point> = Vec::with_capacity(points.len());
    for &point in points {
        if kept.last().is_some_and(|last| close(*last, point)) {
            continue;
        }
        if kept.len() >= 2 {
            let before = kept[kept.len() - 2];
            let last = kept[kept.len() - 1];
            if collinear(before, last, point) {
                kept.pop();
            }
        }
        kept.push(point);
    }
    kept
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS
}

fn collinear(a: Point, b: Point, c: Point) -> bool {
    let same_x = (a.x - b.x).abs() < EPS && (b.x - c.x).abs() < EPS;
    let same_y = (a.y - b.y).abs() < EPS && (b.y - c.y).abs() < EPS;
    same_x || same_y
}

pub fn snap(value: f64) -> f64 {
    (value / GRID).round() * GRID
}

pub fn snap_up(value: f64) -> f64 {
    (value / GRID).ceil() * GRID
}
