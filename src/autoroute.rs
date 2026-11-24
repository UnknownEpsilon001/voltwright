// Automatic PCB routing: a two-layer maze router on a grid.
//
// Each net is joined pad to pad by a shortest-path search (A*) that may change
// layer through a via. Nets are routed shortest first, and every finished
// route is stamped into the grid with a one-cell halo, so later nets keep the
// clearance. Top copper prefers horizontal runs and bottom copper vertical
// ones, which keeps the two layers from fighting over the same channels.
//
// Anything it cannot connect is left as a ratsnest line and reported.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::model::{Layer, Project, Track, Via, P};
use crate::pcb::absolute_pad;

#[derive(Clone, Debug)]
pub struct RouteParams {
    /// Routing grid in thousandths of an inch. Track pitch is two cells.
    pub grid: f32,
    pub width: f32,
    pub clearance: f32,
    pub via_size: f32,
    pub via_drill: f32,
    /// Extra cost of a via, in cells of track.
    pub via_cost: u32,
}

impl Default for RouteParams {
    fn default() -> Self {
        RouteParams { grid: 25.0, width: 20.0, clearance: 8.0, via_size: 50.0, via_drill: 28.0, via_cost: 8 }
    }
}

pub struct RouteResult {
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub connections: usize,
    pub routed: usize,
    /// Nets that still have unconnected pads.
    pub failed: Vec<String>,
}

/// Clamp a size read from a project file to something the grid can hold.
fn sane(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 20_000.0)
    } else {
        0.0
    }
}

/// No clearance halo reaches further than this many cells from its copper.
const MAX_REACH_CELLS: f32 = 64.0;

/// Largest grid the router will build, in cells per layer.
const MAX_CELLS: usize = 4_000_000;

const FREE: u16 = 0;
const BLOCK: u16 = u16::MAX - 1;
const MULTI: u16 = u16::MAX;

struct Grid {
    cols: usize,
    rows: usize,
    origin: P,
    g: f32,
    /// Net that owns the cell itself, per layer.
    occ: [Vec<u16>; 2],
    /// Net whose clearance halo covers the cell (MULTI when several do).
    halo: [Vec<u16>; 2],
}

