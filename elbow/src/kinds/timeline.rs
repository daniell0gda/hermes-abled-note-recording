//! Timeline: events in spoken order on a horizontal baseline. Each event is a dot with its date under
//! the baseline and its name above or below, in turn; a milestone (focal node) gets an accent dot and a
//! bold name. Events are spaced by their dates when every date names a year, else evenly. Groups are
//! periods: brackets under the lowest labels. A timeline has no connectors, so view edges are not drawn.

use crate::figure::{bounds_of, Figure, LegendKey, Marker, Paint, Section, Swatch, Text, Tone};
use crate::geom::{snap_up, Point, Rect};
use crate::node::{name_and_sublabel, NodeContent, Treatment};
use crate::params::Params;
use crate::scene::{unique_nodes, Scene, SceneNode};
use crate::spec::{GroupSpec, View};
use crate::text::{mono_width, sans_width, split_in_two};

const BASELINE: f64 = 0.0;
const DOT: f64 = 4.0;
const MILESTONE_DOT: f64 = 6.0;
const TICK: f64 = 8.0;
const BASELINE_OVERHANG: f64 = 32.0;
const DROP_ABOVE: f64 = 40.0;
const DROP_BELOW: f64 = 48.0;
const NAME_GAP_ABOVE: f64 = 6.0;
const NAME_GAP_BELOW: f64 = 14.0;
const LINE: f64 = 16.0;
const NAME_SIZE: f64 = 12.0;
const MILESTONE_SIZE: f64 = 14.0;
const ONE_LINE_LIMIT: f64 = 140.0;
const DATE_SIZE: f64 = 8.0;
const DATE_TRACKING: f64 = 0.14;
const DATE_BASELINE: f64 = 22.0;
const DATE_CLEARANCE: f64 = 3.0;
const MIN_STEP: f64 = 48.0;
const DATE_GAP: f64 = 16.0;
const LABEL_GAP: f64 = 24.0;
const MIN_SPAN: f64 = 720.0;
const MAX_SPAN: f64 = 1200.0;
const HIT_PADDING: f64 = 4.0;
const PERIOD_TOP: f64 = 28.0;
const PERIOD_ROW: f64 = 32.0;
const PERIOD_PADDING: f64 = 8.0;
const PERIOD_GAP: f64 = 16.0;
const PERIOD_TICK: f64 = 6.0;
const PERIOD_LABEL_SIZE: f64 = 8.0;
const PERIOD_LABEL_TRACKING: f64 = 0.14;
const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// How the events are spread along the baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spacing {
    ByDate,
    InOrder,
}

/// Which side of the baseline an event's name sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Above,
    Below,
}

struct Event {
    lines: Vec<String>,
    date: Option<String>,
    moment: Option<f64>,
    milestone: bool,
}

impl Event {
    fn new(label: &str, content: &NodeContent) -> Event {
        let (name, bracket) = name_and_sublabel(label);
        Event {
            lines: name_lines(&name),
            moment: bracket.as_deref().and_then(moment),
            date: bracket.map(|date| date.to_uppercase()),
            milestone: content.treatment == Treatment::Focal,
        }
    }

    fn name_size(&self) -> f64 {
        if self.milestone {
            MILESTONE_SIZE
        } else {
            NAME_SIZE
        }
    }

    fn name_width(&self) -> f64 {
        self.lines.iter().map(|line| sans_width(line, self.name_size())).fold(0.0, f64::max)
    }

    fn date_width(&self) -> f64 {
        self.date.as_deref().map_or(0.0, |date| mono_width(date, DATE_SIZE, DATE_TRACKING))
    }

    fn dot(&self) -> f64 {
        if self.milestone {
            MILESTONE_DOT
        } else {
            DOT
        }
    }

    fn tone(&self) -> Tone {
        if self.milestone {
            Tone::Accent
        } else {
            Tone::Ink
        }
    }
}

fn name_lines(name: &str) -> Vec<String> {
    if sans_width(name, NAME_SIZE) <= ONE_LINE_LIMIT {
        return vec![name.to_string()];
    }
    split_in_two(name).map_or_else(|| vec![name.to_string()], |(first, second)| vec![first, second])
}

