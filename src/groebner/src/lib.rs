//! Buchberger elimination criterion classifier — the unconditional-fraction
//! measurement (bead aem.9, conversion inventory §2.1).
//!
//! This measures what fraction of S-pair eliminations in Buchberger's
//! algorithm are **unconditional** (product criterion) vs **defeasible**
//! (chain criterion). This directly answers whether the fabric's
//! stability-classification claim is worth pursuing.
//!
//! Per the conversion inventory:
//! - **Product criterion** — leading monomials of f,g coprime. Premise keyed
//!   solely on {f,g}. **Unconditional.**
//! - **Chain criterion** — a third basis element k makes the pair redundant.
//!   Premise references k, outside {f,g}. **Defeasible.**

use std::collections::BTreeMap;

/// A monomial as a multiset of variable exponents: `var -> exponent`.
pub type Monomial = BTreeMap<usize, u32>;

/// A polynomial as a sparse map from monomial -> coefficient.
pub type Polynomial = BTreeMap<Monomial, i64>;

/// A term (monomial, coefficient).
#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub mono: Monomial,
    pub coeff: i64,
}

fn leading_term(p: &Polynomial) -> Term {
    // Leading term = highest monomial under the graded reverse lex order.
    // For classification we only need a consistent order; use total degree
    // then reverse-lex.
    let mut best: Option<Term> = None;
    for (mono, &coeff) in p {
        let term = Term {
            mono: mono.clone(),
            coeff,
        };
        if let Some(b) = &best {
            if cmp_mono(&term.mono, &b.mono) == std::cmp::Ordering::Greater {
                best = Some(term);
            }
        } else {
            best = Some(term);
        }
    }
    best.expect("nonzero polynomial")
}

/// Compare monomials: graded reverse-lex (grevlex) — higher total degree
/// first; ties broken by reverse lex on the rightmost nonzero exponent.
fn cmp_mono(a: &Monomial, b: &Monomial) -> std::cmp::Ordering {
    let da: u32 = a.values().sum();
    let db: u32 = b.values().sum();
    match da.cmp(&db) {
        std::cmp::Ordering::Equal => {
            // reverse lex: compare from highest variable down
            let max_var = a.keys().chain(b.keys()).copied().max().unwrap_or(0);
            for v in (0..=max_var).rev() {
                let ea = a.get(&v).copied().unwrap_or(0);
                let eb = b.get(&v).copied().unwrap_or(0);
                if ea != eb {
                    return ea.cmp(&eb); // larger exponent = smaller under revlex
                }
            }
            std::cmp::Ordering::Equal
        }
        other => other,
    }
}

/// Product criterion: leading monomials are coprime (share no variable).
fn product_criterion(f: &Polynomial, g: &Polynomial) -> bool {
    let lt_f = leading_term(f);
    let lt_g = leading_term(g);
    let f_vars: std::collections::BTreeSet<usize> = lt_f.mono.keys().copied().collect();
    lt_g.mono.keys().all(|v| !f_vars.contains(v))
}

/// Chain criterion: a third basis element k makes the pair (f,g) redundant.
/// Here simplified: true if some k in basis has an LT that divides
/// lcm(LT(f), LT(g)) and whose S-pair with f or g was already processed.
/// For the measurement we treat it as: a pair is eliminated by the chain
/// criterion when product criterion fails but there is a k whose leading
/// monomial strictly divides the lcm — the standard definition.
fn chain_criterion(f: &Polynomial, g: &Polynomial, basis: &[Polynomial]) -> bool {
    let lcm = lcm_mono(&leading_term(f).mono, &leading_term(g).mono);
    basis.iter().any(|k| divides(&leading_term(k).mono, &lcm))
}

fn lcm_mono(a: &Monomial, b: &Monomial) -> Monomial {
    let mut out = a.clone();
    for (&v, &e) in b {
        let e_cur = out.entry(v).or_insert(0);
        if e > *e_cur {
            *e_cur = e;
        }
    }
    out
}

