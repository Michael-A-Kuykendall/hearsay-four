//! Differential exactness: indexed `DecisionFabric` vs the pre-index reference
//! implementation (PERF nlvs proof). 500 seeded random op sequences run in
//! both arms; `DecisionEvent` logs, liveness bits, tilings, and transcripts
//! must match exactly (byte-identical decisions; only cost changes). Pool
//! QUERIES follow the (A') relationship (new == reference minus
//! committed-region tokens: the working set excludes decided regions);
//! pool MUTATIONS (prune/evict) stay bit-identical via `active_all`.

#[allow(dead_code)]
#[path = "support/reference_decision.rs"]
mod reference_decision;

use fabric::FactId;
use fabric::decision as new;
use reference_decision as old;
use std::collections::BTreeSet;

// --- Seeded RNG (xorshift64*; no new deps) ---------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn span(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % (hi - lo) as u64) as u32
    }
    fn llr(&mut self) -> f64 {
        self.next_range(-800, 800) as f64 / 100.0
    }
    fn next_range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

// --- Arm adapters ------------------------------------------------------------

const LABELS: [&str; 6] = ["a", "b", "c", "ab", "bc", "abc"];

fn new_kind(i: usize) -> new::TokenKind {
    [
        new::TokenKind::Word,
        new::TokenKind::Sil,
        new::TokenKind::Phone,
    ][i % 3]
}

fn old_kind(i: usize) -> old::TokenKind {
    [
        old::TokenKind::Word,
        old::TokenKind::Sil,
        old::TokenKind::Phone,
    ][i % 3]
}

fn new_class(i: usize) -> new::SourceClass {
    [
        new::SourceClass::Acoustic,
        new::SourceClass::Lexical,
        new::SourceClass::Speaker,
        new::SourceClass::Verify,
    ][i % 4]
}

fn old_class(i: usize) -> old::SourceClass {
    [
        old::SourceClass::Acoustic,
        old::SourceClass::Lexical,
        old::SourceClass::Speaker,
        old::SourceClass::Verify,
    ][i % 4]
}

fn lm_score(a: &str, b: &str) -> f64 {
    ((a.len() + 3 * b.len()) % 7) as f64 - 3.0
}

fn new_params(rng: &mut Rng) -> new::DecisionParams {
    let h = rng.span(2, 9);
    new::DecisionParams {
        w_acoustic: 1.0,
        w_lexical: 1.0,
        w_sequence: 1.0,
        w_speaker: 1.0,
        w_verify: 1.0,
        w_user: 1.0,
        u0: 100.0,
        theta_live: rng.next_range(-200, 200) as f64 / 100.0,
        live_budget: rng.below(7),
        dormant_budget: rng.below(7),
        combined_budget: if rng.below(2) == 0 {
            None
        } else {
            Some(rng.below(9))
        },
        h,
        h_max: h + rng.span(0, 11),
        delta: rng.next_range(0, 200) as f64 / 100.0,
        j: rng.span(0, 6),
        arm: if rng.below(2) == 0 {
            new::Arm::B
        } else {
            new::Arm::C
        },
    }
}

fn old_params(p: &new::DecisionParams) -> old::DecisionParams {
    old::DecisionParams {
        w_acoustic: p.w_acoustic,
        w_lexical: p.w_lexical,
        w_sequence: p.w_sequence,
        w_speaker: p.w_speaker,
        w_verify: p.w_verify,
        w_user: p.w_user,
        u0: p.u0,
        theta_live: p.theta_live,
        live_budget: p.live_budget,
        dormant_budget: p.dormant_budget,
        combined_budget: p.combined_budget,
        h: p.h,
        h_max: p.h_max,
        delta: p.delta,
        j: p.j,
        arm: match p.arm {
            new::Arm::B => old::Arm::B,
            new::Arm::C => old::Arm::C,
        },
    }
}

/// Mirrored pair: every op applies to both arms in lockstep, so `FactId`
/// assignment (mint + premise interning order) stays aligned.
struct Pair {
    n: new::DecisionFabric,
    o: old::DecisionFabric,
    tokens: Vec<FactId>,
    spans: Vec<(FactId, u32, u32)>,
    committed: Vec<(u32, u32)>,
    premises: Vec<FactId>,
    max_end: u32,
}

impl Pair {
    fn new(params: &new::DecisionParams) -> Self {
        Pair {
            n: new::DecisionFabric::new(params.clone()),
            o: old::DecisionFabric::new(old_params(params)),
            tokens: Vec::new(),
            spans: Vec::new(),
            committed: Vec::new(),
            premises: Vec::new(),
            max_end: 0,
        }
    }

