//! Renders many random diagrams and counts connector rule violations per rule.
//! usage: cargo run --release --example stress [count] [--best] [--dense] [--examples] [--kind <kind>]
//! Without --dense the diagrams stay near the diagram-design budget (up to 9 nodes and 12 edges).

use std::collections::BTreeMap;

use elbow::{render, render_best, Params, View};

const WORDS: [&str; 16] = [
    "sends", "reads", "writes", "calls back", "approves", "rejects", "own db", "notifies", "retries", "assigns", "uploads",
    "token", "redirect with code", "payment fails 3 times", "ok", "",
];
const KINDS: [&str; 5] = ["flow", "state", "tree", "sequence", "swimlane"];

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }
}

fn random_view(random: &mut Random, dense: bool) -> String {
    let kind = KINDS[random.below(KINDS.len())];
    let count = if dense { 3 + random.below(9) } else { 3 + random.below(7) };
    let nodes: Vec<String> = (0..count)
        .map(|index| format!(r#"{{"id": "n{index}", "label": "Node {index} {}"}}"#, if index % 3 == 0 { "(postgres)" } else { "" }))
        .collect();
    let edge_count = if dense { count - 1 + random.below(count) } else { (count - 1 + random.below(count / 2 + 1)).min(12) };
    let edges: Vec<String> = (0..edge_count)
        .map(|index| {
            let source = if index < count - 1 { index / 2 } else { random.below(count) };
            let target = if index < count - 1 { index + 1 } else { random.below(count) };
            let label = WORDS[random.below(WORDS.len())];
            format!(r#"{{"source": "n{source}", "target": "n{target}", "label": "{label}"}}"#)
        })
        .collect();
    let groups = if kind == "swimlane" {
        let lanes = 2 + random.below(3);
        (0..lanes)
            .map(|lane| {
                let members: Vec<String> = (0..count).filter(|node| node % lanes == lane).map(|node| format!(r#""n{node}""#)).collect();
                format!(r#"{{"id": "lane{lane}", "label": "Lane {lane}", "members": [{}]}}"#, members.join(","))
            })
            .collect::<Vec<_>>()
            .join(",")
    } else {
        String::new()
    };
    format!(
        r#"{{"title": "Random", "kind": "{kind}", "nodes": [{}], "edges": [{}], "groups": [{groups}]}}"#,
        nodes.join(","),
        edges.join(",")
    )
}

fn cause(detail: &str) -> String {
    let words: Vec<&str> = detail.split(' ').filter(|word| !word.contains("->") && !word.starts_with('n')).collect();
    let self_loop = detail.split(' ').any(|word| word.split_once("->").is_some_and(|(a, b)| a == b));
    format!("{}{}", words.join(" "), if self_loop { " [self loop]" } else { "" })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: usize = args.iter().find_map(|arg| arg.parse().ok()).unwrap_or(300);
    let best = args.iter().any(|arg| arg == "--best");
    let dense = args.iter().any(|arg| arg == "--dense");
    let only_kind = args.iter().position(|arg| arg == "--kind").and_then(|at| args.get(at + 1)).cloned();
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut by_rule: BTreeMap<u8, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut slowest = 0u128;
    let mut examples: BTreeMap<u8, String> = BTreeMap::new();
    let mut by_cause: BTreeMap<String, usize> = BTreeMap::new();
    for _ in 0..count {
        let json = random_view(&mut random, dense);
        let view: View = serde_json::from_str(&json).expect("generated view parses");
        if only_kind.as_deref().is_some_and(|kind| !json.contains(&format!(r#""kind": "{kind}""#))) {
            continue;
        }
        let rendered = if best { render_best(&view, &Params::default()) } else { render(&view, &Params::default()) };
        slowest = slowest.max(rendered.report.micros);
        let entry = by_kind.entry(format!("{:?}", view.kind)).or_insert((0, 0));
        entry.0 += 1;
        if !rendered.report.violations.is_empty() {
            entry.1 += 1;
        }
        for violation in &rendered.report.violations {
            *by_rule.entry(violation.rule).or_insert(0) += 1;
            *by_cause.entry(cause(&violation.detail)).or_insert(0) += 1;
            examples.entry(violation.rule).or_insert_with(|| format!("{} | {json}", violation.detail));
        }
    }
    for (kind, (total, bad)) in &by_kind {
        println!("{kind:>10}: {bad}/{total} diagrams with violations");
    }
    for (rule, total) in &by_rule {
        println!("rule {rule}: {total}");
    }
    for (cause, total) in &by_cause {
        println!("  {cause}: {total}");
    }
    for (rule, example) in examples.iter().filter(|_| args.iter().any(|arg| arg == "--examples")) {
        println!("example rule {rule}: {example}");
    }
    println!("slowest render: {slowest} us");
}
