# Near-duplicate protocol

- Protocol: `benchfck.near-duplicate.v1`
- Status: frozen before the first near-duplicate audit of the release batch.
- Scope: private `benchfck.item.v3` arity-1 exports with at least 100 unique items.
- Independence statement: no observed pairwise distance from the release batch was used to choose these metrics or thresholds.

## Normalizations

1. **Semantic axis:** evaluate each already-validated canonical reference expression on the complete 256-input domain. A domain point disagrees when its complete output vector differs. The canonical reference is an exact full-domain witness checked against the stored semantic fingerprint; it is not claimed to be globally minimal.
2. **IR axis:** form a multiset of typed AST-node features, retaining variable indices, constants, loop classes, and nesting depth while ignoring variable names and statement order. Remove only the declared size-ladder intervention nodes: copies from an input into scratch variables 9/10 and the paired `+17`/`-17` drains from those scratch variables into variable 2. Distance is multiset Sørensen-Dice distance.
3. **Reference-expression axis:** constant-fold and canonical-render the accepted reference expression, remove the fixed `solve` wrapper, tokenize identifiers/numbers/operators/punctuation, then use Levenshtein distance divided by the longer token sequence.

## Fixed thresholds and linkage

- Semantic-near: disagreement on at most `1/64` of the complete domain (at most 4 of 256 arity-1 inputs).
- IR-near: normalized multiset Sørensen-Dice distance at most `0.10`.
- Reference-near: normalized canonical-expression token edit distance at most `0.10`.
- Pair rule: `semantic-near AND (IR-near OR reference-near)`.
- Exact duplicates are also reported independently on the semantic fingerprint, normalized IR representation, and canonical reference expression.
- Release rule: zero exact semantic duplicates and zero pairs satisfying the near-duplicate pair rule. The audit reports the flagged-pair count and rate over all unordered pairs.

Changing any metric, normalization, threshold, linkage rule, or release rule requires a new protocol version and a new batch; it must not be tuned against an already inspected audit.