/// The point in time a date names, in years: the 4-digit year plus the quarter, half or month when
/// the date gives one (`2021 Q3`, `H2 2023`, `March 2024`, `2024-03`). None without a year.
fn moment(date: &str) -> Option<f64> {
    let words: Vec<String> = date.split(|c: char| !c.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect();
    let year_at = words.iter().position(|word| word.len() == 4 && word.chars().all(|c| c.is_ascii_digit()))?;
    let year: f64 = words[year_at].parse().ok()?;
    let part = words.iter().enumerate().filter(|(index, _)| *index != year_at).find_map(|(index, word)| year_part(word, index > year_at));
    Some(year + part.unwrap_or(0.0))
}

fn year_part(word: &str, after_year: bool) -> Option<f64> {
    let numbered = |prefix: char, count: u32| {
        let rest = word.strip_prefix(prefix)?;
        let number: u32 = rest.parse().ok()?;
        (1..=count).contains(&number).then(|| f64::from(number - 1) / f64::from(count))
    };
    let month_name = || MONTHS.iter().position(|month| word.len() >= 3 && word.starts_with(month)).map(|month| month as f64 / 12.0);
    let month_number = || {
        let number: u32 = word.parse().ok().filter(|_| after_year && word.len() <= 2)?;
        (1..=12).contains(&number).then(|| f64::from(number - 1) / 12.0)
    };
    numbered('q', 4).or_else(|| numbered('h', 2)).or_else(month_name).or_else(month_number)
}

fn side_of(index: usize) -> Side {
    if index % 2 == 0 {
        Side::Above
    } else {
        Side::Below
    }
}

fn date_need(first: &Event, second: &Event) -> f64 {
    ((first.date_width() + second.date_width()) / 2.0 + DATE_GAP).max(MIN_STEP)
}

fn same_side_need(first: &Event, second: &Event) -> f64 {
    (first.name_width() + second.name_width()) / 2.0 + LABEL_GAP
}

fn even_step(events: &[Event]) -> f64 {
    let neighbours = events.windows(2).map(|pair| date_need(&pair[0], &pair[1]));
    let same_side = events.windows(3).map(|three| same_side_need(&three[0], &three[2]) / 2.0);
    snap_up(neighbours.chain(same_side).fold(MIN_STEP, f64::max))
}

fn moments(events: &[Event]) -> Option<Vec<f64>> {
    let moments: Vec<f64> = events.iter().map(|event| event.moment).collect::<Option<Vec<f64>>>()?;
    let low = moments.iter().copied().fold(f64::INFINITY, f64::min);
    let high = moments.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (moments.len() >= 2 && high > low).then_some(moments)
}

/// The x of every event and how they were spread: by date, with a minimum gap so neither the dates
/// nor the names on one side collide, or evenly in spoken order.
fn positions(events: &[Event]) -> (Vec<f64>, Spacing) {
    let step = even_step(events);
    match moments(events) {
        Some(moments) => (dated_positions(events, &moments, step), Spacing::ByDate),
        None => ((0..events.len()).map(|index| index as f64 * step).collect(), Spacing::InOrder),
    }
}

/// Pixels per year: enough for the closest dates to keep their gap, at least the even layout's width
/// (and `MIN_SPAN`) and at most `MAX_SPAN` across, beyond which the minimum gaps take over.
fn scale(events: &[Event], moments: &[f64], step: f64) -> f64 {
    let low = moments.iter().copied().fold(f64::INFINITY, f64::min);
    let high = moments.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let per_year = |need: f64, from: f64, to: f64| if to > from { need / (to - from) } else { 0.0 };
    let neighbours = (1..events.len()).map(|index| per_year(date_need(&events[index - 1], &events[index]), moments[index - 1], moments[index]));
    let same_side = (2..events.len()).map(|index| per_year(same_side_need(&events[index - 2], &events[index]), moments[index - 2], moments[index]));
    let wanted = neighbours.chain(same_side).fold(0.0, f64::max);
    let span = high - low;
    let even_width = ((events.len() - 1) as f64 * step).max(MIN_SPAN);
    wanted.min(MAX_SPAN / span).max(even_width / span)
}

fn dated_positions(events: &[Event], moments: &[f64], step: f64) -> Vec<f64> {
    let scale = scale(events, moments, step);
    let mut xs: Vec<f64> = Vec::with_capacity(events.len());
    for (index, event) in events.iter().enumerate() {
        let by_date = (moments[index] - moments[0]) * scale;
        let after_previous = index.checked_sub(1).map_or(f64::NEG_INFINITY, |previous| xs[previous] + date_need(&events[previous], event));
        let after_same_side = index.checked_sub(2).map_or(f64::NEG_INFINITY, |previous| xs[previous] + same_side_need(&events[previous], event));
        xs.push(snap_up(by_date.max(after_previous).max(after_same_side)));
    }
    xs
}

fn event_figures(event: &Event, x: f64, side: Side) -> Vec<Figure> {
    let drop = if event.milestone { Paint::line(Tone::Accent, 1.0) } else { Paint::line(Tone::Wash(0.30), 1.0) };
    let mut figures = drop_lines(event, x, side, drop);
    figures.push(Figure::circle(Point::new(x, BASELINE), event.dot(), Paint::filled(event.tone())));
    if let Some(date) = &event.date {
        let tone = if event.milestone { Tone::Accent } else { Tone::Muted };
        figures.push(Figure::Text(Text::mono(Point::new(x, BASELINE + DATE_BASELINE), date.clone(), DATE_SIZE).tracking(DATE_TRACKING).tone(tone)));
    }
    figures.extend(name_figures(event, x, side));
    figures
}

fn drop_lines(event: &Event, x: f64, side: Side, paint: Paint) -> Vec<Figure> {
    let at = |y: f64| Point::new(x, y);
    match side {
        Side::Above => vec![Figure::line(at(BASELINE - event.dot()), at(BASELINE - DROP_ABOVE), paint)],
        Side::Below if event.date.is_none() => vec![Figure::line(at(BASELINE + event.dot()), at(BASELINE + DROP_BELOW), paint)],
        Side::Below => {
            let date_top = BASELINE + DATE_BASELINE - DATE_SIZE - DATE_CLEARANCE;
            let date_bottom = BASELINE + DATE_BASELINE + DATE_SIZE / 4.0 + DATE_CLEARANCE;
            vec![
                Figure::line(at(BASELINE + event.dot()), at(date_top), paint),
                Figure::line(at(date_bottom), at(BASELINE + DROP_BELOW), paint),
            ]
        }
    }
}

fn name_figures(event: &Event, x: f64, side: Side) -> Vec<Figure> {
    let count = event.lines.len() as f64;
    let first_baseline = match side {
        Side::Above => BASELINE - DROP_ABOVE - NAME_GAP_ABOVE - (count - 1.0) * LINE,
        Side::Below => BASELINE + DROP_BELOW + NAME_GAP_BELOW,
    };
    let weight = if event.milestone { 600 } else { 500 };
    event
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let at = Point::new(x, first_baseline + index as f64 * LINE);
            Figure::Text(Text::sans(at, line.clone(), event.name_size()).weight(weight))
        })
        .collect()
}

