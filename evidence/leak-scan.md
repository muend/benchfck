# Generated-batch leak scan

- Schema: `benchfck.leak-scan.v1`
- Status: **PASS**
- Public source: `evidence/batch-100-arity1.jsonl`
- Public source SHA-256: `554c8b1f04db355f6be97ae8e9c9a755cd4960691b2a24b736e91e20c5cd417e`
- Private source path: omitted from public evidence
- Private source SHA-256: `7b1635baaa72cfdf2f4d238fb72ade16a67d19357ea83d2e4db3815c2fef2ada`
- Public JSONL records: 1610
- Public private-item records: 0
- Public metadata records: 100
- Public task records: 1510
- Private item records: 100
- Duplicate public metadata IDs: 0
- Duplicate public task IDs: 0
- Duplicate private item IDs: 0
- Public/private item-ID mismatches: 0
- Task item IDs without public metadata: 0
- Private path is Git-ignored: true
- Private path is Git-untracked: true

## Recursive forbidden-key audit

The scan walks every object and array in every raw JSONL record before typed deserialization, so unknown extra fields cannot be hidden by schema parsing. Prompt strings are not treated as JSON keys.

| Forbidden public key | Hits |
|---|---:|
| `avalanche_map` | 0 |
| `avalanche_sampling_rate` | 0 |
| `avalanche_score` | 0 |
| `changed` | 0 |
| `compiler` | 0 |
| `e0` | 0 |
| `e1` | 0 |
| `e1_legend` | 0 |
| `e2` | 0 |
| `e3` | 0 |
| `e4` | 0 |
| `encodings` | 0 |
| `expected_answer` | 0 |
| `expected_output` | 0 |
| `full_trace` | 0 |
| `input` | 0 |
| `ir` | 0 |
| `matched_digest_hex` | 0 |
| `matching_expression` | 0 |
| `oracle_fingerprint` | 0 |
| `oracles` | 0 |
| `outcome` | 0 |
| `reference_solution` | 0 |
| `seed` | 0 |
| `semantic_fingerprint` | 0 |
| `t2_nontriviality_witness` | 0 |
| `t2_reference_solution` | 0 |
| `trace` | 0 |
