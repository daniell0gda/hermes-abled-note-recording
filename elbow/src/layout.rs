//! Layered layout in abstract coordinates: `u` runs from rank to rank, `v` runs across a rank.
//! Long edges pass through dummy vertices, one per rank they cross.

use crate::geom::{snap, snap_up};

const PORT_PITCH: f64 = 14.0;
const DUMMY_GAP: f64 = 16.0;
const DUMMY_NODE_GAP: f64 = 24.0;
const RELAX_ROUNDS: usize = 40;
const STRAIGHT_WEIGHT: f64 = 8.0;
const TRANSPOSE_ROUNDS: usize = 4;

/// Where a node must sit inside its rank: anywhere, or at the low or high end of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pin {
    Low,
    #[default]
    Free,
    High,
}

/// The size of a node: `su` along the ranks, `sv` across them, `room_low` kept free on its low-`v` side,
/// the lane (band across the ranks) it must stay in when the layout has lanes, and its pin.
#[derive(Clone, Copy, Debug)]
pub struct NodeBox {
    pub su: f64,
    pub sv: f64,
    pub room_low: f64,
    pub lane: usize,
    pub pin: Pin,
}

/// Bands the layout must keep: lanes across the ranks with the room each keeps free before and after
/// its nodes, or tiers along the ranks (`tiers[node]`, lowest first) that each own whole ranks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bands<'a> {
    pub lane_room: Option<&'a [(f64, f64)]>,
    pub tiers: Option<&'a [usize]>,
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub rank: usize,
    pub su: f64,
    pub sv: f64,
    pub room_low: f64,
    pub lane: usize,
    pub pin: Pin,
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
    pub lane_bands: Vec<(f64, f64)>,
    pub lane_room: Vec<(f64, f64)>,
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

    pub fn start_port(&self, hop: &Hop) -> f64 {
        self.vertices[hop.from].v + self.port_offset(hop.from, hop.from_slot)
    }

    pub fn end_port(&self, hop: &Hop) -> f64 {
        self.vertices[hop.to].v + self.port_offset(hop.to, hop.to_slot)
    }
}

/// Lays out `boxes` connected by `edges` (pairs of box indices, no self loops).
/// With lanes, every box stays inside the band of its lane, and bands run across all ranks.
/// With tiers, every tier owns a run of whole ranks, in tier order.
pub fn layer(boxes: &[NodeBox], edges: &[(usize, usize)], node_gap: f64, sweeps: usize, bands: Bands) -> Layered {
    let reversed = back_edges(boxes.len(), edges, bands.tiers);
    let forward: Vec<(usize, usize)> = edges
        .iter()
        .zip(&reversed)
        .map(|(&(a, b), &flip)| if flip { (b, a) } else { (a, b) })
        .collect();
    let ranks = match bands.tiers {
        Some(tiers) => tiered_ranks(&forward, tiers),
        None => assign_ranks(boxes.len(), &forward),
    };
    let mut layered = with_dummies(boxes, &forward, &reversed, &ranks);
    order(&mut layered, sweeps);
    apply_pins(&mut layered);
    assign_slots(&mut layered);
    grow_for_ports(&mut layered);
    match bands.lane_room {
        Some(room) => place_in_lanes(&mut layered, node_gap, room),
        None => place_across(&mut layered, node_gap),
    }
    layered
}

/// Which `edges` point against the ranks: the ones a cycle break reverses, and with tiers every edge
/// from a later tier to an earlier one.
pub fn back_edges(count: usize, edges: &[(usize, usize)], tiers: Option<&[usize]>) -> Vec<bool> {
    let Some(tiers) = tiers else { return reversed_edges(count, edges) };
    let within: Vec<usize> = (0..edges.len()).filter(|&index| tiers[edges[index].0] == tiers[edges[index].1]).collect();
    let local_pairs: Vec<(usize, usize)> = within.iter().map(|&index| edges[index]).collect();
    let local = reversed_edges(count, &local_pairs);
    let mut reversed: Vec<bool> = edges.iter().map(|&(a, b)| tiers[a] > tiers[b]).collect();
    for (&index, flip) in within.iter().zip(local) {
        reversed[index] = flip;
    }
    reversed
}