impl Grid {
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.cols + x
    }
    fn centre(&self, x: usize, y: usize) -> P {
        P::new(self.origin.x + x as f32 * self.g, self.origin.y + y as f32 * self.g)
    }
    fn nearest(&self, p: P) -> (usize, usize) {
        let x = ((p.x - self.origin.x) / self.g).round().clamp(0.0, (self.cols - 1) as f32) as usize;
        let y = ((p.y - self.origin.y) / self.g).round().clamp(0.0, (self.rows - 1) as f32) as usize;
        (x, y)
    }
    fn claim_halo(&mut self, layer: usize, i: usize, net: u16) {
        let h = &mut self.halo[layer][i];
        if *h == FREE {
            *h = net;
        } else if *h != net {
            *h = MULTI;
        }
    }
    fn passable(&self, layer: usize, i: usize, net: u16) -> bool {
        let o = self.occ[layer][i];
        let h = self.halo[layer][i];
        (o == FREE || o == net) && (h == FREE || h == net)
    }
    /// Mark everything within `reach` of a point: cells close to its centre
    /// belong to the net, the ring out to `block` is its clearance halo.
    fn stamp_disc(&mut self, layers: &[usize], c: P, block: f32, net: u16) {
        // Sizes come from the project file, so bound everything by the grid.
        if !c.x.is_finite() || !c.y.is_finite() {
            return;
        }
        let block = sane(block).min(self.g * MAX_REACH_CELLS);
        let r = (block / self.g).ceil() as usize + 1;
        let (cx, cy) = self.nearest(c);
        for y in cy.saturating_sub(r)..=(cy + r).min(self.rows - 1) {
            for x in cx.saturating_sub(r)..=(cx + r).min(self.cols - 1) {
                let d = self.centre(x, y).dist(c);
                let i = self.idx(x, y);
                for &l in layers {
                    if d < self.g * 0.5 {
                        if self.occ[l][i] == FREE || self.occ[l][i] == net {
                            self.occ[l][i] = net;
                        } else {
                            self.occ[l][i] = MULTI;
                        }
                    } else if d < block {
                        self.claim_halo(l, i, net);
                    }
                }
            }
        }
    }
    /// Mark the cells near a track: scan only the cells inside the track's
    /// bounding box grown by `block`, clipped to the grid, so the work is
    /// bounded by the grid size however long the track is.
    fn stamp_segment(&mut self, layer: usize, a: P, b: P, block: f32, net: u16) {
        if !a.x.is_finite() || !a.y.is_finite() || !b.x.is_finite() || !b.y.is_finite() {
            return;
        }
        let block = sane(block).min(self.g * MAX_REACH_CELLS);
        let cell = |v: f32, origin: f32, max: usize| (((v - origin) / self.g).floor().clamp(0.0, max as f32 - 1.0)) as usize;
        let reach = block + self.g;
        let x0 = cell(a.x.min(b.x) - reach, self.origin.x, self.cols);
        let x1 = cell(a.x.max(b.x) + reach, self.origin.x, self.cols);
        let y0 = cell(a.y.min(b.y) - reach, self.origin.y, self.rows);
        let y1 = cell(a.y.max(b.y) + reach, self.origin.y, self.rows);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = crate::model::point_seg_dist(self.centre(x, y), a, b);
                let i = self.idx(x, y);
                if d < self.g * 0.5 {
                    if self.occ[layer][i] == FREE || self.occ[layer][i] == net {
                        self.occ[layer][i] = net;
                    } else {
                        self.occ[layer][i] = MULTI;
                    }
                } else if d < block {
                    self.claim_halo(layer, i, net);
                }
            }
        }
    }
}

struct PadRef {
    net: u16,
    at: P,
    radius: f32,
    through: bool,
    cell: (usize, usize),
}

