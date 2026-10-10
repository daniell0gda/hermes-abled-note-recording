//! Layered layout in abstract coordinates: `u` runs from rank to rank, `v` runs across a rank.
//! Long edges pass through dummy vertices, one per rank they cross.

use crate::geom::{snap, snap_up};

const PORT_PITCH: f64 = 14.0;
const DUMMY_GAP: f64 = 16.0;
const DUMMY_NODE_GAP: f64 = 24.0;
const RELAX_ROUNDS: usize = 40;
const STRAIGHT_WEIGHT: f64 = 8.0;
const TRANSPOSE_ROUNDS: usize = 4;

/// The size of a node: `su` along the ranks, `sv` across them, and `room_low` kept free on its low-`v` side.
#[derive(Clone, Copy, Debug)]
pub struct NodeBox {
    pub su: f64,
    pub sv: f64,
    pub room_low: f64,
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub rank: usize,
    pub su: f64,
    pub sv: f64,
    pub room_low: f64,
    pub dummy: bool,
    pub v: f64,
}

/// One rank-to-rank step of an edge, with its attach slot (`k` of `n`) at both ends.
#[derive(Clone, Copy, Debug)]
pub struct Hop {
    pub chain: usize,
    pub from: usize,
    pub to: usize,
    pub from_slot: (usize, usize),
    pub to_slot: (usize, usize),
}

/// The vertices an edge passes, in rank order; `reversed` when the edge points against the ranks.
#[derive(Clone, Debug)]
pub struct Chain {
    pub hops: Vec<usize>,
    pub reversed: bool,
}

#[derive(Clone, Debug)]
pub struct Layered {
    pub vertices: Vec<Vertex>,
    pub ranks: Vec<Vec<usize>>,
    pub hops: Vec<Hop>,
    pub chains: Vec<Chain>,
}

impl Layered {
    /// Where a hop attaches to `vertex`, relative to the vertex centre.
    pub fn port_offset(&self, vertex: usize, slot: (usize, usize)) -> f64 {
        let vertex = &self.vertices[vertex];
        if vertex.dummy {
            return 0.0;
        }
        let (k, n) = slot;
        vertex.sv * (k + 1) as f64 / (n + 1) as f64 - vertex.sv / 2.0
    }

    pub fn from_port(&self, hop: &Hop) -> f64 {
        self.vertices[hop.from].v + self.port_offset(hop.from, hop.from_slot)
    }

    pub fn to_port(&self, hop: &Hop) -> f64 {
        self.vertices[hop.to].v + self.port_offset(hop.to, hop.to_slot)
    }
}

/// Lays out `boxes` connected by `edges` (pairs of box indices, no self loops).
pub fn layer(boxes: &[NodeBox], edges: &[(usize, usize)], node_gap: f64, sweeps: usize) -> Layered {
    let reversed = reversed_edges(boxes.len(), edges);
    let forward: Vec<(usize, usize)> = edges
        .iter()
        .zip(&reversed)
        .map(|(&(a, b), &flip)| if flip { (b, a) } else { (a, b) })
        .collect();
    let ranks = assign_ranks(boxes.len(), &forward);
    let mut layered = with_dummies(boxes, &forward, &reversed, &ranks);
    order(&mut layered, sweeps);
    assign_slots(&mut layered);
    grow_for_ports(&mut layered);
    place_across(&mut layered, node_gap);
    layered
}

fn reversed_edges(count: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut outgoing = vec![Vec::new(); count];
    let mut indegree = vec![0usize; count];
    for (index, &(a, b)) in edges.iter().enumerate() {
        outgoing[a].push(index);
        indegree[b] += 1;
    }
    let mut roots: Vec<usize> = (0..count).filter(|&n| indegree[n] == 0).collect();
    roots.extend((0..count).filter(|&n| indegree[n] != 0));
    let mut state = vec![0u8; count];
    let mut reversed = vec![false; edges.len()];
    for root in roots {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(top) = stack.last_mut() {
            let node = top.0;
            if let Some(&edge) = outgoing[node].get(top.1) {
                top.1 += 1;
                let target = edges[edge].1;
                match state[target] {
                    0 => {
                        state[target] = 1;
                        stack.push((target, 0));
                    }
                    1 => reversed[edge] = true,
                    _ => {}
                }
            } else {
                state[node] = 2;
                stack.pop();
            }
        }
    }
    reversed
}