fn grid_rect(rect: Rect) -> Rect {
    let x = -snap_up(-rect.x);
    let y = -snap_up(-rect.y);
    Rect::new(x, y, snap_up(rect.right()) - x, snap_up(rect.bottom()) - y)
}

fn event_node(id: String, content: NodeContent, event: &Event, x: f64, side: Side) -> SceneNode {
    let marks = event_figures(event, x, side);
    let covered = bounds_of(&marks).unwrap_or(Rect::new(x, BASELINE, 0.0, 0.0));
    let rect = grid_rect(covered.inflate(HIT_PADDING));
    let figures = std::iter::once(Figure::hit_area(rect)).chain(marks).collect();
    SceneNode { figures, ..SceneNode::new(id, rect, content) }
}

fn axis(xs: &[f64]) -> Vec<Figure> {
    let (Some(&first), Some(&last)) = (xs.first(), xs.last()) else { return Vec::new() };
    let (start, end) = (first - BASELINE_OVERHANG, last + BASELINE_OVERHANG);
    let tick = |x: f64| Figure::line(Point::new(x, BASELINE - TICK), Point::new(x, BASELINE + TICK), Paint::line(Tone::Wash(0.20), 1.0));
    let baseline = Figure::line(Point::new(start, BASELINE), Point::new(end, BASELINE), Paint::line(Tone::MutedWash(0.45), 1.0));
    std::iter::once(baseline).chain([start, end].into_iter().chain(xs.iter().copied()).map(tick)).collect()
}

