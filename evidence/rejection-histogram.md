# Acceptance rejection histogram

- Schema: `benchfck.rejection-histogram.v1`
- Source trace: `target/rejection-probe-500.jsonl`
- Source SHA-256: `ce90f4e2b2e6d65ce6642c31e98f9e526860c3cdd2a27b9462998d28099f74ba`
- Probe parameters: seed=42, count=1000, candidates=500, difficulty=Hard, arity=1, max_per_cell=derived
- Configuration: `config/defaults.toml` (`b6215fefc2ffdaa6867633ed55fdb80e0ff2d05f2f34af5c7788f3ff0a99f83c`)
- Evaluated candidates: 500
- Accepted: 376
- Rejected: 124
- Acceptance rate: 75.20%
- Total candidate time: 5289.723 s
- Mean candidate time: 10.579 s

Each rejected candidate is attributed only to its first failing gate. A zero-hit row means the gate was not the first failure in this sample; it is reported, not treated as a defect.

| First rejection category | Hits | Share of all candidates |
|---|---:|---:|
| `analytical_triviality_family_match` | 0 | 0.00% |
| `avalanche` | 0 | 0.00% |
| `cross_backend_domain` | 0 | 0.00% |
| `difficulty_band` | 0 | 0.00% |
| `duplicate_semantic_fingerprint` | 42 | 8.40% |
| `encoding_dependent_task_budgets` | 0 | 0.00% |
| `input_selection` | 0 | 0.00% |
| `insufficient_proven_exhaustive_ast_depth` | 0 | 0.00% |
| `nontriviality_enumerator_error` | 0 | 0.00% |
| `off_idiom_rate` | 82 | 16.40% |
| `oracle_execution` | 0 | 0.00% |
| `per_argument_sensitivity` | 0 | 0.00% |
| `reference_expression_not_found` | 0 | 0.00% |
| `semantic_class_quota` | 0 | 0.00% |
| `short_expression_match_within_enumerated_layer` | 0 | 0.00% |
| `size_tier_cell_quota` | 0 | 0.00% |
| `trace_semantic_density` | 0 | 0.00% |
| `worst_case_preflight` | 0 | 0.00% |

| Requested size tier | Candidates | Accepted | Acceptance rate |
|---:|---:|---:|---:|
| 0 | 50 | 8 | 16.00% |
| 1 | 50 | 23 | 46.00% |
| 2 | 50 | 36 | 72.00% |
| 3 | 50 | 41 | 82.00% |
| 4 | 50 | 46 | 92.00% |
| 5 | 50 | 46 | 92.00% |
| 6 | 50 | 44 | 88.00% |
| 7 | 50 | 44 | 88.00% |
| 8 | 50 | 44 | 88.00% |
| 9 | 50 | 44 | 88.00% |