fn assign_ranks(count: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut outgoing = vec![Vec::new(); count];
    let mut indegree = vec![0usize; count];
    for &(a, b) in edges {
        outgoing[a].push(b);
        indegree[b] += 1;
    }
    let mut rank = vec![0usize; count];
    let mut remaining = indegree.clone();
    let mut queue: Vec<usize> = (0..count).filter(|&n| remaining[n] == 0).collect();
    let mut head = 0;
    while head < queue.len() {
        let node = queue[head];
        head += 1;
        for &next in &outgoing[node] {
            rank[next] = rank[next].max(rank[node] + 1);
            remaining[next] -= 1;
            if remaining[next] == 0 {
                queue.push(next);
            }
        }
    }
    for node in 0..count {
        let closest = outgoing[node].iter().map(|&next| rank[next]).min();
        if let (0, Some(closest)) = (indegree[node], closest) {
            rank[node] = closest.saturating_sub(1).max(rank[node]);
        }
    }
    rank
}

fn with_dummies(boxes: &[NodeBox], edges: &[(usize, usize)], reversed: &[bool], ranks: &[usize]) -> Layered {
    let mut vertices: Vec<Vertex> = boxes
        .iter()
        .zip(ranks)
        .map(|(size, &rank)| Vertex { rank, su: size.su, sv: size.sv, room_low: size.room_low, dummy: false, v: 0.0 })
        .collect();
    let mut hops = Vec::new();
    let mut chains = Vec::new();
    for (index, &(a, b)) in edges.iter().enumerate() {
        let mut path = vec![a];
        for rank in ranks[a] + 1..ranks[b] {
            vertices.push(Vertex { rank, su: 0.0, sv: 0.0, room_low: 0.0, dummy: true, v: 0.0 });
            path.push(vertices.len() - 1);
        }
        path.push(b);
        let chain_hops = path
            .windows(2)
            .map(|pair| {
                hops.push(Hop { chain: index, from: pair[0], to: pair[1], from_slot: (0, 1), to_slot: (0, 1) });
                hops.len() - 1
            })
            .collect();
        chains.push(Chain { hops: chain_hops, reversed: reversed[index] });
    }
    let rank_count = vertices.iter().map(|vertex| vertex.rank + 1).max().unwrap_or(0);
    let mut by_rank = vec![Vec::new(); rank_count];
    for (index, vertex) in vertices.iter().enumerate() {
        by_rank[vertex.rank].push(index);
    }
    Layered { vertices, ranks: by_rank, hops, chains }
}

fn neighbours(layered: &Layered) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut up = vec![Vec::new(); layered.vertices.len()];
    let mut down = vec![Vec::new(); layered.vertices.len()];
    for hop in &layered.hops {
        down[hop.from].push(hop.to);
        up[hop.to].push(hop.from);
    }
    (up, down)
}

fn positions(layered: &Layered) -> Vec<usize> {
    let mut position = vec![0; layered.vertices.len()];
    for rank in &layered.ranks {
        for (index, &vertex) in rank.iter().enumerate() {
            position[vertex] = index;
        }
    }
    position
}

fn order(layered: &mut Layered, sweeps: usize) {
    let (up, down) = neighbours(layered);
    let mut best = layered.ranks.clone();
    let mut best_crossings = crossings(layered);
    for sweep in 0..sweeps {
        let downward = sweep % 2 == 0;
        let rank_indices: Vec<usize> = if downward {
            (1..layered.ranks.len()).collect()
        } else {
            (0..layered.ranks.len().saturating_sub(1)).rev().collect()
        };
        for rank in rank_indices {
            let position = positions(layered);
            let adjacent = if downward { &up } else { &down };
            let mut keyed: Vec<(f64, usize, usize)> = layered.ranks[rank]
                .iter()
                .enumerate()
                .map(|(index, &vertex)| (barycentre(&adjacent[vertex], &position).unwrap_or(index as f64), index, vertex))
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            layered.ranks[rank] = keyed.into_iter().map(|(_, _, vertex)| vertex).collect();
        }
        transpose(layered);
        let count = crossings(layered);
        if count < best_crossings {
            best_crossings = count;
            best = layered.ranks.clone();
        }
    }
    layered.ranks = best;
}

fn barycentre(adjacent: &[usize], position: &[usize]) -> Option<f64> {
    if adjacent.is_empty() {
        return None;
    }
    Some(adjacent.iter().map(|&vertex| position[vertex] as f64).sum::<f64>() / adjacent.len() as f64)
}