/// The periods as selectable brackets under the lowest labels; brackets whose spans would touch go
/// on separate rows.
fn periods(view: &View, ids: &[String], xs: &[f64], top: f64) -> Vec<Section> {
    let mut rows: Vec<Vec<(f64, f64)>> = Vec::new();
    view.groups
        .iter()
        .filter_map(|group| {
            let (low, high, label) = period_span(group, ids, xs)?;
            let row = free_row(&mut rows, low, high);
            Some(period(group, low, high, label, top + row as f64 * PERIOD_ROW))
        })
        .collect()
}

fn period_span(group: &GroupSpec, ids: &[String], xs: &[f64]) -> Option<(f64, f64, String)> {
    let members: Vec<f64> = ids.iter().zip(xs).filter(|(id, _)| group.members.contains(id)).map(|(_, &x)| x).collect();
    let low = members.iter().copied().fold(f64::INFINITY, f64::min);
    let high = members.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if members.is_empty() {
        return None;
    }
    let label = if group.label.is_empty() { group.id.to_uppercase() } else { group.label.to_uppercase() };
    let half = ((high - low) / 2.0 + PERIOD_PADDING).max(mono_width(&label, PERIOD_LABEL_SIZE, PERIOD_LABEL_TRACKING) / 2.0 + PERIOD_PADDING);
    let middle = (low + high) / 2.0;
    Some((middle - half, middle + half, label))
}

fn free_row(rows: &mut Vec<Vec<(f64, f64)>>, low: f64, high: f64) -> usize {
    let clear = |row: &Vec<(f64, f64)>| row.iter().all(|&(start, end)| high + PERIOD_GAP <= start || end + PERIOD_GAP <= low);
    let row = rows.iter().position(clear).unwrap_or(rows.len());
    if row == rows.len() {
        rows.push(Vec::new());
    }
    rows[row].push((low, high));
    row
}

fn period(group: &GroupSpec, low: f64, high: f64, label: String, y: f64) -> Section {
    let bracket = vec![Point::new(low, y - PERIOD_TICK), Point::new(low, y), Point::new(high, y), Point::new(high, y - PERIOD_TICK)];
    let area = Rect::new(low - HIT_PADDING, y - PERIOD_TICK - HIT_PADDING, high - low + 2.0 * HIT_PADDING, PERIOD_ROW - HIT_PADDING);
    let text = Text::mono(Point::new((low + high) / 2.0, y + 14.0), label, PERIOD_LABEL_SIZE).tracking(PERIOD_LABEL_TRACKING);
    Section {
        id: group.id.clone(),
        figures: vec![
            Figure::hit_area(area),
            Figure::Line { points: bracket, paint: Paint::line(Tone::Muted, 1.0), end: Marker::None },
            Figure::Text(text),
        ],
    }
}

fn legend(events: &[Event], spacing: Spacing, has_periods: bool) -> Vec<LegendKey> {
    let mut keys = vec![LegendKey::new(Swatch::Dot(Paint::filled(Tone::Ink)), "EVENT")];
    if events.iter().any(|event| event.milestone) {
        keys.push(LegendKey::new(Swatch::Dot(Paint::filled(Tone::Accent)), "MILESTONE"));
    }
    if has_periods {
        keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::Muted, 1.0), Marker::None), "PERIOD"));
    }
    let scale = match spacing {
        Spacing::ByDate => "SPACED BY DATE",
        Spacing::InOrder => "IN ORDER, NOT TO SCALE",
    };
    keys.push(LegendKey::new(Swatch::Line(Paint::line(Tone::MutedWash(0.45), 1.0), Marker::None), scale));
    keys
}

