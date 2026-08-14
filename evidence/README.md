# Evidence boundary

This directory is reserved for deliberate, release-grade validation artifacts.
Files under `target/` and ad-hoc outputs elsewhere are diagnostics, never evidence.

**Published release artifacts: 4.** Phase 2 has begun after all 2.0 entry gates passed:

- `batch-100-arity1.jsonl`: 100 accepted public items/tasks across 8 profiles and 10 tiers.
- `matched-pairs.csv`: 39 E0↔E2 and 31 E0↔E3 disjoint T2 prompt pairs, all within 10% BPE.
- `budget-pilot.jsonl`: deterministic first-20 audit with 12 distinct T2 and 12 distinct
  T3 caps, zero caps at the 384/96 ceilings, and exact encoding invariance.
- `rejection-histogram.md`: 500 production-path candidates with every known first-failure
  category reported, including zero-hit gates; 376 accepted and 124 rejected.

These artifacts close only the arity-1 population, matched-pair, budget-diversity, and
candidate rejection-report gates. They are not model results or a completed
release-evidence package. `MANIFEST.txt` is policy
infrastructure, not a result artifact.

Every evidence artifact except `MANIFEST.txt` must be listed in `MANIFEST.txt` as:

```text
<lowercase SHA-256><two spaces><path relative to the repository root>
```

Phase 2 evidence must not be generated until gates 2.0a, 2.0b, and 2.0c pass.