fn transpose(layered: &mut Layered) {
    for _ in 0..TRANSPOSE_ROUNDS {
        let mut improved = false;
        for rank in 0..layered.ranks.len() {
            for index in 0..layered.ranks[rank].len().saturating_sub(1) {
                let before = crossings(layered);
                layered.ranks[rank].swap(index, index + 1);
                if crossings(layered) < before {
                    improved = true;
                } else {
                    layered.ranks[rank].swap(index, index + 1);
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Edge crossings between neighbouring ranks.
pub fn crossings(layered: &Layered) -> usize {
    let position = positions(layered);
    let mut by_rank: Vec<Vec<(usize, usize)>> = vec![Vec::new(); layered.ranks.len()];
    for hop in &layered.hops {
        by_rank[layered.vertices[hop.from].rank].push((position[hop.from], position[hop.to]));
    }
    by_rank
        .iter()
        .map(|pairs| {
            let mut count = 0;
            for (i, a) in pairs.iter().enumerate() {
                for b in &pairs[i + 1..] {
                    if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                        count += 1;
                    }
                }
            }
            count
        })
        .sum()
}

fn assign_slots(layered: &mut Layered) {
    let position = positions(layered);
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); layered.vertices.len()];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); layered.vertices.len()];
    for (index, hop) in layered.hops.iter().enumerate() {
        outgoing[hop.from].push(index);
        incoming[hop.to].push(index);
    }
    for side in &mut outgoing {
        side.sort_by_key(|&hop| (position[layered.hops[hop].to], hop));
        let count = side.len();
        for (k, &hop) in side.iter().enumerate() {
            layered.hops[hop].from_slot = (k, count);
        }
    }
    for side in &mut incoming {
        side.sort_by_key(|&hop| (position[layered.hops[hop].from], hop));
        let count = side.len();
        for (k, &hop) in side.iter().enumerate() {
            layered.hops[hop].to_slot = (k, count);
        }
    }
}

fn grow_for_ports(layered: &mut Layered) {
    let mut most = vec![0usize; layered.vertices.len()];
    for hop in &layered.hops {
        most[hop.from] = most[hop.from].max(hop.from_slot.1);
        most[hop.to] = most[hop.to].max(hop.to_slot.1);
    }
    for (vertex, count) in layered.vertices.iter_mut().zip(most) {
        if !vertex.dummy {
            vertex.sv = vertex.sv.max(snap_up((count + 1) as f64 * PORT_PITCH));
        }
    }
}

fn separation(above: &Vertex, below: &Vertex, node_gap: f64) -> f64 {
    let gap = match (above.dummy, below.dummy) {
        (false, false) => node_gap,
        (true, true) => DUMMY_GAP,
        _ => DUMMY_NODE_GAP,
    };
    above.sv / 2.0 + gap + below.room_low + below.sv / 2.0
}

fn place_across(layered: &mut Layered, node_gap: f64) {
    for rank in &layered.ranks {
        let mut v = 0.0;
        for (index, &vertex) in rank.iter().enumerate() {
            if index > 0 {
                v += separation(&layered.vertices[rank[index - 1]], &layered.vertices[vertex], node_gap);
            }
            layered.vertices[vertex].v = v;
        }
    }
    let pulls = pulls(layered);
    for _ in 0..RELAX_ROUNDS {
        for rank in 0..layered.ranks.len() {
            relax_rank(layered, rank, &pulls, node_gap);
        }
    }
    snap_and_normalise(layered);
}

#[derive(Clone, Copy)]
struct Pull {
    other: usize,
    own_offset: f64,
    other_offset: f64,
    weight: f64,
}

impl Pull {
    fn wanted_centre(&self, vertices: &[Vertex]) -> f64 {
        vertices[self.other].v + self.other_offset - self.own_offset
    }
}

fn pulls(layered: &Layered) -> Vec<Vec<Pull>> {
    let mut pulls = vec![Vec::new(); layered.vertices.len()];
    for hop in &layered.hops {
        let from_offset = layered.port_offset(hop.from, hop.from_slot);
        let to_offset = layered.port_offset(hop.to, hop.to_slot);
        let weight = match (layered.vertices[hop.from].dummy, layered.vertices[hop.to].dummy) {
            (true, true) => STRAIGHT_WEIGHT,
            (false, false) => 1.0,
            _ => STRAIGHT_WEIGHT / 2.0,
        };
        pulls[hop.from].push(Pull { other: hop.to, own_offset: from_offset, other_offset: to_offset, weight });
        pulls[hop.to].push(Pull { other: hop.from, own_offset: to_offset, other_offset: from_offset, weight });
    }
    pulls
}

