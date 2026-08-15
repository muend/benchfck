# Duplicate and near-duplicate audit

- Schema: `benchfck.duplicate-audit.v1`
- Protocol: `benchfck.near-duplicate.v1` (`sha256:b5d4c891ea03f8588e4ee5a54ae3b4612bb3b7feff3835449740f491002443f4`)
- Private source SHA-256: `7b1635baaa72cfdf2f4d238fb72ade16a67d19357ea83d2e4db3815c2fef2ada` (path and contents are intentionally unpublished)
- Arity: `1`
- Items: `100`
- Unordered pairs: `4950`
- Release result: **PASS**

## Exact duplicate checks

| Axis | Exact-equal pairs |
|---|---:|
| Complete-domain semantic fingerprint | 0 |
| Ladder-normalized IR feature multiset | 0 |
| Canonical reference expression | 0 |

## Fixed-threshold checks

| Check | Pairs |
|---|---:|
| Semantic distance ≤1/64 | 0 |
| Normalized IR distance ≤0.10 | 1358 |
| Canonical reference distance ≤0.10 | 1344 |
| Flagged by semantic AND (IR OR reference) | 0 (0.0000%) |

## Closest pair under the frozen ordering

`item-8ebdd79a41ba59564af5` (residue_square_p5_c11) ↔ `item-3debb99f0ec0e92cd192` (residue_complement_p5_q3): semantic `205/256` (80.0781%), IR `0.1000`, reference `0.1707`, flagged `false`.

## Flagged pairs

None.

Canonical references were re-parsed, constant-folded, evaluated over the complete domain, and required to reproduce each stored semantic fingerprint before pairwise distances were computed.
