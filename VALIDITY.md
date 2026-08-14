# benchfck validity contract (v3 candidate)

Passing unit tests makes the harness operational; it does not by itself make
the benchmark publication-valid.

## Item acceptance constraints

1. **Semantic program space.** The legacy affine-only five-shape grammar is
   forbidden. The public arity-1 subset contains eight promoted constructors across
   residue/quotient coupling, shifted-residue coupling, residue-square, and residue-
   complement families. A normalized class is a name-independent, complete-domain
   profile over period class, bias-normalized affine-piece bucket, output support,
   and modular first/second-difference support. A batch must contain at least eight
   such classes, no affine-only class, and no class may exceed 25%.
2. **Density measurements.** Executed non-movement E0 steps / all E0 steps
   (`trace_semantic_density >= 0.30`) is an acceptance gate.
   `text_semantic_density` (non-movement E0 source characters / all E0 source
   characters) is a **recorded covariate with no floor** — see the gate ledger
   in §"Retired gates". Because it decays monotonically with program size,
   size tier and movement fraction are collinear by construction in the accepted
   population, and any size-tier effect must be reported controlling for it.
3. **Encoding ladder.** RLE is the accepted E2/E3 carrier. Expanded and omitted
   carriers are diagnostic variants. RLE and expanded executions must preserve
   E0 outputs and exact weighted step counts. E2 is compact explicit addressing;
   E3 intentionally uses verbose machine lexemes. **Neither rung carries an
   item-level prompt-ratio acceptance gate.** Both are admissible only through
   the preregistered token-matched-pair analysis, which controls the length
   confound rather than shrinking it; a ratio ceiling would additionally bias
   the accepted population against the larger size tiers, since program growth
   monotonically raises the explicit-rung ratio. Total-prompt ratios are
   mandatory covariates and drive matching; program-only ratios report the
   undiluted representation manipulation. Both are recorded per item and both
   are reported, never enforced.
4. **Hybrid T2 nontriviality certificate.** The constructor's answer expression
   is never the oracle. `ExprParser` accepts constant-only subexpressions; the
   verifier folds them before applying the lexical response cap, so equivalent
   syntax does not create parse-failure noise. Layer 1 exhaustively enumerates
   only the AST depth actually reachable under the declared resource ceiling
   and records `proven_exhaustive_ast_depth`; it makes no depth-24 claim. Layer
   2 characterizes named whole-function properties, but a property is never an
   unconditional rejection. It must synthesize a parser-valid expression whose
   folded grammar-token cost is below the same 25-token threshold; the concrete
   expression and cost are stored in the witness. `inputs[N]` and `//` are each
   one grammar token. Exact period and minimum affine-piece count are diagnostic
   unless such a compact witness exists. Exact full-domain `i64` vectors are
   required because byte equality is not a congruence for intermediate `//`
   and `%`. G2 remains reference search only.
5. **Nontriviality and budgets.** Acceptance requires
   `proven_exhaustive_ast_depth >= 3`, no short match in the exact layer,
   `named_families_excluded=true`, and `hybrid_gate_passed=true`. Reaching the
   operator ceiling after depth 3 is reported but does not pretend that deeper
   levels were exhausted.
   Each item has one T2 cap and one T3 cap shared unchanged by E0/E1/E2/E3.
   For batches, each cap family must take at least `min(5, item_count)` distinct
   values. The deterministic budget stratum is representation-independent and
   the resulting caps are published as metadata.
6. **Item non-degeneracy.** Every IR statement executes, every loop enters, and
   perturbing each argument independently at the selected binding changes the
   output.
7. **Semantic avalanche.** Only data/I/O substitutions contribute. Movement and
   syntax corruption do not. T3 mutation positions are balanced and annotated
   as early, middle, or late.
8. **Leakage boundary.** `generate` is public by default and emits one non-secret
   metadata record per item plus task records without answers or oracle
   fingerprints. `--with-answers` is an explicit private export for `mock-run`
   and `score`.