fn relax_rank(layered: &mut Layered, rank: usize, pulls: &[Vec<Pull>], node_gap: f64) {
    let members = layered.ranks[rank].clone();
    let mut targets = Vec::with_capacity(members.len());
    let mut weights = Vec::with_capacity(members.len());
    for &vertex in &members {
        let own = &pulls[vertex];
        let total: f64 = own.iter().map(|pull| pull.weight).sum();
        if total == 0.0 {
            targets.push(layered.vertices[vertex].v);
            weights.push(1e-3);
            continue;
        }
        let wanted: f64 = own.iter().map(|pull| pull.wanted_centre(&layered.vertices) * pull.weight).sum();
        targets.push(wanted / total);
        weights.push(total);
    }
    let mut offsets = vec![0.0; members.len()];
    for index in 1..members.len() {
        offsets[index] = offsets[index - 1]
            + separation(&layered.vertices[members[index - 1]], &layered.vertices[members[index]], node_gap);
    }
    let shifted: Vec<f64> = targets.iter().zip(&offsets).map(|(target, offset)| target - offset).collect();
    let fitted = isotonic(&shifted, &weights);
    for (index, &vertex) in members.iter().enumerate() {
        layered.vertices[vertex].v = fitted[index] + offsets[index];
    }
}

fn isotonic(values: &[f64], weights: &[f64]) -> Vec<f64> {
    let mut blocks: Vec<(f64, f64, usize)> = Vec::new();
    for (&value, &weight) in values.iter().zip(weights) {
        blocks.push((weight, weight * value, 1));
        while blocks.len() >= 2 {
            let last = blocks[blocks.len() - 1];
            let before = blocks[blocks.len() - 2];
            if before.1 / before.0 <= last.1 / last.0 {
                break;
            }
            blocks.pop();
            let merged = blocks.last_mut().expect("two blocks were present");
            *merged = (before.0 + last.0, before.1 + last.1, before.2 + last.2);
        }
    }
    blocks.iter().flat_map(|&(weight, sum, count)| std::iter::repeat(sum / weight).take(count)).collect()
}

fn snap_and_normalise(layered: &mut Layered) {
    for vertex in &mut layered.vertices {
        let top = snap(vertex.v - vertex.sv / 2.0);
        vertex.v = top + vertex.sv / 2.0;
    }
    let lowest = layered
        .vertices
        .iter()
        .map(|vertex| vertex.v - vertex.sv / 2.0 - vertex.room_low)
        .fold(f64::INFINITY, f64::min);
    if lowest.is_finite() {
        let shift = snap(lowest);
        for vertex in &mut layered.vertices {
            vertex.v -= shift;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(count: usize) -> Vec<NodeBox> {
        vec![NodeBox { su: 120.0, sv: 60.0, room_low: 0.0 }; count]
    }

    #[test]
    fn a_cycle_is_broken_by_reversing_one_edge() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2), (2, 0)], 40.0, 8);
        assert_eq!(layered.chains.iter().filter(|chain| chain.reversed).count(), 1);
    }

    #[test]
    fn a_long_edge_gets_one_dummy_per_crossed_rank() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2), (0, 2)], 40.0, 8);
        assert_eq!(layered.vertices.iter().filter(|vertex| vertex.dummy).count(), 1);
        assert_eq!(layered.chains[2].hops.len(), 2);
    }

    #[test]
    fn a_chain_of_single_edges_is_straight() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2)], 40.0, 8);
        let v: Vec<f64> = layered.vertices.iter().map(|vertex| vertex.v).collect();
        assert_eq!(v[0], v[1]);
        assert_eq!(v[1], v[2]);
    }

    #[test]
    fn nodes_in_one_rank_keep_their_gap() {
        let layered = layer(&boxes(3), &[(0, 1), (0, 2)], 40.0, 8);
        let gap = (layered.vertices[1].v - layered.vertices[2].v).abs() - 60.0;
        assert!(gap >= 40.0 - 4.0, "gap was {gap}");
    }

    #[test]
    fn isotonic_pools_violators() {
        assert_eq!(isotonic(&[1.0, 3.0, 2.0], &[1.0, 1.0, 1.0]), vec![1.0, 2.5, 2.5]);
    }
}
