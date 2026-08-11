# benchfck validity contract (v3 candidate)

Passing unit tests makes the harness operational; it does not by itself make
the benchmark publication-valid.

## Item acceptance constraints

1. **Semantic program space.** The legacy affine-only five-shape grammar is
   forbidden. Accepted programs come from eight declared semantic constructors:
   modulus/quotient composition, parity, threshold, bit-mask equivalence,
   multiplicative decomposition, signed decomposition, quadratic remainder,
   and mixed product composition. A batch must contain at least eight normalized
   classes, no affine-only class, and no class may exceed 25%.
2. **Two density measurements.** Both executed non-movement E0 steps / all E0
   steps (`trace_semantic_density >= 0.30`) and non-movement E0 source characters
   / all E0 source characters (`text_semantic_density >= 0.35`) are mandatory.
3. **Encoding ladder.** RLE is the accepted E2/E3 carrier. Expanded and omitted
   carriers are diagnostic variants. RLE and expanded executions must preserve
   E0 outputs and exact weighted step counts. E2 is compact explicit addressing;
   its matched E2/E0 prompt ratio uses declared `cl100k_base` BPE and must be
   <= 3.5. E3 intentionally uses verbose machine lexemes and has no item-level
   ratio gate: it is admissible only through the preregistered token-matched-pair
   analysis. Total-prompt ratios remain mandatory covariates and drive matching.
   Program-only ratios report the undiluted representation manipulation.
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
10. **Size-controlled H1 design.** Items occupy eight declared program-size tiers.
    Tiers add executed reversible workloads and preserve the complete function.
    A 100-item batch is rejected unless T2 prompts provide at least 30 disjoint
    E0↔E2 pairs and 30 disjoint E0↔E3 pairs within 10% `cl100k_base` BPE distance,
     with the E0 member coming from a strictly larger tier.

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

Two additional blockers remain explicit: the hybrid nontriviality certificate
currently supports only arity 1, and the 450 generated constructor candidates
collapse to four measured semantic clusters rather than the eight required by
the batch gate. Publication records these limitations; it does not alter the
benchmark logic to hide them.

## Current blocking status

The redefined arity-one hybrid gate is operational. A private smoke for
`multiplicative_decomposition` proved AST depth 3, reported the one-million
operator ceiling while attempting deeper levels, produced no compact analytic
witness, and emitted `hybrid_gate_passed=true`. With verbose E3 restored, the
smoke measured E2/E0 as 3.356 total-prompt / 3.981 program-only and E3/E0 as
7.072 total-prompt / 8.681 program-only. The raw eight-tier diagnostic produced
42 E0↔E2 and 41 E0↔E3 disjoint pairs within 10%.

Phase 2 remains blocked while constructor proposals are promoted into IR.
Token-calibrated concrete witnesses still reject five of the eight old
constructors (each has a 21-grammar-token affine-residue/additive-period form).
The generated design search tested 750 templates: 300 trivial control templates
were rejected and 450 coupled-modulus candidates survived in four measured
semantic clusters. No survivor is yet an accepted constructor or evidence.

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
- Eight occupied size tiers and the manifested `matched-pairs.csv`, with at least
  30 disjoint ±10% BPE pairs for each E0↔E2 and E0↔E3 contrast.
- RLE/expanded/omitted pilot reported separately; omitted is never silently
  mixed into the accepted matched-step ladder.
- Perfect and flawed family-complete controls, human review, contamination
  study, repeated-call uncertainty intervals, and clean-checkout reproduction.

Until every release-evidence item passes, leaderboard results are engineering
diagnostics and not claims about general reasoning ability.