    /// (A') pool relationship: the new arm's pools exclude committed-region
    /// tokens; the reference includes them. All else is bit-identical.
    fn is_committed(&self, t: FactId) -> bool {
        let Some((_, s, e)) = self.spans.iter().find(|(f, _, _)| *f == t) else {
            return false;
        };
        self.committed.iter().any(|(rs, re)| *s >= *rs && *e <= *re)
    }

    fn mint_external_premise(&mut self) -> FactId {
        let a = self.n.fabric_mut().intern();
        self.n.fabric_mut().assert_assumption(a);
        let b = self.o.fabric_mut().intern();
        self.o.fabric_mut().assert_assumption(b);
        assert_eq!(a, b, "premise FactId alignment");
        self.premises.push(a);
        a
    }

    fn premise_set(&mut self, rng: &mut Rng, extra_endpoints: &[FactId]) -> BTreeSet<FactId> {
        let mut out: BTreeSet<FactId> = extra_endpoints.iter().copied().collect();
        for _ in 0..rng.below(3) {
            if !self.tokens.is_empty() && rng.below(2) == 0 {
                out.insert(self.tokens[rng.below(self.tokens.len())]);
            } else if !self.premises.is_empty() {
                out.insert(self.premises[rng.below(self.premises.len())]);
            }
        }
        if rng.below(10) == 0 {
            out.insert(self.mint_external_premise());
        }
        out
    }

    fn step(&mut self, rng: &mut Rng, seq: usize, op: usize) {
        let tag = format!("seq {seq} op {op}");
        let choice = rng.below(100);
        if self.tokens.is_empty() || choice < 28 {
            // Mint.
            let label = LABELS[rng.below(LABELS.len())];
            let kind = rng.below(10);
            let start = rng.span(0, 60);
            let end = start + rng.span(1, 13);
            let a = self
                .n
                .mint_token(label, new_kind(kind), start, end)
                .expect("mint");
            let b = self
                .o
                .mint_token(label, old_kind(kind), start, end)
                .expect("mint");
            assert_eq!(a, b, "mint FactId alignment at {tag}");
            self.tokens.push(a);
            self.spans.push((a, start, end));
            self.max_end = self.max_end.max(end);
        } else if choice < 50 {
            // Unary evidence (sometimes USER via add_user).
            let t = self.tokens[rng.below(self.tokens.len())];
            if rng.below(12) == 0 {
                let p = if self.premises.is_empty() || rng.below(2) == 0 {
                    self.mint_external_premise()
                } else {
                    self.premises[rng.below(self.premises.len())]
                };
                let ra = self.n.add_user(t, p);
                let rb = self.o.add_user(t, p);
                assert_eq!(ra.is_ok(), rb.is_ok(), "add_user at {tag}");
            } else {
                let c = rng.below(4);
                let llr = rng.llr();
                let ps = self.premise_set(rng, &[]);
                let ra = self.n.add_unary(t, new_class(c), llr, ps.clone());
                let rb = self.o.add_unary(t, old_class(c), llr, ps);
                assert_eq!(ra.is_ok(), rb.is_ok(), "add_unary at {tag}");
            }
        } else if choice < 63 {
            // Sequence evidence (bridge-style: endpoints as premises).
            let f = self.tokens[rng.below(self.tokens.len())];
            let t = self.tokens[rng.below(self.tokens.len())];
            let llr = rng.llr();
            let ps = self.premise_set(rng, &[f, t]);
            let ra = self.n.add_sequence(f, t, llr, ps.clone());
            let rb = self.o.add_sequence(f, t, llr, ps);
            assert_eq!(ra.is_ok(), rb.is_ok(), "add_sequence at {tag}");
            // §7 v1.4: arcs ride along half the sequences, so the driver
            // covers arc-linked and arc-free pairs on both sides.
            if rng.below(2) == 0 {
                let ra = self.n.add_lattice_arc(f, t);
                let rb = self.o.add_lattice_arc(f, t);
                assert_eq!(ra.is_ok(), rb.is_ok(), "add_lattice_arc at {tag}");
            }
        } else if choice < 68 {
            // Boundary pair.
            let t = self.tokens[rng.below(self.tokens.len())];
            let llr = rng.llr();
            let ra = self.n.add_boundary_sequence(t, llr);
            let rb = self.o.add_boundary_sequence(t, llr);
            assert_eq!(ra.is_ok(), rb.is_ok(), "add_boundary at {tag}");
        } else if choice < 78 {
            // Streaming context attachment.
            let t = self.tokens[rng.below(self.tokens.len())];
            self.n
                .attach_context_for_new_token(t, &|a, b| lm_score(&a.label, &b.label));
            self.o
                .attach_context_for_new_token(t, &|a, b| lm_score(&a.label, &b.label));
        } else if choice < 85 {
            self.n.enforce_budgets();
            self.o.enforce_budgets();
        } else if choice < 91 {
            // Commit with a wandering frontier (eligible/flush/denied paths).
            let s = rng.span(0, self.max_end.max(1));
            let e = s + rng.span(0, 25);
            let frontier = rng.span(0, self.max_end + 30);
            let ra = self.n.commit_region((s, e), frontier);
            let rb = self.o.commit_region((s, e), frontier);
            assert_eq!(
                format!("{ra:?}"),
                format!("{rb:?}"),
                "commit_region(({s},{e}), {frontier}) at {tag}"
            );
            if matches!(ra, new::CommitOutcome::Committed { .. }) {
                self.committed.push((s, e));
            }
        } else if choice < 95 {
            // External retraction (token or premise) via fabric_mut.
            if !self.tokens.is_empty() && (self.premises.is_empty() || rng.below(2) == 0) {
                let t = self.tokens[rng.below(self.tokens.len())];
                self.n.fabric_mut().retract(t);
                self.o.fabric_mut().retract(t);
            } else if !self.premises.is_empty() {
                let p = self.premises[rng.below(self.premises.len())];
                self.n.fabric_mut().retract(p);
                self.o.fabric_mut().retract(p);
            }
        } else {
            // Read-only queries (compared below at the checkpoint anyway).
            let t = self.tokens[rng.below(self.tokens.len())];
            let _ = (self.n.liveness(t), self.o.liveness(t));
            let _ = (self.n.is_live(t), self.o.is_live(t));
            let _ = (self.n.is_dormant(t), self.o.is_dormant(t));
            let _ = (self.n.dependents_of(t), self.o.dependents_of(t));
            let _ = (self.n.lattice_successors(t), self.o.lattice_successors(t));
        }
        self.checkpoint(&tag);
    }