pub fn route(proj: &Project, par: &RouteParams) -> RouteResult {
    let (o, e) = proj.board.rect();
    let g = par.grid.max(1.0);
    let too_big = !(e.x - o.x).is_finite() || !(e.y - o.y).is_finite() || e.x <= o.x || e.y <= o.y;
    let (cols, rows) = if too_big {
        (0, 0)
    } else {
        (
            (((e.x - o.x) / g).floor().min(1.0e6) as usize) + 1,
            (((e.y - o.y) / g).floor().min(1.0e6) as usize) + 1,
        )
    };
    if too_big || cols.saturating_mul(rows) > MAX_CELLS {
        return RouteResult {
            tracks: Vec::new(),
            vias: Vec::new(),
            connections: 1,
            routed: 0,
            failed: vec!["(the board outline is empty or too large for the router)".to_string()],
        };
    }
    let n = cols * rows;
    let mut grid = Grid {
        cols,
        rows,
        origin: o,
        g,
        occ: [vec![FREE; n], vec![FREE; n]],
        halo: [vec![FREE; n], vec![FREE; n]],
    };

    // Net names to ids; parts of no net get an id of their own.
    let mut ids: HashMap<String, u16> = HashMap::new();
    for f in &proj.fps {
        for pad in &f.pads {
            if let Some(nm) = &pad.net {
                let next = ids.len() as u16 + 1;
                ids.entry(nm.clone()).or_insert(next);
            }
        }
    }
    let named = ids.len() as u16;
    let mut lone = named;
    let mut lone_id = || {
        lone += 1;
        lone
    };
    let half_w = par.width * 0.5;

    // Board edge: keep the track clear of the outline.
    let edge = proj.rules.edge.max(par.clearance) + half_w;
    for y in 0..rows {
        for x in 0..cols {
            let c = grid.centre(x, y);
            if c.x < o.x + edge || c.x > e.x - edge || c.y < o.y + edge || c.y > e.y - edge {
                let i = grid.idx(x, y);
                grid.occ[0][i] = BLOCK;
                grid.occ[1][i] = BLOCK;
            }
        }
    }

    // Pads.
    let mut pads: Vec<PadRef> = Vec::new();
    for f in &proj.fps {
        for pad in &f.pads {
            let at = absolute_pad(f, pad);
            let net = pad.net.as_ref().and_then(|nm| ids.get(nm).copied()).unwrap_or_else(&mut lone_id);
            let through = pad.drill > 0.0;
            let layers: &[usize] = if through { &[0, 1] } else { &[0] };
            let radius = sane(pad.size) * 0.5;
            grid.stamp_disc(layers, at, radius + par.clearance + half_w, net);
            pads.push(PadRef { net, at, radius, through, cell: grid.nearest(at) });
        }
    }
    // Existing copper stays where it is.
    for t in &proj.tracks {
        let net = t.net.as_ref().and_then(|nm| ids.get(nm).copied()).unwrap_or_else(&mut lone_id);
        let l = if t.layer == Layer::BottomCopper { 1 } else { 0 };
        if t.layer == Layer::TopCopper || t.layer == Layer::BottomCopper {
            grid.stamp_segment(l, t.a, t.b, sane(t.width) * 0.5 + par.clearance + half_w, net);
        }
    }
    for v in &proj.vias {
        let net = v.net.as_ref().and_then(|nm| ids.get(nm).copied()).unwrap_or_else(&mut lone_id);
        grid.stamp_disc(&[0, 1], v.pos, sane(v.size) * 0.5 + par.clearance + half_w, net);
    }
    // A pad's own access cell must belong to its net.
    for p in &pads {
        let i = grid.idx(p.cell.0, p.cell.1);
        for l in 0..2 {
            if l == 0 || p.through {
                grid.occ[l][i] = p.net;
                grid.halo[l][i] = p.net;
            }
        }
    }

    // Pads of each net, and which of them are already joined by copper.
    let mut by_net: HashMap<u16, Vec<usize>> = HashMap::new();
    for (i, p) in pads.iter().enumerate() {
        if p.net <= named {
            by_net.entry(p.net).or_default().push(i);
        }
    }
    let names: HashMap<u16, String> = ids.iter().map(|(k, v)| (*v, k.clone())).collect();

    let mut order: Vec<u16> = by_net.keys().copied().collect();
    let span = |net: &u16| {
        let v = &by_net[net];
        let (mut lo, mut hi) = (P::new(f32::MAX, f32::MAX), P::new(f32::MIN, f32::MIN));
        for &i in v {
            let q = pads[i].at;
            lo = P::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = P::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        (hi.x - lo.x) + (hi.y - lo.y)
    };
    order.sort_by(|a, b| span(a).total_cmp(&span(b)).then(a.cmp(b)));

    let mut out = RouteResult { tracks: Vec::new(), vias: Vec::new(), connections: 0, routed: 0, failed: Vec::new() };
    for net in order {
        let members = &by_net[&net];
        let comps = joined_groups(proj, &pads, members, par);
        if comps.len() < 2 {
            continue;
        }
        out.connections += comps.len() - 1;
        let name = names.get(&net).cloned().unwrap_or_default();
        let mut tree: Vec<(usize, usize)> = Vec::new(); // (layer, cell index)
        let mut pending: Vec<Vec<usize>> = comps;
        let first = pending.remove(0);
        for &p in &first {
            add_pad_cells(&grid, &pads[p], &mut tree);
        }
        let mut failed = false;
        while !pending.is_empty() {
            // Target mask: every cell of the pads still to join.
            let mut target = vec![[false; 2]; n];
            for comp in &pending {
                for &p in comp {
                    let i = grid.idx(pads[p].cell.0, pads[p].cell.1);
                    target[i][0] = true;
                    if pads[p].through {
                        target[i][1] = true;
                    }
                }
            }
            let Some(path) = search(&grid, net, &tree, &target, par) else {
                failed = true;
                break;
            };
            let end = *path.last().unwrap();
            // Which waiting group did the path reach?
            let k = pending
                .iter()
                .position(|comp| comp.iter().any(|&p| grid.idx(pads[p].cell.0, pads[p].cell.1) == end.1))
                .unwrap_or(0);
            let joined = pending.remove(k);
            stamp_path(&mut grid, &path, net);
            emit(&grid, &path, net, &name, par, &mut out);
            tree.extend(path.iter().copied());
            for &p in &joined {
                add_pad_cells(&grid, &pads[p], &mut tree);
                stub(&grid, &pads[p], &path, &name, par, &mut out);
            }
            // The path's first cell sits on a pad of the tree.
            if let Some(&start) = path.first() {
                for &p in members {
                    if grid.idx(pads[p].cell.0, pads[p].cell.1) == start.1 {
                        stub(&grid, &pads[p], &path, &name, par, &mut out);
                    }
                }
            }
            out.routed += 1;
        }
        if failed {
            out.failed.push(name);
        }
    }
    out
}

/// Lines still to be routed: for each net, one link between neighbouring
/// groups of pads that copper does not yet join. The ratsnest shows these.
pub fn open_links(proj: &Project) -> Vec<(P, P)> {
    let mut pads: Vec<PadRef> = Vec::new();
    let mut ids: HashMap<String, u16> = HashMap::new();
    for f in &proj.fps {
        for pad in &f.pads {
            let net = match &pad.net {
                Some(nm) => {
                    let next = ids.len() as u16 + 1;
                    *ids.entry(nm.clone()).or_insert(next)
                }
                None => 0,
            };
            pads.push(PadRef { net, at: absolute_pad(f, pad), radius: pad.size * 0.5, through: pad.drill > 0.0, cell: (0, 0) });
        }
    }
    let par = RouteParams::default();
    let mut links = Vec::new();
    for net in 1..=ids.len() as u16 {
        let members: Vec<usize> = (0..pads.len()).filter(|&i| pads[i].net == net).collect();
        let mut groups = joined_groups(proj, &pads, &members, &par);
        if groups.len() < 2 {
            continue;
        }
        // Chain each group to the closest pad pair in the groups already placed.
        let mut placed: Vec<usize> = groups.remove(0);
        while !groups.is_empty() {
            let mut best = (f32::MAX, 0usize, 0usize, 0usize);
            for (gi, g) in groups.iter().enumerate() {
                for &a in g {
                    for &b in &placed {
                        let d = pads[a].at.dist(pads[b].at);
                        if d < best.0 {
                            best = (d, gi, a, b);
                        }
                    }
                }
            }
            links.push((pads[best.3].at, pads[best.2].at));
            placed.extend(groups.remove(best.1));
        }
    }
    links
}

fn add_pad_cells(grid: &Grid, p: &PadRef, tree: &mut Vec<(usize, usize)>) {
    let i = grid.idx(p.cell.0, p.cell.1);
    tree.push((0, i));
    if p.through {
        tree.push((1, i));
    }
}

/// Group a net's pads by the copper that already joins them.
fn joined_groups(proj: &Project, pads: &[PadRef], members: &[usize], par: &RouteParams) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..members.len()).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    // Existing tracks of the net, merged into clusters first.
    let name = members.first().and_then(|_| None::<&str>);
    let _ = name;
    let tracks: Vec<&Track> = proj
        .tracks
        .iter()
        .filter(|t| t.net.is_some() && members.iter().any(|&m| proj_pad_net(proj, pads, m) == t.net.as_deref()))
        .collect();
    let mut tp: Vec<usize> = (0..tracks.len()).collect();
    for i in 0..tracks.len() {
        for j in (i + 1)..tracks.len() {
            let (a, b) = (tracks[i], tracks[j]);
            let touch = (a.width + b.width) * 0.5;
            let same_layer = a.layer == b.layer && crate::model::seg_dist(a.a, a.b, b.a, b.b) <= touch;
            let via = proj.vias.iter().any(|v| {
                let r = v.size * 0.5 + touch;
                v.net == a.net && crate::model::point_seg_dist(v.pos, a.a, a.b) <= r && crate::model::point_seg_dist(v.pos, b.a, b.b) <= r
            });
            if same_layer || via {
                let (ra, rb) = (find(&mut tp, i), find(&mut tp, j));
                tp[ra] = rb;
            }
        }
    }
    for (mi, &m) in members.iter().enumerate() {
        for (ti, t) in tracks.iter().enumerate() {
            if crate::model::point_seg_dist(pads[m].at, t.a, t.b) <= pads[m].radius + t.width * 0.5 {
                for (mj, &m2) in members.iter().enumerate().skip(mi + 1) {
                    for (tj, t2) in tracks.iter().enumerate() {
                        if find(&mut tp, ti) == find(&mut tp, tj)
                            && crate::model::point_seg_dist(pads[m2].at, t2.a, t2.b) <= pads[m2].radius + t2.width * 0.5
                        {
                            let (ra, rb) = (find(&mut parent, mi), find(&mut parent, mj));
                            parent[ra] = rb;
                        }
                    }
                }
            }
        }
    }
    let _ = par;
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (mi, &m) in members.iter().enumerate() {
        let r = find(&mut parent, mi);
        groups.entry(r).or_default().push(m);
    }
    let mut v: Vec<Vec<usize>> = groups.into_values().collect();
    v.sort_by_key(|g| g[0]);
    v
}

