//! Entity relationships: a cardinality written into an edge label ("1:N", "0..1 : 1..*", "one-to-many")
//! becomes a small mark on each end of the line, next to its entity, and the rest of the label stays the
//! relationship name.

use crate::geom::{segments, Point, Rect};
use crate::scene::LabelBox;
use crate::text::mono_width;

pub const MARK_SIZE: f64 = 9.0;
const MARK_PADDING: f64 = 3.0;
const MARK_HEIGHT: f64 = 12.0;
const MARK_OFFSETS: [f64; 3] = [6.0, 14.0, 22.0];
const NODE_CLEARANCE: f64 = 2.0;
const LINE_CLEARANCE: f64 = 1.0;
const TRIM: &[char] = &[' ', '-', '–', '·', ',', ':', ';'];

/// The cardinality at the source and at the target end of a relationship.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ends {
    pub source: String,
    pub target: String,
}

/// Splits an (uppercase) edge label into the relationship name and its cardinality, if it has one.
pub fn split(text: &str) -> (String, Option<Ends>) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let Some((at, length, ends)) = ratio(&words).or_else(|| phrase(&words)) else { return (text.to_string(), None) };
    let rest: Vec<&str> = words[..at].iter().chain(&words[at + length..]).copied().collect();
    (rest.join(" ").replace("()", "").trim_matches(TRIM).to_string(), Some(ends))
}

fn bare(word: &str) -> &str {
    word.trim_matches(|c| matches!(c, '(' | ')' | ',' | '[' | ']'))
}

fn is_amount(text: &str) -> bool {
    let single = |part: &str| part.len() == 1 && part.chars().all(|c| c.is_ascii_digit() || matches!(c, 'N' | 'M' | '*'));
    match text.split_once("..") {
        Some((low, high)) => single(low) && single(high),
        None => single(text),
    }
}

fn ratio(words: &[&str]) -> Option<(usize, usize, Ends)> {
    let ends = |source: &str, target: &str| Ends { source: source.to_string(), target: target.to_string() };
    for (at, word) in words.iter().enumerate() {
        if let Some((source, target)) = bare(word).split_once(':').filter(|(a, b)| is_amount(a) && is_amount(b)) {
            return Some((at, 1, ends(source, target)));
        }
        let around = (at.checked_sub(1).map(|before| bare(words[before])), words.get(at + 1).map(|after| bare(after)));
        if let (":", (Some(source), Some(target))) = (*word, around) {
            if is_amount(source) && is_amount(target) {
                return Some((at - 1, 3, ends(source, target)));
            }
        }
    }
    None
}

fn amount_word(word: &str) -> Option<&'static str> {
    match word {
        "ONE" | "1" => Some("1"),
        "MANY" | "N" | "*" => Some("N"),
        _ => None,
    }
}

fn phrase(words: &[&str]) -> Option<(usize, usize, Ends)> {
    let ends = |source: &'static str, target: &'static str| {
        let target = if source == "N" && target == "N" { "M" } else { target };
        Ends { source: source.to_string(), target: target.to_string() }
    };
    for at in 0..words.len() {
        let parts: Vec<&str> = bare(words[at]).split('-').collect();
        if let [source, "TO", target] = parts[..] {
            if let (Some(source), Some(target)) = (amount_word(source), amount_word(target)) {
                return Some((at, 1, ends(source, target)));
            }
        }
        if let (Some(source), Some(&"TO"), Some(target)) = (amount_word(bare(words[at])), words.get(at + 1), words.get(at + 2)) {
            if let Some(target) = amount_word(bare(target)) {
                return Some((at, 3, ends(source, target)));
            }
        }
    }
    None
}

/// The two marks of every edge with ends, on the line next to the entity, clear of nodes, other
/// lines and marks already placed.
pub fn place(ends: &[Option<Ends>], paths: &[Vec<Point>], nodes: &[Rect]) -> Vec<Vec<LabelBox>> {
    let mut marks = Vec::with_capacity(ends.len());
    let mut taken: Vec<Rect> = Vec::new();
    for (edge, own) in ends.iter().enumerate() {
        let path = &paths[edge];
        let Some(own) = own.as_ref().filter(|_| path.len() >= 2) else {
            marks.push(Vec::new());
            continue;
        };
        let last = path.len() - 1;
        let mut placed = Vec::with_capacity(2);
        for (text, end, next) in [(&own.source, path[0], path[1]), (&own.target, path[last], path[last - 1])] {
            let width = mono_width(text, MARK_SIZE, 0.0) + 2.0 * MARK_PADDING;
            let spots = MARK_OFFSETS.map(|offset| on_line(end, next, offset, width));
            let rect = spots
                .iter()
                .copied()
                .min_by_key(|spot| conflicts(spot, edge, paths, nodes, &taken))
                .unwrap_or(spots[0]);
            taken.push(rect);
            placed.push(LabelBox { rect, text: text.clone() });
        }
        marks.push(placed);
    }
    marks
}

fn on_line(end: Point, next: Point, offset: f64, width: f64) -> Rect {
    if (end.y - next.y).abs() < 0.01 {
        let x = if next.x > end.x { end.x + offset } else { end.x - offset - width };
        Rect::new(x, end.y - MARK_HEIGHT / 2.0, width, MARK_HEIGHT)
    } else {
        let y = if next.y > end.y { end.y + offset } else { end.y - offset - MARK_HEIGHT };
        Rect::new(end.x - width / 2.0, y, width, MARK_HEIGHT)
    }
}

fn conflicts(mark: &Rect, edge: usize, paths: &[Vec<Point>], nodes: &[Rect], taken: &[Rect]) -> usize {
    let on_nodes = nodes.iter().filter(|rect| mark.inflate(NODE_CLEARANCE).overlaps(rect)).count();
    let on_marks = taken.iter().filter(|rect| mark.inflate(NODE_CLEARANCE).overlaps(rect)).count();
    let on_lines: usize = paths
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != edge)
        .map(|(_, path)| segments(path).iter().filter(|segment| mark.distance_to(segment) < LINE_CLEARANCE).count())
        .sum();
    on_nodes * 4 + on_marks * 4 + on_lines * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends(source: &str, target: &str) -> Option<Ends> {
        Some(Ends { source: source.into(), target: target.into() })
    }

    #[test]
    fn a_ratio_is_split_from_the_relationship_name() {
        assert_eq!(split("PLACES 1:N"), ("PLACES".to_string(), ends("1", "N")));
        assert_eq!(split("HAS (0..1:1..*)"), ("HAS".to_string(), ends("0..1", "1..*")));
        assert_eq!(split("1 : N"), (String::new(), ends("1", "N")));
    }

    #[test]
    fn a_spoken_cardinality_is_understood() {
        assert_eq!(split("CONTAINS ONE-TO-MANY"), ("CONTAINS".to_string(), ends("1", "N")));
        assert_eq!(split("MANY TO MANY"), (String::new(), ends("N", "M")));
    }

    #[test]
    fn a_label_without_cardinality_stays_as_it_is() {
        assert_eq!(split("BELONGS TO"), ("BELONGS TO".to_string(), None));
        assert_eq!(split("AT 10:30"), ("AT 10:30".to_string(), None));
    }

    #[test]
    fn a_mark_sits_on_its_line_next_to_the_entity() {
        let path = vec![Point::new(100.0, 50.0), Point::new(200.0, 50.0)];
        let marks = place(&[ends("1", "N")], &[path], &[]);
        assert_eq!((marks[0][0].rect.x, marks[0][0].rect.y), (106.0, 44.0));
        assert!(marks[0][1].rect.right() <= 194.0);
    }
}