9. **Diagnostics.** Every deterministic candidate rejection is categorized and
   retained in the pre-acceptance histogram.
10. **Size-controlled H1 design.** Items occupy ten declared program-size tiers.
    Tiers add executed reversible workloads and preserve the complete function.
    A 100-item batch is rejected unless T2 prompts provide at least 30 disjoint
    E0↔E2 pairs and 30 disjoint E0↔E3 pairs within 10% `cl100k_base` BPE distance,
     with the E0 member coming from a strictly larger tier.

## Retired gates — ledger

Three acceptance gates have been demoted to recorded covariates. Each demotion
was driven by a measurement, not by convenience, and each is listed here so that
a reader can see in one place what is guaranteed and what is merely observed.
The cumulative count is deliberate: a fourth demotion should be treated as a
signal that the acceptance design, not the individual gate, needs review.

| Gate | Was | Why demoted | Now |
|---|---|---|---|
| `maximum_e2_prompt_bpe_ratio` | E2/E0 prompt BPE ≤ 3.5 | Purpose superseded: the ratio existed to shrink a length confound that the preregistered token-matched-pair analysis instead *controls*. Keeping it on E2 while exempting E3 was also inconsistent. Measured effect: of 7 items accepted after removal, 6 had ratios in 3.55–3.94 and would have been rejected — the gate was biasing the accepted population against larger programs. | Recorded covariate |
| `minimum_text_semantic_density` | source-text density ≥ 0.35 | Measured decay is monotone in program size: 0.463 (tier 0) → 0.076 (tier 7), 0/48 candidates passing at every tier ≥ 2. The floor admitted only tiers 0–1, whose E0 token range [226, 466] is **disjoint** from the E2 range [1102, 2182]. With no overlap the encoding effect is unidentified by matched pairs *and* by regression, so the floor did not make H1 stricter — it made H1 unanswerable. | Recorded covariate; mandatory control in size-tier analysis |
| *(reserved)* | | | |

**Standing rule derived from these cases.** Any acceptance gate defined as a
ratio over program length selects on length. The direction varies —
`text_semantic_density` and `off_idiom_rate` fall with size while the explicit-rung
prompt ratio rises with it — but the selection is unavoidable. A length-normalised
gate must therefore be calibrated per size tier, reformulated in a
length-independent way, or demoted to a covariate. It must not be introduced
without one of those three.

## Open-generator hypothesis-space limitation and rotation policy

Publishing the generator necessarily publishes the hypothesis space induced by
its constructor grammar. For T2, disclosure can turn unconstrained function
recovery into parameter fitting inside a known closed-form family. This is a
structural limitation of an open generator, not something the item-level
public/private answer split can prevent.

The public repository therefore contains the mechanism and a declared public
constructor subset for development and reproduction. Official scoring requires
a separate constructor set reserved at the generator level and kept outside the
public repository, together with private answer keys. The current alpha has not
yet populated or run such an official private constructor set, so it cannot
support official scores or a leaderboard.

Constructor rotation is one-way. A private constructor may be retired and
disclosed for reproduction only after its scoring epoch is closed; once
disclosed, it is never reused for official scoring. Each scoring epoch must
version and hash the public mechanism and public constructor subset, record the
private-set rotation event without publishing its contents, and regenerate all
official items and answer keys. Results from different private rotations must
be reported as separate epochs rather than silently pooled.

Two limitations remain explicit: the hybrid nontriviality certificate currently
supports only arity 1, and the eight promoted public profiles span only four
structural families. The public subset is for development and reproduction, not
an adequate private official-scoring population. Publication records these
limitations; it does not alter the benchmark logic to hide them.

## Current blocking status

The redefined arity-one hybrid gate is operational. A private smoke for
`multiplicative_decomposition` proved AST depth 3, reported the one-million
operator ceiling while attempting deeper levels, produced no compact analytic
witness, and emitted `hybrid_gate_passed=true`. With verbose E3 restored, the
smoke measured E2/E0 as 3.356 total-prompt / 3.981 program-only and E3/E0 as
7.072 total-prompt / 8.681 program-only. After compiler compaction, the
recalibrated ten-tier diagnostic produced at least 30 E0↔E2 and 30 E0↔E3 disjoint
pairs within 10%.