fn proj_pad_net<'a>(proj: &'a Project, pads: &[PadRef], idx: usize) -> Option<&'a str> {
    // Pads were collected in footprint order, so the index walks the same list.
    let mut k = 0;
    for f in &proj.fps {
        for pad in &f.pads {
            if k == idx {
                return pad.net.as_deref();
            }
            k += 1;
        }
    }
    let _ = pads;
    None
}

/// A* from every cell of the net's tree to the nearest target cell.
fn search(grid: &Grid, net: u16, tree: &[(usize, usize)], target: &[[bool; 2]], par: &RouteParams) -> Option<Vec<(usize, usize)>> {
    let n = grid.cols * grid.rows;
    let node = |l: usize, i: usize| l * n + i;
    let mut dist = vec![u32::MAX; 2 * n];
    let mut prev = vec![u32::MAX; 2 * n];
    // Bounding box of the targets, for an admissible straight-line estimate.
    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0usize, usize::MAX, 0usize);
    for (i, t) in target.iter().enumerate() {
        if t[0] || t[1] {
            let (x, y) = (i % grid.cols, i / grid.cols);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    let h = |i: usize| -> u32 {
        let (x, y) = (i % grid.cols, i / grid.cols);
        let dx = if x < x0 { x0 - x } else if x > x1 { x - x1 } else { 0 };
        let dy = if y < y0 { y0 - y } else if y > y1 { y - y1 } else { 0 };
        (dx + dy) as u32 * 10
    };
    let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
    for &(l, i) in tree {
        let s = node(l, i);
        dist[s] = 0;
        heap.push(Reverse((h(i), s as u32)));
    }
    while let Some(Reverse((_, s))) = heap.pop() {
        let s = s as usize;
        let (l, i) = (s / n, s % n);
        let d = dist[s];
        if target[i][l] && d > 0 {
            let mut path = vec![(l, i)];
            let mut cur = s;
            while prev[cur] != u32::MAX {
                cur = prev[cur] as usize;
                path.push((cur / n, cur % n));
            }
            path.reverse();
            return Some(path);
        }
        let (x, y) = (i % grid.cols, i / grid.cols);
        let mut try_move = |nl: usize, ni: usize, cost: u32, heap: &mut BinaryHeap<Reverse<(u32, u32)>>| {
            let ns = node(nl, ni);
            let nd = d + cost;
            if nd < dist[ns] {
                dist[ns] = nd;
                prev[ns] = s as u32;
                heap.push(Reverse((nd + h(ni), ns as u32)));
            }
        };
        // Top copper runs horizontally, bottom vertically; the other way costs more.
        let (hcost, vcost) = if l == 0 { (10, 16) } else { (16, 10) };
        if x > 0 && (grid.passable(l, i - 1, net) || target[i - 1][l]) {
            try_move(l, i - 1, hcost, &mut heap);
        }
        if x + 1 < grid.cols && (grid.passable(l, i + 1, net) || target[i + 1][l]) {
            try_move(l, i + 1, hcost, &mut heap);
        }
        if y > 0 && (grid.passable(l, i - grid.cols, net) || target[i - grid.cols][l]) {
            try_move(l, i - grid.cols, vcost, &mut heap);
        }
        if y + 1 < grid.rows && (grid.passable(l, i + grid.cols, net) || target[i + grid.cols][l]) {
            try_move(l, i + grid.cols, vcost, &mut heap);
        }
        // Via: the cell and its four neighbours must be usable on both layers.
        let other = 1 - l;
        let usable = |ci: usize| grid.passable(0, ci, net) && grid.passable(1, ci, net);
        let around = [
            Some(i),
            (x > 0).then(|| i - 1),
            (x + 1 < grid.cols).then(|| i + 1),
            (y > 0).then(|| i - grid.cols),
            (y + 1 < grid.rows).then(|| i + grid.cols),
        ];
        if grid.passable(other, i, net) && around.iter().flatten().all(|&ci| usable(ci) || (grid.passable(l, ci, net) && grid.passable(other, ci, net))) {
            try_move(other, i, par.via_cost * 10, &mut heap);
        }
    }
    None
}

fn stamp_path(grid: &mut Grid, path: &[(usize, usize)], net: u16) {
    for (k, &(l, i)) in path.iter().enumerate() {
        grid.occ[l][i] = net;
        grid.halo[l][i] = net;
        let (x, y) = ((i % grid.cols) as i32, (i / grid.cols) as i32);
        let via = (k > 0 && path[k - 1].1 == i) || (k + 1 < path.len() && path[k + 1].1 == i);
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= grid.cols as i32 || ny >= grid.rows as i32 {
                    continue;
                }
                let ni = ny as usize * grid.cols + nx as usize;
                grid.claim_halo(l, ni, net);
                if via {
                    grid.claim_halo(1 - l, ni, net);
                }
            }
        }
    }
}