    /// Byte-identity checkpoint: event log (exact), live/dormant pools
    /// (new == reference minus committed-region tokens, the (A') pool
    /// relationship), per-token liveness bits (exact), tilings, and
    /// committed transcripts.
    fn checkpoint(&self, tag: &str) {
        assert_eq!(
            format!("{:?}", self.n.events()),
            format!("{:?}", self.o.events()),
            "event log at {tag}"
        );
        let old_live: Vec<FactId> = self
            .o
            .live_tokens()
            .into_iter()
            .filter(|t| !self.is_committed(*t))
            .collect();
        assert_eq!(self.n.live_tokens(), old_live, "live at {tag}");
        let old_dormant: Vec<FactId> = self
            .o
            .dormant_tokens()
            .into_iter()
            .filter(|t| !self.is_committed(*t))
            .collect();
        assert_eq!(self.n.dormant_tokens(), old_dormant, "dormant at {tag}");
        for t in &self.tokens {
            assert_eq!(
                self.n.liveness(*t).to_bits(),
                self.o.liveness(*t).to_bits(),
                "liveness({}) at {tag}",
                t.0
            );
        }
    }

    fn check_tilings(&self, tag: &str) {
        for (s, e) in [
            (0, 30),
            (0, 60),
            (10, 50),
            (0, self.max_end),
            (5, 5),
            (40, 20),
        ] {
            let a = self.n.best_tilings((s, e));
            let b = self.o.best_tilings((s, e));
            assert_eq!(
                format!("{a:?}"),
                format!("{b:?}"),
                "best_tilings(({s},{e})) at {tag}"
            );
        }
        // Committed transcripts: winner labels per Committed event.
        let transcript = |ev: &[new::DecisionEvent]| -> Vec<Vec<String>> {
            ev.iter()
                .filter_map(|e| match e {
                    new::DecisionEvent::Committed { winner, .. } => Some(
                        winner
                            .iter()
                            .map(|f| self.n.token(*f).unwrap().label.clone())
                            .collect(),
                    ),
                    _ => None,
                })
                .collect()
        };
        let ot: Vec<Vec<String>> = self
            .o
            .events()
            .iter()
            .filter_map(|e| match e {
                old::DecisionEvent::Committed { winner, .. } => Some(
                    winner
                        .iter()
                        .map(|f| self.o.token(*f).unwrap().label.clone())
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(transcript(self.n.events()), ot, "transcripts at {tag}");
    }
}

#[test]
fn differential_500_random_sequences_match_exactly() {
    for seq in 0..500 {
        let mut rng =
            Rng(0x9E3779B97F4A7C15u64.wrapping_add((seq as u64).wrapping_mul(0xBF58476D1CE4E5B9)));
        let params = new_params(&mut rng);
        let mut pair = Pair::new(&params);
        let ops = 30 + rng.below(50);
        for op in 0..ops {
            pair.step(&mut rng, seq, op);
        }
        pair.check_tilings(&format!("seq {seq} final"));
    }
}
