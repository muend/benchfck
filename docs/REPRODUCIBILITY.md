# Phase 2 reproducibility

The Phase 2 package has two distinct reproducibility claims:

1. **Archival integrity:** the files currently published under `evidence/` match
   `evidence/MANIFEST.txt` byte-for-byte.
2. **Regeneration:** a clean checkout can run the declared commands and recover the same
   scientific content.

The manifest proves the first claim. `scripts/reproduce-phase2.ps1` exercises the second.
Neither claim, by itself, is independent reproduction; that release gate requires a third
party to run the protocol and sign the resulting report.

## Modes

```powershell
# Clean clone, manifest/public-contract verification, and typed JSONL validation.
./scripts/reproduce-phase2.ps1 -Mode Verify

# Also regenerate the seven byte-deterministic artifacts. Expect roughly 10–20 minutes.
./scripts/reproduce-phase2.ps1 -Mode Core

# Also rerun the 500-candidate probe and property-10k population.
# The published measurements indicate roughly 100 minutes on the original machine.
./scripts/reproduce-phase2.ps1 -Mode Full
```

Every run creates a new directory below `target/`; the script refuses to overwrite an
existing workspace. It clones the selected Git commit without local build products,
creates private answer-bearing material only below the clone's ignored `.private/`, and
writes `reproduction-report.json` outside the clone.

## Comparison matrix

| Artifact | Clean verification | Core regeneration | Full comparison |
|---|---|---|---|
| `batch-100-arity1.jsonl` | Manifest + public contract | Exact SHA-256 | Exact SHA-256 |
| `matched-pairs.csv` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `budget-pilot.jsonl` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `near-duplicate-protocol.md` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `duplicate-audit.md` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `carrier-pilot.md` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `leak-scan.md` | Manifest | Exact SHA-256 | Exact SHA-256 |
| `property-10k.log` | Manifest | Not rerun | Scientific fields exact; `elapsed_seconds` normalized |
| `rejection-histogram.md` | Manifest | Not rerun | Counts/tables exact; trace hash and timing fields normalized |

`property-10k.log` intentionally records measured wall time. The candidate trace behind
the rejection histogram records per-candidate elapsed milliseconds; therefore its SHA-256
and the derived timing lines are hardware/runtime observations, not cross-machine
deterministic values. The normalization rules are fixed in the script and do not ignore
acceptance counts, rejection categories, tier distributions, protocol identifiers, or
configuration hashes.

## Independent reproduction gate

The reproducer records commit, platform, PowerShell, Rust, Cargo, mode, and every
comparison result. For the release gate, a person who did not create the evidence must:

1. start from the public commit and an empty workspace;
2. run at least `Core`, and `Full` when time/resources permit;
3. retain the generated JSON report and command log;
4. record any platform difference or failure without editing the expected artifacts;
5. sign and date a short attestation linked from the release report.

Until that happens, the repository provides a reproduction protocol, not an independent
reproduction claim.