fn label_of<'a>(view: &'a View, id: &str) -> &'a str {
    view.nodes.iter().find(|node| node.id == id).map_or("", |node| node.label.as_str())
}

/// Lays out `view` as a timeline.
pub fn build(view: &View, _params: &Params) -> Scene {
    let (ids, contents) = unique_nodes(view);
    let events: Vec<Event> = ids.iter().zip(&contents).map(|(id, content)| Event::new(label_of(view, id), content)).collect();
    let (xs, spacing) = positions(&events);
    let nodes: Vec<SceneNode> = ids
        .iter()
        .cloned()
        .zip(contents)
        .enumerate()
        .map(|(index, (id, content))| event_node(id, content, &events[index], xs[index], side_of(index)))
        .collect();
    let lowest = nodes.iter().map(|node| node.rect.bottom()).fold(BASELINE + DATE_BASELINE, f64::max);
    let sections = periods(view, &ids, &xs, snap_up(lowest + PERIOD_TOP));
    let legend = legend(&events, spacing, !sections.is_empty());
    Scene { nodes, sections, decor: axis(&xs), legend: Some(legend), ..Scene::empty(view.kind) }.measured()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{EdgeSpec, Kind, NodeSpec};

    fn node(id: &str, label: &str) -> NodeSpec {
        NodeSpec { id: id.into(), label: label.into(), shape: Default::default(), focal: false }
    }

    fn timeline(labels: &[&str]) -> View {
        let nodes = labels.iter().enumerate().map(|(index, label)| node(&format!("e{index}"), label)).collect();
        View { kind: Kind::Timeline, nodes, ..View::default() }
    }

    fn dot_x(scene: &Scene, index: usize) -> f64 {
        scene.nodes[index].figures.iter().find_map(|figure| match figure {
            Figure::Circle { centre, .. } => Some(centre.x),
            _ => None,
        }).expect("a dot")
    }

    fn name_text(node: &SceneNode) -> &Text {
        node.figures.iter().filter_map(|figure| match figure {
            Figure::Text(text) if text.font == crate::figure::Font::Sans => Some(text),
            _ => None,
        }).next().expect("a name")
    }

    #[test]
    fn a_date_with_a_year_becomes_a_point_in_time() {
        assert_eq!(moment("2019"), Some(2019.0));
        assert_eq!(moment("2021 Q3"), Some(2021.5));
        assert_eq!(moment("Q3 2021"), Some(2021.5));
        assert_eq!(moment("H2 2023"), Some(2023.5));
        assert_eq!(moment("March 2024"), Some(2024.0 + 2.0 / 12.0));
        assert_eq!(moment("2024-03-15"), Some(2024.0 + 2.0 / 12.0));
        assert_eq!(moment("Q3"), None);
        assert_eq!(moment("sprint 12"), None);
    }

    #[test]
    fn names_alternate_above_and_below_the_baseline() {
        let scene = build(&timeline(&["Kick-off (2019)", "Beta (2020)", "Launch (2021)", "Scale (2022)"]), &Params::default());
        let above: Vec<bool> = scene.nodes.iter().map(|node| name_text(node).at.y < BASELINE).collect();
        assert_eq!(above, vec![true, false, true, false]);
    }

    #[test]
    fn the_date_is_the_tick_label_under_the_baseline() {
        let scene = build(&timeline(&["Kick-off (2019)", "Beta (2020)"]), &Params::default());
        for node in &scene.nodes {
            let date = node.figures.iter().find_map(|figure| match figure {
                Figure::Text(text) if text.font == crate::figure::Font::Mono => Some(text),
                _ => None,
            });
            let date = date.expect("a date");
            assert!(date.at.y > BASELINE && date.text.chars().all(|c| !c.is_lowercase()));
        }
    }

    #[test]
    fn events_are_spaced_by_their_dates_when_every_date_names_a_year() {
        let scene = build(&timeline(&["A (2019)", "B (2020)", "C (2024)"]), &Params::default());
        let first_gap = dot_x(&scene, 1) - dot_x(&scene, 0);
        let second_gap = dot_x(&scene, 2) - dot_x(&scene, 1);
        assert!((second_gap / first_gap - 4.0).abs() < 0.2, "{first_gap} then {second_gap}");
        assert!(scene.legend.expect("a legend").iter().any(|key| key.text == "SPACED BY DATE"));
    }

    #[test]
    fn events_are_evenly_spaced_when_a_date_has_no_year() {
        let scene = build(&timeline(&["A (2019)", "B (soon)", "C (2024)"]), &Params::default());
        assert_eq!(dot_x(&scene, 1) - dot_x(&scene, 0), dot_x(&scene, 2) - dot_x(&scene, 1));
        assert!(scene.legend.expect("a legend").iter().any(|key| key.text == "IN ORDER, NOT TO SCALE"));
    }

    #[test]
    fn close_dates_still_keep_names_and_dates_apart() {
        let labels = ["Payments platform rewrite (2021 Q1)", "Fraud rules (2021 Q2)", "New checkout flow (2021 Q3)", "Launch (2030)"];
        let scene = build(&timeline(&labels), &Params::default());
        let texts: Vec<Rect> = scene.nodes.iter().flat_map(|node| &node.figures).filter_map(|figure| match figure {
            Figure::Text(text) => Some(text.bounds()),
            _ => None,
        }).collect();
        for (index, rect) in texts.iter().enumerate() {
            assert!(texts[index + 1..].iter().all(|other| !rect.overlaps(other)), "{rect:?}");
        }
    }

    #[test]
    fn a_milestone_has_a_larger_accent_dot_and_a_bold_name() {
        let mut view = timeline(&["Kick-off (2019)", "Launch (2020)"]);
        view.nodes[1].focal = true;
        let scene = build(&view, &Params::default());
        let dot = |node: &SceneNode| node.figures.iter().find_map(|figure| match figure {
            Figure::Circle { radius, paint, .. } => Some((*radius, paint.fill)),
            _ => None,
        });
        assert_eq!(dot(&scene.nodes[0]), Some((DOT, Tone::Ink)));
        assert_eq!(dot(&scene.nodes[1]), Some((MILESTONE_DOT, Tone::Accent)));
        assert_eq!(name_text(&scene.nodes[1]).weight, 600);
        assert!(name_text(&scene.nodes[0]).weight < 600);
    }

    #[test]
    fn an_event_hit_area_covers_its_dot_drop_and_name() {
        let scene = build(&timeline(&["Kick-off (2019)", "Beta launch (2020)"]), &Params::default());
        for node in &scene.nodes {
            assert_eq!(node.figures[0], Figure::hit_area(node.rect));
            for figure in &node.figures[1..] {
                let bounds = figure.bounds();
                assert!(bounds.x >= node.rect.x && bounds.right() <= node.rect.right() && bounds.y >= node.rect.y && bounds.bottom() <= node.rect.bottom());
            }
        }
    }

    #[test]
    fn a_period_is_a_selectable_bracket_under_the_names() {
        let mut view = timeline(&["A (2019)", "B (2020)", "C (2021)", "D (2022)"]);
        view.groups = vec![
            GroupSpec { id: "build".into(), label: "Build".into(), members: vec!["e0".into(), "e1".into()] },
            GroupSpec { id: "grow".into(), label: "Grow".into(), members: vec!["e2".into(), "e3".into()] },
        ];
        let scene = build(&view, &Params::default());
        assert_eq!(scene.sections.iter().map(|section| section.id.as_str()).collect::<Vec<_>>(), vec!["build", "grow"]);
        let lowest_name = scene.nodes.iter().map(|node| node.rect.bottom()).fold(0.0, f64::max);
        for section in &scene.sections {
            assert!(section.figures[0].bounds().y > lowest_name);
        }
        assert!(!scene.sections[0].figures[0].bounds().overlaps(&scene.sections[1].figures[0].bounds()));
    }

    #[test]
    fn a_timeline_draws_no_connectors() {
        let mut view = timeline(&["A (2019)", "B (2020)"]);
        view.edges = vec![EdgeSpec { id: "a->b".into(), source: "e0".into(), target: "e1".into(), label: String::new(), style: Default::default(), focal: false }];
        assert!(build(&view, &Params::default()).edges.is_empty());
    }
}
