# Evidence boundary

This directory is reserved for deliberate, release-grade validation artifacts.
Files under `target/` and ad-hoc outputs elsewhere are diagnostics, never evidence.

**Published release artifacts: 9.** Phase 2 has begun after all 2.0 entry gates passed:

- `batch-100-arity1.jsonl`: 100 accepted public items/tasks across 8 profiles and 10 tiers.
- `matched-pairs.csv`: 39 E0↔E2 and 31 E0↔E3 disjoint T2 prompt pairs, all within 10% BPE.
- `budget-pilot.jsonl`: deterministic first-20 audit with 12 distinct T2 and 12 distinct
  T3 caps, zero caps at the 384/96 ceilings, and exact encoding invariance.
- `rejection-histogram.md`: 500 production-path candidates with every known first-failure
  category reported, including zero-hit gates; 376 accepted and 124 rejected.
- `near-duplicate-protocol.md`: independently frozen semantic/IR/reference metrics,
  thresholds, linkage rule, and zero-flag release rule.
- `duplicate-audit.md`: all 4,950 unordered pairs in the private 100-item batch; zero
  exact semantic/IR/reference pairs and zero pairs flagged by the frozen near rule.
- `property-10k.log`: release-build PASS over 10,000 deterministic IR programs,
  2,560,000 complete-domain bindings, and IR/E0/E2/E3; elapsed 250.773 seconds.
- `carrier-pilot.md`: one paired item from each of ten size tiers; RLE, expanded, and
  omitted E2/E3 renderings independently parse and preserve output. RLE/expanded retain
  exact weighted steps; omitted is strictly lower and remains diagnostic-only.
- `leak-scan.md`: all 1,610 raw public records scanned recursively before typed parsing;
  zero private records or forbidden answer/oracle keys, exact 100-item public/private ID
  equality, unique/non-orphan tasks, and an ignored plus untracked private source.

These artifacts close only the arity-1 population, matched-pair, budget-diversity,
candidate rejection-report, duplicate/near-duplicate, 10k property, carrier-pilot, and
generated-batch leak gates. Together they complete the v0.4 arity-1 Phase 2 population
package. They are not model results, human review, or clean-checkout reproduction.
`MANIFEST.txt` is policy
infrastructure, not a result artifact.

Every evidence artifact except `MANIFEST.txt` must be listed in `MANIFEST.txt` as:

```text
<lowercase SHA-256><two spaces><path relative to the repository root>
```

Phase 2 evidence must not be generated until gates 2.0a, 2.0b, and 2.0c pass.
