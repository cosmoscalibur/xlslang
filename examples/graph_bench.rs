//! Ordering benchmark behind the graph-library decision in docs/DESIGN.md:
//! `cargo run --release --example graph_bench`.
//!
//! Every contender receives the same random dependency graph (edges point to
//! earlier nodes, ~3 per node, like cell references) and must produce an
//! evaluation order that also exposes cycles.

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::DiGraph;
use std::time::Instant;
use ultragraph::{GraphMut, StructuralGraphAlgorithms, TopologicalGraphAlgorithms, UltraGraph};
use xlslang::graph::{Csr, tarjan};

fn main() {
    for n in [10_000usize, 100_000, 1_000_000] {
        let mut seed = 7u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        let adj: Vec<Vec<u32>> = (0..n)
            .map(|v| {
                if v == 0 {
                    vec![]
                } else {
                    (0..3).map(|_| (rnd() % v) as u32).collect()
                }
            })
            .collect();
        println!(
            "n = {n} ({} edges)",
            adj.iter().map(Vec::len).sum::<usize>()
        );

        let t = Instant::now();
        let s = tarjan(&Csr::from_adjacency(&adj));
        println!(
            "  xlslang CSR + Tarjan SCC          {:>9.1?}  ({} components)",
            t.elapsed(),
            s.bounds.len() - 1
        );

        let t = Instant::now();
        let mut g = DiGraph::<(), ()>::with_capacity(n, n * 3);
        let ix: Vec<_> = (0..n).map(|_| g.add_node(())).collect();
        for (v, es) in adj.iter().enumerate() {
            for &w in es {
                g.add_edge(ix[v], ix[w as usize], ());
            }
        }
        let built = t.elapsed();
        let t2 = Instant::now();
        let c = tarjan_scc(&g).len();
        println!(
            "  petgraph build + tarjan_scc       {:>9.1?}  (build {built:.1?}, {c} components)",
            t.elapsed()
        );
        let t = Instant::now();
        let ok = toposort(&g, None).is_ok();
        println!(
            "  petgraph toposort (DAG only)      {:>9.1?}  (ok={ok}; scc alone {:.1?})",
            t.elapsed(),
            t2.elapsed()
        );

        let t = Instant::now();
        let mut u = UltraGraph::<u32>::with_capacity(n, Some(n * 3));
        let ids: Vec<usize> = (0..n).map(|v| u.add_node(v as u32).unwrap()).collect();
        for (v, es) in adj.iter().enumerate() {
            for &w in es {
                u.add_edge(ids[v], ids[w as usize], ()).unwrap();
            }
        }
        u.freeze();
        let built = t.elapsed();
        let order = u.topological_sort();
        println!(
            "  ultragraph build+freeze+toposort  {:>9.1?}  (build {built:.1?}, ok={})",
            t.elapsed(),
            matches!(order, Ok(Some(_)))
        );
        let t = Instant::now();
        let scc = u
            .strongly_connected_components()
            .map(|c| c.len())
            .unwrap_or(0);
        println!(
            "  ultragraph SCC (after freeze)     {:>9.1?}  ({scc} components)",
            t.elapsed()
        );
    }
}