The old population blocker is closed for the public arity-1 instrument.
Token-calibrated concrete witnesses still reject five of the eight old
constructors (each has a 21-grammar-token affine-residue/additive-period form).
The bias-invariant v4 design search tested 3,300 templates: 300 trivial controls
were rejected and global digest deduplication left 1,730 unique semantic functions
in 51 coarse profile buckets. The audit corrected a 108-record overcount; seven buckets
are singletons, 30 mix multiple template families, and the largest contains 250
functions. More precisely, global deduplication removes 1,270 of 3,000 filtered
candidate records; the previous adjacent-only pass removed 1,162 and missed 108.
These buckets are diagnostic signatures, not independent semantic classes.
Eight candidates across four structural families were promoted to IR and pass
complete-domain formula, hybrid-family, private-reference, and one-million-step
Brainfuck regressions. A 600-candidate production-schedule diagnostic measured the
255-input E0 maximum at 5,678,676 steps (tier 9); the pinned execution cap is now
8,000,000, a 40.9% margin. A subsequent seed-42, 100-candidate release probe accepted
85 items and populated every tier: acceptance by tier was
`1,6,8,10,10,10,10,10,10,10`. The only remaining rejection category was
`off_idiom_rate` (15); `worst_case_preflight`, oracle execution, and encoding-budget
rejections were zero. Candidate evaluation took 440.3 seconds total (4.40 s/candidate,
11.58 accepts/minute). The first release batch now contains 100 accepted items across all
ten tiers and eight profiles (largest share 13%). Its manifested pair table contains 39
E0↔E2 and 31 E0↔E3 disjoint pairs; maximum gaps are 8.66% and 7.13%, respectively.
Exact full-domain semantic fingerprints, reference solutions, and normalized IR are each
100/100 unique after global exact-fingerprint rejection was added. This closes the
accepted-population, exact-duplicate, and matched-pair entry gates, not the full release
contract. A deterministic first-20 budget pilot also passes with 12 distinct T2 caps,
12 distinct T3 caps, exact equality across every rendered encoding, and zero caps at the
384/96 safety ceilings. The near-duplicate metric/threshold, remaining pilots/property
evidence, private constructor population, external model runs, and independent
reproduction still do not exist.

A seed-42 production-path probe evaluated 500 candidates: 376 accepted (75.20%) and
124 rejected. First-failure attribution was `off_idiom_rate` for 82 candidates (16.40%)
and `duplicate_semantic_fingerprint` for 42 (8.40%); all other known categories had zero
first-failure hits and remain explicitly reported. Zero is not forced upward: it means a
gate was not the first failure in this sample, not that the gate was absent or invalid.

## Required release evidence

- Fast unit suite and the non-ignored 500-program exhaustive property shard.
- The ignored 10,000-program exhaustive property suite in release mode.
- At least 100 accepted items with class shares, folded-expression audit,
  duplicate/near-duplicate audit, and rejection histogram.
- At least 20 items spanning at least five measured N values, demonstrating
  encoding-invariant item caps and at least five observed values for both T2
  and T3.
- Arity-2 batch evidence proving independent input sampling and per-argument
  perturbation sensitivity.
- Ten occupied size tiers and the manifested `matched-pairs.csv`, with at least
  30 disjoint ±10% BPE pairs for each E0↔E2 and E0↔E3 contrast.
- RLE/expanded/omitted pilot reported separately; omitted is never silently
  mixed into the accepted matched-step ladder.
- Perfect and flawed family-complete controls, human review, contamination
  study, repeated-call uncertainty intervals, and clean-checkout reproduction.

Until every release-evidence item passes, leaderboard results are engineering
diagnostics and not claims about general reasoning ability.