/// Turn a cell path into tracks (runs of one direction) and vias.
fn emit(grid: &Grid, path: &[(usize, usize)], net: u16, name: &str, par: &RouteParams, out: &mut RouteResult) {
    let _ = net;
    let layer_of = |l: usize| if l == 0 { Layer::TopCopper } else { Layer::BottomCopper };
    let pt = |i: usize| grid.centre(i % grid.cols, i / grid.cols);
    let mut run_start = 0usize;
    for k in 1..=path.len() {
        let end_of_run = k == path.len() || path[k].0 != path[k - 1].0 || {
            // Direction change inside a layer ends the run.
            let d0 = (path[k].1 as i64 - path[k - 1].1 as i64).signum();
            k >= 2 && path[k - 1].0 == path[k - 2].0 && {
                let d_prev = path[k - 1].1 as i64 - path[k - 2].1 as i64;
                (d_prev.abs() == 1) != ((path[k].1 as i64 - path[k - 1].1 as i64).abs() == 1) && d0 != 0
            }
        };
        if !end_of_run {
            continue;
        }
        let last = k - 1;
        let (a, b) = (path[run_start], path[last]);
        if a.1 != b.1 {
            out.tracks.push(Track {
                id: 0,
                a: pt(a.1),
                b: pt(b.1),
                width: par.width,
                layer: layer_of(a.0),
                net: Some(name.to_string()),
            });
        }
        if k < path.len() && path[k].0 != path[k - 1].0 {
            out.vias.push(Via { id: 0, pos: pt(path[k].1), size: par.via_size, drill: par.via_drill, net: Some(name.to_string()) });
            run_start = k;
        } else {
            run_start = last;
        }
    }
}

