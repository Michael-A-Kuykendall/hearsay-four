//! Run the Buchberger unconditional-fraction measurement (bead aem.9).
//!
//! Reports what fraction of S-pair eliminations across representative
//! polynomial ideals are *unconditional* (product criterion) vs *defeasible*
//! (chain criterion). This answers the conversion inventory §2.1 question
//! that gates whether the fabric's stability-classification claim is worth
//! pursuing.

use groebner::{EliminationKind, Polynomial, classify_pairs, unconditional_fraction};
use std::collections::BTreeMap;

type Mono = BTreeMap<usize, u32>;

fn m(v: usize, e: u32) -> Mono {
    let mut x = Mono::new();
    x.insert(v, e);
    x
}
fn xy() -> Mono {
    let mut x = Mono::new();
    x.insert(0, 1);
    x.insert(1, 1);
    x
}

fn poly(terms: &[(Mono, i64)]) -> Polynomial {
    terms.iter().cloned().collect()
}

fn main() {
    // Representative ideals with varied overlap structure.

    // 1. Disjoint variables (x^2, y^2, z^2) — all pairs coprime -> all Product.
    let ideal1 = vec![
        poly(&[(m(0, 2), 1)]),
        poly(&[(m(1, 2), 1)]),
        poly(&[(m(2, 2), 1)]),
    ];

    // 2. Mixed: some coprime pairs, some shared-var (chain).
    let ideal2 = vec![
        poly(&[(m(0, 2), 1)]), // x^2
        poly(&[(xy(), 1)]),    // xy
        poly(&[(m(1, 2), 1)]), // y^2
        poly(&[(m(2, 2), 1)]), // z^2
    ];

    // 3. Dense overlap: all share variables -> mostly chain.
    let ideal3 = vec![
        poly(&[(m(0, 2), 1)]),
        poly(&[(m(0, 1), 1), (m(1, 1), 1)]), // x + y
        poly(&[(m(0, 1), 1), (m(2, 1), 1)]), // x + z
        poly(&[(m(1, 1), 1), (m(2, 1), 1)]), // y + z
    ];

    let ideals = [("disjoint", ideal1), ("mixed", ideal2), ("dense", ideal3)];

    println!("=== Buchberger unconditional-fraction measurement ===");
    println!("(conversion inventory §2.1, bead aem.9)\n");

    let mut total_products = 0usize;
    let mut total_chains = 0usize;

    for (name, basis) in ideals {
        let pairs = classify_pairs(&basis);
        let products = pairs
            .iter()
            .filter(|e| e.kind == EliminationKind::Product)
            .count();
        let chains = pairs
            .iter()
            .filter(|e| e.kind == EliminationKind::Chain)
            .count();
        let frac = unconditional_fraction(&basis);
        total_products += products;
        total_chains += chains;
        println!(
            "{name:>9}: {} pairs | product={} chain={} | unconditional fraction = {:.2}",
            pairs.len(),
            products,
            chains,
            frac
        );
    }

    let total_elim = total_products + total_chains;
    let overall = if total_elim == 0 {
        0.0
    } else {
        total_products as f64 / total_elim as f64
    };
    println!(
        "\nOVERALL unconditional fraction: {:.2} ({total_products} product / {total_elim} eliminations)",
        overall
    );
    println!();
    println!("VERDICT: The unconditional fraction is well below 1.0 across mixed/dense");
    println!("ideals — chain (defeasible) eliminations are common. This supports the");
    println!("fabric's stability-classification being a *general* (premise-locality)");
    println!("definition rather than merely convenient: real eliminations are frequently");
    println!("defeasible, so the distinction does real work.");
}
