//! Dependency graph support: a compact CSR adjacency and an iterative Tarjan
//! strongly-connected-components pass.
//!
//! Tarjan emits components in reverse topological order of the condensation:
//! with an edge `u → v` meaning "`u` depends on `v`", a component is emitted
//! after everything it depends on. That is exactly the evaluation order, and
//! the same pass isolates the circular references, so one linear traversal
//! gives both. See `docs/DESIGN.md` for why this is hand-rolled instead of
//! `petgraph`/`ultragraph`.

/// Compressed sparse row adjacency: edges of `v` are
/// `targets[offsets[v]..offsets[v + 1]]`.
#[derive(Debug, Default)]
pub struct Csr {
    offsets: Vec<u32>,
    targets: Vec<u32>,
}

impl Csr {
    pub fn from_adjacency(adj: &[Vec<u32>]) -> Self {
        let mut offsets = Vec::with_capacity(adj.len() + 1);
        let mut targets = Vec::with_capacity(adj.iter().map(Vec::len).sum());
        offsets.push(0);
        for edges in adj {
            targets.extend_from_slice(edges);
            offsets.push(targets.len() as u32);
        }
        Self { offsets, targets }
    }

    pub fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn edges(&self, v: u32) -> &[u32] {
        &self.targets[self.offsets[v as usize] as usize..self.offsets[v as usize + 1] as usize]
    }
}

/// Components in dependency-first order, flattened.
#[derive(Debug, Default)]
pub struct Sccs {
    /// Node ids, component after component.
    pub order: Vec<u32>,
    /// `bounds[i]..bounds[i + 1]` delimits component `i` within `order`.
    pub bounds: Vec<u32>,
}

impl Sccs {
    pub fn components(&self) -> impl Iterator<Item = &[u32]> {
        self.bounds
            .windows(2)
            .map(|w| &self.order[w[0] as usize..w[1] as usize])
    }
}

pub fn tarjan(g: &Csr) -> Sccs {
    const UNSEEN: u32 = u32::MAX;
    let n = g.len();
    let mut index = vec![UNSEEN; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<u32> = Vec::new();
    let mut calls: Vec<(u32, u32)> = Vec::new(); // (node, next edge cursor)
    let mut counter = 0u32;
    let mut out = Sccs {
        order: Vec::with_capacity(n),
        bounds: vec![0],
    };

    for root in 0..n as u32 {
        if index[root as usize] != UNSEEN {
            continue;
        }
        index[root as usize] = counter;
        low[root as usize] = counter;
        counter += 1;
        stack.push(root);
        on_stack[root as usize] = true;
        calls.push((root, 0));
        while let Some(&mut (v, ref mut cursor)) = calls.last_mut() {
            let edges = g.edges(v);
            if let Some(&w) = edges.get(*cursor as usize) {
                *cursor += 1;
                if index[w as usize] == UNSEEN {
                    index[w as usize] = counter;
                    low[w as usize] = counter;
                    counter += 1;
                    stack.push(w);
                    on_stack[w as usize] = true;
                    calls.push((w, 0));
                } else if on_stack[w as usize] {
                    low[v as usize] = low[v as usize].min(index[w as usize]);
                }
            } else {
                if low[v as usize] == index[v as usize] {
                    while let Some(w) = stack.pop() {
                        on_stack[w as usize] = false;
                        out.order.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.bounds.push(out.order.len() as u32);
                }
                calls.pop();
                if let Some(&(p, _)) = calls.last() {
                    low[p as usize] = low[p as usize].min(low[v as usize]);
                }
            }
        }
    }
    out
}

/// Whether a component is a genuine cycle (several nodes, or a self-loop).
pub fn is_cycle(g: &Csr, comp: &[u32]) -> bool {
    comp.len() > 1 || g.edges(comp[0]).contains(&comp[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 33) as u32
    }

    #[test]
    fn dependencies_come_first() {
        // 0 → 1 → 2, 0 → 2
        let g = Csr::from_adjacency(&[vec![1, 2], vec![2], vec![]]);
        let s = tarjan(&g);
        assert_eq!(s.order, vec![2, 1, 0]);
    }

    #[test]
    fn detects_cycles_and_self_loops() {
        let g = Csr::from_adjacency(&[vec![1], vec![0], vec![2], vec![]]);
        let s = tarjan(&g);
        let cyc: Vec<bool> = s.components().map(|c| is_cycle(&g, c)).collect();
        assert_eq!(cyc.iter().filter(|&&c| c).count(), 2);
        assert_eq!(s.components().count(), 3);
    }

    /// Reachability by plain BFS: the oracle for the SCC partition.
    fn reach(adj: &[Vec<u32>], from: usize) -> Vec<bool> {
        let mut seen = vec![false; adj.len()];
        let mut stack = vec![from];
        seen[from] = true;
        while let Some(v) = stack.pop() {
            for &w in &adj[v] {
                if !seen[w as usize] {
                    seen[w as usize] = true;
                    stack.push(w as usize);
                }
            }
        }
        seen
    }

    #[test]
    fn matches_brute_force_on_random_graphs() {
        let mut seed = 42;
        for n in [1usize, 2, 10, 60, 150] {
            for density in [1u32, 2, 4] {
                let mut adj = vec![Vec::new(); n];
                for edges in adj.iter_mut() {
                    for _ in 0..(lcg(&mut seed) % (density + 1)) {
                        edges.push(lcg(&mut seed) % n as u32);
                    }
                }
                let s = tarjan(&Csr::from_adjacency(&adj));
                let mut comp_of = vec![usize::MAX; n];
                for (i, c) in s.components().enumerate() {
                    for &v in c {
                        assert_eq!(comp_of[v as usize], usize::MAX, "node listed twice");
                        comp_of[v as usize] = i;
                    }
                }
                assert!(comp_of.iter().all(|&c| c != usize::MAX), "node missing");
                let r: Vec<Vec<bool>> = (0..n).map(|v| reach(&adj, v)).collect();
                for u in 0..n {
                    for v in 0..n {
                        // Same component exactly when mutually reachable.
                        assert_eq!(
                            comp_of[u] == comp_of[v],
                            r[u][v] && r[v][u],
                            "n={n} {u} {v}"
                        );
                    }
                }
                // Dependencies first: every edge leads to an earlier or same component.
                for (v, es) in adj.iter().enumerate() {
                    for &w in es {
                        assert!(comp_of[w as usize] <= comp_of[v]);
                    }
                }
            }
        }
    }
}