/// Join a pad's centre to the grid cell its route ends on.
fn stub(grid: &Grid, p: &PadRef, path: &[(usize, usize)], name: &str, par: &RouteParams, out: &mut RouteResult) {
    let cell = grid.centre(p.cell.0, p.cell.1);
    if cell.dist(p.at) < 0.5 {
        return;
    }
    let i = grid.idx(p.cell.0, p.cell.1);
    let layer = path.iter().find(|c| c.1 == i).map(|c| c.0).unwrap_or(0);
    out.tracks.push(Track {
        id: 0,
        a: p.at,
        b: cell,
        width: par.width,
        layer: if layer == 0 { Layer::TopCopper } else { Layer::BottomCopper },
        net: Some(name.to_string()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Library;

    /// Pads of each net that end up joined, by walking tracks and vias.
    fn unconnected_nets(proj: &Project) -> Vec<String> {
        let mut bad = Vec::new();
        let mut nets: Vec<String> = proj.fps.iter().flat_map(|f| f.pads.iter().filter_map(|p| p.net.clone())).collect();
        nets.sort();
        nets.dedup();
        for net in nets {
            let pads: Vec<(P, f32)> = proj
                .fps
                .iter()
                .flat_map(|f| f.pads.iter().filter(|p| p.net.as_deref() == Some(net.as_str())).map(|p| (absolute_pad(f, p), p.size * 0.5)))
                .collect();
            if pads.len() < 2 {
                continue;
            }
            let tracks: Vec<&Track> = proj.tracks.iter().filter(|t| t.net.as_deref() == Some(net.as_str())).collect();
            // Union tracks that touch on one layer or share a via, then see which pads each cluster reaches.
            let mut parent: Vec<usize> = (0..tracks.len()).collect();
            fn find(p: &mut Vec<usize>, i: usize) -> usize {
                if p[i] == i {
                    i
                } else {
                    let r = find(p, p[i]);
                    p[i] = r;
                    r
                }
            }
            for i in 0..tracks.len() {
                for j in (i + 1)..tracks.len() {
                    let (a, b) = (tracks[i], tracks[j]);
                    let touch = (a.width + b.width) * 0.5 + 0.5;
                    let same = a.layer == b.layer && crate::model::seg_dist(a.a, a.b, b.a, b.b) <= touch;
                    let via = proj.vias.iter().any(|v| {
                        v.net.as_deref() == Some(net.as_str())
                            && crate::model::point_seg_dist(v.pos, a.a, a.b) <= v.size * 0.5 + 0.5
                            && crate::model::point_seg_dist(v.pos, b.a, b.b) <= v.size * 0.5 + 0.5
                    });
                    if same || via {
                        let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                        parent[ra] = rb;
                    }
                }
            }
            // Cluster id reached by each pad (None if no track touches it).
            let reach: Vec<Vec<usize>> = pads
                .iter()
                .map(|(at, r)| {
                    (0..tracks.len())
                        .filter(|&t| crate::model::point_seg_dist(*at, tracks[t].a, tracks[t].b) <= r + tracks[t].width * 0.5 + 0.5)
                        .map(|t| find(&mut parent, t))
                        .collect()
                })
                .collect();
            // Pads are one group when they share a cluster; chain them.
            let mut grp: Vec<usize> = (0..pads.len()).collect();
            for a in 0..pads.len() {
                for b in (a + 1)..pads.len() {
                    if reach[a].iter().any(|c| reach[b].contains(c)) {
                        let (ga, gb) = (find(&mut grp, a), find(&mut grp, b));
                        grp[ga] = gb;
                    }
                }
            }
            let first = find(&mut grp, 0);
            if (1..pads.len()).any(|i| find(&mut grp, i) != first) {
                bad.push(net);
            }
        }
        bad
    }

    fn laid_out(name: &str) -> (Project, Library) {
        let lib = Library::builtin();
        let mut proj = crate::examples::build(name, &lib).unwrap();
        let nl = proj.netlist(&lib);
        proj.sync_pcb(&lib, &nl);
        // Roomy board around the footprints, on a 25 th grid.
        let (mut lo, mut hi) = (P::new(f32::MAX, f32::MAX), P::new(f32::MIN, f32::MIN));
        for f in &proj.fps {
            lo = P::new(lo.x.min(f.x), lo.y.min(f.y));
            hi = P::new(hi.x.max(f.x), hi.y.max(f.y));
        }
        proj.board.origin = P::new(((lo.x - 450.0) / 25.0).floor() * 25.0, ((lo.y - 450.0) / 25.0).floor() * 25.0);
        proj.board.width = hi.x - lo.x + 900.0;
        proj.board.height = hi.y - lo.y + 900.0;
        (proj, lib)
    }

    fn apply(proj: &mut Project, res: RouteResult) {
        for mut t in res.tracks {
            t.id = proj.alloc_id();
            proj.tracks.push(t);
        }
        for mut v in res.vias {
            v.id = proj.alloc_id();
            proj.vias.push(v);
        }
    }

    #[test]
    fn routes_a_small_board_completely_and_cleanly() {
        for name in [
            "LED with current-limiting resistor",
            "Transistor switch with lamp",
            "555 timer LED blinker",
            "Common-emitter amplifier",
            "AND, OR and XOR gates",
        ] {
            let (mut proj, _) = laid_out(name);
            let res = route(&proj, &RouteParams::default());
            assert!(res.failed.is_empty(), "{name}: could not route {:?}", res.failed);
            assert!(res.routed > 0 && !res.tracks.is_empty(), "{name}: nothing was routed");
            eprintln!("ROUTE {name}: {} connections, {} tracks, {} vias", res.routed, res.tracks.len(), res.vias.len());
            apply(&mut proj, res);
            assert_eq!(unconnected_nets(&proj), Vec::<String>::new(), "{name}: nets left open");
            assert!(open_links(&proj).is_empty(), "{name}: ratsnest still shows open links");
            let drc = proj.drc();
            assert!(drc.is_empty(), "{name}: {} DRC violations, first {:?}", drc.len(), drc.first().map(|d| (&d.kind, &d.detail)));
        }
    }

    #[test]
    fn nets_already_joined_are_left_alone() {
        let (mut proj, _) = laid_out("LED with current-limiting resistor");
        assert!(!open_links(&proj).is_empty(), "an unrouted board has ratsnest lines");
        let first = route(&proj, &RouteParams::default());
        apply(&mut proj, first);
        let again = route(&proj, &RouteParams::default());
        assert_eq!(again.connections, 0, "everything is already connected");
        assert!(again.tracks.is_empty());
    }
}

#[cfg(test)]
mod hostile_input {
    use super::*;
    use crate::model::Library;

    #[test]
    fn absurd_sizes_and_boards_do_not_hang_or_panic() {
        let lib = Library::builtin();
        let mut proj = crate::examples::build("LED with current-limiting resistor", &lib).unwrap();
        let nl = proj.netlist(&lib);
        proj.sync_pcb(&lib, &nl);
        proj.board.width = 2000.0;
        proj.board.height = 1500.0;
        proj.fps[0].pads[0].size = f32::INFINITY;
        proj.fps[0].pads[1].size = 1.0e30;
        proj.vias.push(Via { id: 99, pos: P::new(500.0, 500.0), size: f32::NAN, drill: 1.0, net: None });
        proj.tracks.push(Track { id: 98, a: P::new(0.0, 0.0), b: P::new(1.0e30, 5.0), width: -5.0, layer: Layer::TopCopper, net: None });
        let _ = route(&proj, &RouteParams::default());
        proj.board.width = 1.0e12;
        let res = route(&proj, &RouteParams::default());
        assert!(!res.failed.is_empty(), "an oversized board is refused");
    }
}