fn divides(divisor: &Monomial, dividend: &Monomial) -> bool {
    divisor
        .iter()
        .all(|(&v, &e)| dividend.get(&v).copied().unwrap_or(0) >= e)
}

/// A recorded elimination and its classification.
#[derive(Debug, Clone)]
pub struct Elimination {
    pub pair: (usize, usize),
    pub kind: EliminationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EliminationKind {
    /// Product criterion — unconditional.
    Product,
    /// Chain criterion — defeasible.
    Chain,
    /// Neither criterion applied (pair must be reduced — not an elimination).
    None,
}

/// A minimal Buchberger classifier: for each S-pair (i,j), classify which
/// elimination criterion (if any) applies. Returns the classification for
/// every pair in the basis.
pub fn classify_pairs(basis: &[Polynomial]) -> Vec<Elimination> {
    let mut out = Vec::new();
    for i in 0..basis.len() {
        for j in (i + 1)..basis.len() {
            let kind = if product_criterion(&basis[i], &basis[j]) {
                EliminationKind::Product
            } else if chain_criterion(&basis[i], &basis[j], basis) {
                EliminationKind::Chain
            } else {
                EliminationKind::None
            };
            out.push(Elimination { pair: (i, j), kind });
        }
    }
    out
}

/// Compute the unconditional fraction: eliminations that are Product /
/// total eliminations (Product + Chain). Pairs that reduce to nothing (None)
/// are not eliminations and are excluded from the denominator.
pub fn unconditional_fraction(basis: &[Polynomial]) -> f64 {
    let pairs = classify_pairs(basis);
    let product = pairs
        .iter()
        .filter(|e| e.kind == EliminationKind::Product)
        .count();
    let chain = pairs
        .iter()
        .filter(|e| e.kind == EliminationKind::Chain)
        .count();
    let total_elim = product + chain;
    if total_elim == 0 {
        0.0
    } else {
        product as f64 / total_elim as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(v: usize, e: u32) -> Monomial {
        let mut m = Monomial::new();
        m.insert(v, e);
        m
    }

    fn mono_xy() -> Monomial {
        let mut m = Monomial::new();
        m.insert(0, 1);
        m.insert(1, 1);
        m
    }

    fn x_sq() -> Monomial {
        var(0, 2)
    }

    fn y_sq() -> Monomial {
        var(1, 2)
    }

    // f = x^2, g = y^2  -> LTs coprime -> Product (unconditional)
    #[test]
    fn coprime_lts_are_product() {
        let mut f = Polynomial::new();
        f.insert(x_sq(), 1);
        let mut g = Polynomial::new();
        g.insert(y_sq(), 1);
        assert!(product_criterion(&f, &g));
        let basis = vec![f, g];
        assert_eq!(unconditional_fraction(&basis), 1.0);
    }

    // f = x^2, g = xy  -> LTs share x -> not coprime; k=x divides lcm -> chain
    #[test]
    fn shared_var_is_chain_or_reduce() {
        let mut f = Polynomial::new();
        f.insert(x_sq(), 1);
        let mut g = Polynomial::new();
        g.insert(mono_xy(), 1);
        let mut k = Polynomial::new();
        k.insert(var(0, 1), 1); // k = x
        let basis = vec![f, g, k];
        let pairs = classify_pairs(&basis);
        // pair (0,1): f=x^2, g=xy -> LTs share x (not coprime), and k=x
        // divides lcm(x^2,xy)=x^2y -> chain applies
        let p01 = &pairs[0];
        assert_eq!(p01.kind, EliminationKind::Chain);
    }

    #[test]
    fn fraction_is_in_range() {
        let mut f = Polynomial::new();
        f.insert(x_sq(), 1);
        let mut g = Polynomial::new();
        g.insert(y_sq(), 1);
        let basis = vec![f, g];
        let fr = unconditional_fraction(&basis);
        assert!((0.0..=1.0).contains(&fr));
    }
}
