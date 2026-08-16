# Technical report: pre-results release scaffold

This document fixes the reporting structure before model results exist. It is not a
completed benchmark paper, model comparison, leaderboard, or release claim. Sections that
depend on external runs remain explicitly marked **NOT RUN**.

## Current claim

benchfck v0.4 is an arity-1 engineering candidate: its generator, exact verifier, public
population package, private planned-epoch population, and offline evidence controls are
operational. No model/provider has been called. Consequently H1/H2, effect sizes,
confidence intervals, human error review, and model-level validity are unresolved.

## Fixed scope and methods

- Cells are unsigned wrapping bytes; tape and input semantics are fixed in `src/lib.rs`.
- Public generation uses eight promoted semantic profiles across ten program-size tiers.
- IR, canonical Brainfuck E0, compact explicit E2, and verbose explicit E3 must agree over
  the complete 256-input arity-1 domain.
- Encoding contrasts are admissible only inside preregistered token-matched pairs with
  prompt length, size tier, layout, and movement density carried into analysis.
- Grading is exact and programmatic. No LLM-as-judge path is permitted.

The detailed acceptance and validity contract is in `VALIDITY.md`; the nine immutable
Phase 2 artifacts and their hashes are under `evidence/`.

## Evidence ledger

| Layer | Status | Release interpretation |
|---|---|---|
| Repository, CI, CodeQL, manifest | PASS | Engineering and archival controls exist |
| 100-item arity-1 population | PASS | Eight profiles and ten tiers; not a private scoring set |
| Duplicate, budget, carrier, leak gates | PASS | Phase 2 population package is internally consistent |
| 10k complete-domain compiler property suite | PASS | Cross-backend compiler evidence, not model evidence |
| Clean-checkout developer reproduction | PASS (Core) | Protocol works for the developer; not independent attestation |
| Private provider execution boundary | IMPLEMENTED | Same public acceptance pipeline; private implementation remains ignored and unpublished |
| Private constructor population | PRIVATE VALIDATION PASS | 100 items, 8 opaque profiles, 10 tiers, 34/34 prompt-matched pairs; content-free commitments only |
| Scoring epoch | PLANNED | `v0.4-private-001`; no activation timestamp/report hash, so official scoring cannot begin |
| Preregistration | NOT FROZEN | No model call is authorized |
| External model pilot | NOT RUN | H1/H2 have no result |
| Blinded human review | NOT RUN | Requires frozen post-run sample |
| Independent third-party reproduction | NOT RUN | Release gate remains open |

## Results placeholders

### H1 — representation ladder

**NOT RUN.** Report paired effect sizes, uncertainty, provider-token/local-token deltas,
layout and tier controls, and all preregistered null/negative outcomes here. Do not replace
an unmatched E0/E2/E3 comparison with the paired analysis.

### H2 — task-family structure

**NOT RUN / confirmatory claim deferred.** The planned three-system panel is too small for
confirmatory factor or IRT claims. Descriptive family results must stay labeled as such.

### Failure taxonomy and human audit

**NOT RUN.** Publish the frozen sample frame, blinded reviewer decisions, agreement,
adjudications, exclusions, and protocol deviations using `docs/HUMAN-REVIEW-PROTOCOL.md`.

## Known limitations

1. The public constructor subset exposes its hypothesis space and is not eligible as an
   unpublished official-scoring population.
2. v0.4 is arity 1 only. Arity 2 requires a memory-safe 65,536-binding exact enumerator and
   a genuinely bivariate certificate in a separately preregistered v0.5 scope.
3. The public profiles remain structurally narrow; profile buckets are not independent
   semantic classes.
4. The evidence establishes engineering behavior, not ecological validity across models,
   languages, providers, or decoding regimes.
5. Wall time and per-candidate timing are platform observations; the reproducer normalizes
   only those declared runtime fields.

## Release gate matrix

| Gate | Required evidence | Owner/action | State |
|---|---|---|---|
| P3 preregistration | Immutable document + hash before any response | Maintainer + user go/no-go | OPEN |
| Private scoring epoch | Executable private population, private gate report, public commitment | Trusted custodian/auditor | PLANNED; ACTIVATION OPEN |
| Model pilot | Immutable responses and provider manifests under approved cap | Maintainer after approval | DEFERRED |
| Statistical report | Preregistered estimates, intervals, null/negative results | Analysis phase | BLOCKED BY PILOT |
| Human review | Two blinded reviewers + adjudication record | Independent reviewers | BLOCKED BY RESULTS |
| Independent reproduction | Signed clean-checkout report | Third party | OPEN |
| Archival release | Version tag and optional DOI after all applicable gates | Maintainer | OPEN |

## Provenance fields for the completed report

Record the repository commit, evidence manifest SHA-256, configuration SHA-256, scoring
epoch ID, preregistration SHA-256, provider/model snapshots, tokenizer versions, seeds,
decoding parameters, retry policy, environment/toolchain versions, reviewer packet hash,
and independent reproduction attestation. Results from different epochs or run cohorts
must never be silently pooled.