fn tiered_ranks(forward: &[(usize, usize)], tiers: &[usize]) -> Vec<usize> {
    let within: Vec<(usize, usize)> = forward.iter().copied().filter(|&(a, b)| tiers[a] == tiers[b]).collect();
    let local = assign_ranks(tiers.len(), &within);
    let tier_count = tiers.iter().map(|&tier| tier + 1).max().unwrap_or(0);
    let mut depth = vec![0usize; tier_count];
    for (&tier, &rank) in tiers.iter().zip(&local) {
        depth[tier] = depth[tier].max(rank + 1);
    }
    let mut start = vec![0usize; tier_count];
    for tier in 1..tier_count {
        start[tier] = start[tier - 1] + depth[tier - 1];
    }
    tiers.iter().zip(local).map(|(&tier, rank)| start[tier] + rank).collect()
}

fn apply_pins(layered: &mut Layered) {
    let vertices = &layered.vertices;
    for rank in &mut layered.ranks {
        rank.sort_by_key(|&vertex| (vertices[vertex].lane, vertices[vertex].pin));
    }
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
        .map(|(size, &rank)| Vertex {
            rank,
            su: size.su,
            sv: size.sv,
            room_low: size.room_low,
            lane: size.lane,
            pin: size.pin,
            dummy: false,
            v: 0.0,
        })
        .collect();
    let mut hops = Vec::new();
    let mut chains = Vec::new();
    for (index, &(a, b)) in edges.iter().enumerate() {
        let mut path = vec![a];
        for rank in ranks[a] + 1..ranks[b] {
            vertices.push(Vertex {
                rank,
                su: 0.0,
                sv: 0.0,
                room_low: 0.0,
                lane: boxes[b].lane,
                pin: Pin::Free,
                dummy: true,
                v: 0.0,
            });
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
    for rank in &mut by_rank {
        rank.sort_by_key(|&vertex| vertices[vertex].lane);
    }
    Layered { vertices, ranks: by_rank, hops, chains, lane_bands: Vec::new(), lane_room: Vec::new() }
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
            let mut keyed: Vec<(usize, f64, usize, usize)> = layered.ranks[rank]
                .iter()
                .enumerate()
                .map(|(index, &vertex)| {
                    let centre = barycentre(&adjacent[vertex], &position).unwrap_or(index as f64);
                    (layered.vertices[vertex].lane, centre, index, vertex)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
            layered.ranks[rank] = keyed.into_iter().map(|(_, _, _, vertex)| vertex).collect();
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
                let pair = (layered.ranks[rank][index], layered.ranks[rank][index + 1]);
                if layered.vertices[pair.0].lane != layered.vertices[pair.1].lane {
                    continue;
                }
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
    snap_to_grid(layered);
    straighten_long_edges(layered, node_gap);
    normalise(layered);
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
        let above = &layered.vertices[members[index - 1]];
        let below = &layered.vertices[members[index]];
        let lanes_apart = !layered.lane_bands.is_empty() && above.lane != below.lane;
        offsets[index] = offsets[index - 1] + if lanes_apart { 0.0 } else { separation(above, below, node_gap) };
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

const MIN_LANE: f64 = 96.0;

fn stack_extent(layered: &Layered, members: &[usize], node_gap: f64) -> f64 {
    let Some(&first) = members.first() else { return 0.0 };
    let head = layered.vertices[first].sv / 2.0 + layered.vertices[first].room_low;
    let body: f64 = members
        .windows(2)
        .map(|pair| separation(&layered.vertices[pair[0]], &layered.vertices[pair[1]], node_gap))
        .sum();
    head + body + layered.vertices[*members.last().unwrap_or(&first)].sv / 2.0
}

fn lane_bands(layered: &Layered, node_gap: f64) -> Vec<(f64, f64)> {
    let lane_count = layered.vertices.iter().map(|vertex| vertex.lane + 1).max().unwrap_or(0);
    let mut bands = Vec::with_capacity(lane_count);
    let mut top = 0.0;
    for lane in 0..lane_count {
        let tallest = layered
            .ranks
            .iter()
            .map(|rank| {
                let members: Vec<usize> = rank.iter().copied().filter(|&vertex| layered.vertices[vertex].lane == lane).collect();
                stack_extent(layered, &members, node_gap)
            })
            .fold(0.0, f64::max);
        let (before, after) = layered.lane_room[lane];
        let height = snap_up(MIN_LANE.max(tallest + before + after));
        bands.push((top, top + height));
        top += height;
    }
    bands
}

fn inner_band(layered: &Layered, lane: usize) -> (f64, f64) {
    let (top, bottom) = layered.lane_bands[lane];
    let (before, after) = layered.lane_room[lane];
    (top + before, bottom - after)
}

fn lane_limits(vertex: &Vertex, inner: (f64, f64)) -> (f64, f64) {
    let low = inner.0 + vertex.room_low + vertex.sv / 2.0;
    let high = inner.1 - vertex.sv / 2.0;
    (low, high.max(low))
}

fn place_in_lanes(layered: &mut Layered, node_gap: f64, room: &[(f64, f64)]) {
    let lane_count = layered.vertices.iter().map(|vertex| vertex.lane + 1).max().unwrap_or(0);
    layered.lane_room = (0..lane_count).map(|lane| room.get(lane).copied().unwrap_or_default()).collect();
    layered.lane_bands = lane_bands(layered, node_gap);
    for rank in layered.ranks.clone() {
        for lane in 0..layered.lane_bands.len() {
            let members: Vec<usize> = rank.iter().copied().filter(|&vertex| layered.vertices[vertex].lane == lane).collect();
            let inner = inner_band(layered, lane);
            let middle = (inner.0 + inner.1) / 2.0;
            let mut v = middle - stack_extent(layered, &members, node_gap) / 2.0;
            for (index, &vertex) in members.iter().enumerate() {
                v += if index == 0 {
                    layered.vertices[vertex].room_low + layered.vertices[vertex].sv / 2.0
                } else {
                    separation(&layered.vertices[members[index - 1]], &layered.vertices[vertex], node_gap)
                };
                layered.vertices[vertex].v = v;
            }
        }
    }
    let pulls = pulls(layered);
    for _ in 0..RELAX_ROUNDS {
        for rank in 0..layered.ranks.len() {
            relax_rank(layered, rank, &pulls, node_gap);
            keep_in_lanes(layered, rank, node_gap);
        }
    }
    snap_to_grid(layered);
    straighten_long_edges(layered, node_gap);
}

fn keep_in_lanes(layered: &mut Layered, rank: usize, node_gap: f64) {
    let members = layered.ranks[rank].clone();
    for lane in 0..layered.lane_bands.len() {
        let in_lane: Vec<usize> = members.iter().copied().filter(|&vertex| layered.vertices[vertex].lane == lane).collect();
        let band = inner_band(layered, lane);
        for (index, &vertex) in in_lane.iter().enumerate() {
            let (low, high) = lane_limits(&layered.vertices[vertex], band);
            let mut v = layered.vertices[vertex].v.clamp(low, high);
            if index > 0 {
                let above = &layered.vertices[in_lane[index - 1]];
                v = v.max(above.v + separation(above, &layered.vertices[vertex], node_gap));
            }
            layered.vertices[vertex].v = v;
        }
        for index in (0..in_lane.len()).rev() {
            let vertex = in_lane[index];
            let (_, high) = lane_limits(&layered.vertices[vertex], band);
            let mut v = layered.vertices[vertex].v.min(high);
            if index + 1 < in_lane.len() {
                let below = &layered.vertices[in_lane[index + 1]];
                v = v.min(below.v - separation(&layered.vertices[vertex], below, node_gap));
            }
            layered.vertices[vertex].v = v;
        }
    }
}

fn snap_to_grid(layered: &mut Layered) {
    for vertex in &mut layered.vertices {
        let top = snap(vertex.v - vertex.sv / 2.0);
        vertex.v = top + vertex.sv / 2.0;
    }
}

fn straighten_long_edges(layered: &mut Layered, node_gap: f64) {
    let position = positions(layered);
    for chain in &layered.chains {
        let dummies: Vec<usize> = chain.hops.iter().skip(1).map(|&hop| layered.hops[hop].from).collect();
        let (Some(&first), Some(&last)) = (chain.hops.first(), chain.hops.last()) else { continue };
        if dummies.is_empty() {
            continue;
        }
        let (low, high) = dummies
            .iter()
            .map(|&dummy| free_range(layered, &position, dummy, node_gap))
            .fold((f64::NEG_INFINITY, f64::INFINITY), |(low, high), range| (low.max(range.0), high.min(range.1)));
        if low > high {
            continue;
        }
        let ends = [layered.end_port(&layered.hops[last]), layered.start_port(&layered.hops[first])];
        let v = ends.into_iter().find(|v| (low..=high).contains(v)).unwrap_or_else(|| {
            let mean = dummies.iter().map(|&dummy| layered.vertices[dummy].v).sum::<f64>() / dummies.len() as f64;
            mean.clamp(low, high)
        });
        for &dummy in &dummies {
            layered.vertices[dummy].v = v;
        }
    }
}

fn free_range(layered: &Layered, position: &[usize], vertex: usize, node_gap: f64) -> (f64, f64) {
    let own = &layered.vertices[vertex];
    let rank = &layered.ranks[own.rank];
    let index = position[vertex];
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    if index > 0 {
        let above = &layered.vertices[rank[index - 1]];
        low = above.v + separation(above, own, node_gap);
    }
    if let Some(&next) = rank.get(index + 1) {
        let below = &layered.vertices[next];
        high = below.v - separation(own, below, node_gap);
    }
    if own.lane < layered.lane_bands.len() {
        let (lane_low, lane_high) = lane_limits(own, inner_band(layered, own.lane));
        low = low.max(lane_low);
        high = high.min(lane_high);
    }
    (low, high)
}

fn normalise(layered: &mut Layered) {
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
        vec![NodeBox { su: 120.0, sv: 60.0, room_low: 0.0, lane: 0, pin: Pin::Free }; count]
    }

    #[test]
    fn a_cycle_is_broken_by_reversing_one_edge() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2), (2, 0)], 40.0, 8, Bands::default());
        assert_eq!(layered.chains.iter().filter(|chain| chain.reversed).count(), 1);
    }

    #[test]
    fn a_long_edge_gets_one_dummy_per_crossed_rank() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2), (0, 2)], 40.0, 8, Bands::default());
        assert_eq!(layered.vertices.iter().filter(|vertex| vertex.dummy).count(), 1);
        assert_eq!(layered.chains[2].hops.len(), 2);
    }

    #[test]
    fn a_chain_of_single_edges_is_straight() {
        let layered = layer(&boxes(3), &[(0, 1), (1, 2)], 40.0, 8, Bands::default());
        let v: Vec<f64> = layered.vertices.iter().map(|vertex| vertex.v).collect();
        assert_eq!(v[0], v[1]);
        assert_eq!(v[1], v[2]);
    }

    #[test]
    fn nodes_in_one_rank_keep_their_gap() {
        let layered = layer(&boxes(3), &[(0, 1), (0, 2)], 40.0, 8, Bands::default());
        let gap = (layered.vertices[1].v - layered.vertices[2].v).abs() - 60.0;
        assert!(gap >= 40.0 - 4.0, "gap was {gap}");
    }

    #[test]
    fn every_tier_owns_its_own_ranks_in_tier_order() {
        let tiers = [1, 0, 0, 2];
        let bands = Bands { tiers: Some(&tiers), ..Bands::default() };
        let layered = layer(&boxes(4), &[(0, 3), (1, 2), (3, 1)], 40.0, 8, bands);
        let rank: Vec<usize> = layered.vertices.iter().take(4).map(|vertex| vertex.rank).collect();
        assert_eq!(rank, vec![2, 0, 1, 3]);
        assert!(layered.chains[2].reversed);
    }

    #[test]
    fn a_pinned_node_sits_at_its_end_of_the_rank() {
        let mut nodes = boxes(4);
        nodes[1].pin = Pin::High;
        let layered = layer(&nodes, &[(0, 1), (0, 2), (0, 3)], 40.0, 8, Bands::default());
        assert_eq!(layered.ranks[1].last(), Some(&1));
    }

    #[test]
    fn a_lane_keeps_its_room_free() {
        let mut nodes = boxes(2);
        nodes[1].lane = 1;
        let room = [(24.0, 24.0), (64.0, 24.0)];
        let layered = layer(&nodes, &[(0, 1)], 40.0, 8, Bands { lane_room: Some(&room), ..Bands::default() });
        let second = &layered.vertices[1];
        assert!(second.v - second.sv / 2.0 >= layered.lane_bands[1].0 + 64.0);
    }

    #[test]
    fn a_long_edge_runs_straight_through_its_dummies() {
        let layered = layer(&boxes(5), &[(0, 1), (1, 2), (2, 3), (0, 3), (0, 4)], 40.0, 8, Bands::default());
        let dummies: Vec<f64> = layered.vertices.iter().filter(|vertex| vertex.dummy).map(|vertex| vertex.v).collect();
        assert_eq!(dummies.len(), 2);
        assert_eq!(dummies[0], dummies[1]);
    }

    #[test]
    fn isotonic_pools_violators() {
        assert_eq!(isotonic(&[1.0, 3.0, 2.0], &[1.0, 1.0, 1.0]), vec![1.0, 2.5, 2.5]);
    }
}

