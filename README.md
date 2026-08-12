# benchfck

[![version](https://img.shields.io/badge/version-0.4.0--alpha-orange)](https://github.com/muend/benchfck)
[![status](https://img.shields.io/badge/status-engineering_candidate-6f42c1)](VALIDITY.md)
[![model results](https://img.shields.io/badge/model_results-none-lightgrey)](VALIDITY.md)
[![release evidence](https://img.shields.io/badge/release_evidence-0-lightgrey)](evidence/README.md)
[![CI](https://github.com/muend/benchfck/actions/workflows/ci.yml/badge.svg)](https://github.com/muend/benchfck/actions/workflows/ci.yml)
[![CodeQL](https://github.com/muend/benchfck/actions/workflows/codeql.yml/badge.svg)](https://github.com/muend/benchfck/actions/workflows/codeql.yml)
[![license](https://img.shields.io/badge/code-Apache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2024-b7410e?logo=rust)](Cargo.toml)

## What this is

`benchfck` is a Brainfuck-based generator and exact Rust harness for producing,
validating, and scoring machine-state tasks over controlled instruction encodings. It
generates programs and program–input items rather than shipping a fixed dataset. Program
execution, cross-encoding equivalence, answer verification, and metric production are
deterministic; no learned judge is used.

> [!IMPORTANT]
> **v0.4.0-alpha engineering candidate. No model results have been produced. No
> leaderboard exists. Release gates in [`VALIDITY.md`](VALIDITY.md) are unmet.**

```mermaid
flowchart LR
    S["Seed + public constructor"] --> IR["Typed IR"]
    IR --> C["Brainfuck compiler"]
    C --> E0["E0 canonical stream"]
    C --> E1["E1 permuted symbols"]
    E0 --> D["Independent parser"]
    D --> E2["E2 compact explicit ops"]
    D --> E3["E3 verbose explicit ops"]
    IR --> X{"Exact equivalence<br/>over 256 inputs"}
    E0 --> X
    E2 --> X
    E3 --> X
    X -->|pass| T["T1 · T2 · T3 tasks"]
    X -->|fail| R["Reject candidate"]
    T --> V["Restricted verifier + separate metrics"]
```

## Pinned execution semantics

This block is included verbatim in every emitted task prompt and enforced by the
executors:

```text
cell = 8-bit unsigned, wraps at 255→0 and 0→255
tape = 30,000 cells, pointer starts at 0, moving left of 0 is a hard error
`,` on exhausted input sets the cell to 0
step cap = 1,000,000; exceeding it classifies the run as `NON_TERMINATING`
```

## Encoding ladder

Each rung preserves execution semantics while changing how operands and operations are
written. The snippets below are illustrative syntax, not a benchmark item or answer key.

| Rung | Representation | Illustrative line | Isolated intervention |
|---|---|---|---|
| **E0** | Canonical Brainfuck | `>>>>+[-<+>]` | Implicit pointer-relative addressing |
| **E1** | Per-item symbol permutation plus operational legend | `+[[.<,]` | Surface-symbol identity |
| **E2** | Compact explicit operations, RLE carrier | `M23*18` | Explicit cell addressing |
| **E3** | Verbose explicit operations, RLE carrier | `MOVE cell 23 count 18` | Lexeme length and descriptiveness |

```mermaid
flowchart TB
    E0["E0 · symbols + implicit address"] --> E1["E1 · permuted symbols + legend"]
    E1 --> E2["E2 · compact explicit address"]
    E2 --> E3["E3 · verbose explicit address"]
```

![Measured diagnostic BPE ratios](docs/assets/encoding-ratios.svg)

The chart reports one engineering diagnostic measured on **2026-08-11** with
`cl100k_base`; it is not release evidence and contains no task score. Ratios are relative
to E0. Total-prompt ratios are used for matching because the entire prompt is consumed;
program-only ratios describe intervention strength.

| Contrast | Prompt BPE ratio | Program BPE ratio | Analysis rule |
|---|---:|---:|---|
| E2 / E0 | 3.356× | 3.981× | Acceptance ceiling 3.5×; also report token covariate |
| E3 / E0 | 7.072× | 8.681× | No ratio ceiling; only preregistered token-matched pairs |

## Task families

**T1 — State tracking.** Given an encoding, input, and an interior execution step, return
the requested machine-state fields. Three seed-rotated probes are emitted per encoding;
first divergence and error criticality remain separate diagnostics.

**T2 — Computation compression.** Return a restricted arithmetic expression that matches
the program over the complete input domain. Responses are parsed into a safe AST, constant
folded, and exhaustively checked; arbitrary submitted code is never executed. The hybrid
nontriviality certificate is exact only through its measured enumeration depth and then
adds named, witness-producing analytical exclusions.

**T3 — Causal mutation.** Predict the output after a specified program mutation. Mutation
position is recorded as an early/middle/late covariate, and each item has one response cap
shared across E0–E3.

**T4–T6 are reserved.** Their schema identifiers exist, but no official task, score, or
claim is attached to them in this alpha.

## Acceptance gates

Configured values come from [`config/defaults.toml`](config/defaults.toml); the full
validity contract is [`VALIDITY.md`](VALIDITY.md).

| Gate | Current value | Scope |
|---|---:|---|
| Cell model | unsigned 8-bit, wrapping | all executions |
| Tape / left boundary | 30,000 cells / hard error | all executions |
| Step cap | 1,000,000 | all executions |
| Maximum input arity | 2 | generation; exhaustive domains are 256 or 65,536 |
| Layout disciplines | 4 | three generation layouts + explicit held-out layout |
| Statement templates target | 3 | compiler diversity |
| Per-argument sensitivity | required | candidate acceptance |
| IR ≡ E0 ≡ E2 ≡ E3 | full input domain | candidate acceptance |
| Trace semantic density | ≥ 0.30 | candidate acceptance |
| Source-text semantic density | ≥ 0.35 | candidate acceptance |
| Avalanche score | ≥ 0.60 | candidate acceptance |
| Avalanche sampled positions | ≥ 64 | candidate acceptance |
| Canonical-idiom rate | < 0.08 | candidate acceptance |
| E2/E0 prompt BPE ratio | ≤ 3.5 (`cl100k_base`) | candidate acceptance |
| E3/E0 prompt BPE ratio | exempt | token-matched analysis only |
| T1 probes | 3 per encoding | public task export |
| T2 folded expression threshold | 25 restricted-grammar tokens | hybrid nontriviality gate |
| T2 exact enumerator | target AST depth 7; proven minimum 3 | measured, never reported as depth 24 |
| T2 response safety cap | 384 tokens | one item-level cap across E0–E3 |
| T3 response safety cap | 96 tokens | one item-level cap across E0–E3 |
| Size ladder | 8 labelled tiers | function-preserving reversible work |
| Matched-pair batch gate | ≥30 disjoint pairs per E0↔E2 and E0↔E3 | 100-item batch, ≤10% BPE gap |

## Quick start

The complete local smoke path needs Rust only—no API key or external model service.

```powershell
cargo build --release

cargo run --release -- generate `
  --seed 42 --count 1 --difficulty hard --arity 1 `
  --output target/benchfck-public.jsonl

cargo run --release -- validate `
  --input target/benchfck-public.jsonl

cargo run --release -- generate `
  --seed 42 --count 1 --difficulty hard --arity 1 --with-answers `
  --output target/benchfck-private.jsonl

cargo run --release -- mock-run `
  --input target/benchfck-private.jsonl `
  --output target/benchfck-metrics.jsonl --solver perfect
```

Generated files are diagnostics by default and cannot enter `evidence/`. A future release
evidence run must explicitly pass `--artifact-class evidence`, write below `evidence/`, and
update the SHA-256 manifest.

Verification commands:

```powershell
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release --test property_10k -- --ignored
```

## Evidence status

| Evidence class | Published count | Meaning |
|---|---:|---|
| Model runs | **0** | No external evaluation has been run |
| Release JSONL artifacts | **0** | Phase 2 has not started |
| Leaderboards | **0** | No ranking or aggregate benchmark score exists |
| Engineering diagnostics | local only | Used to test instrumentation; not benchmark evidence |

## What is deliberately absent

- No external model adapter is wired into the repository.
- No answer-bearing item, private constructor, or oracle artifact is published.
- No release dataset or result file is present under `evidence/`.
- No benchmark score, model comparison, leaderboard, or validity claim is reported.
- E4 natural-language rendering exists only as residual-risk diagnostic code and is not an
  official evaluation rung.

## Known limitations

1. **Open-generator hypothesis space.** Publishing a constructor family reveals that
   family to anyone inspecting the source. The public mechanism and a public constructor
   subset support development; official scoring requires a separate, unpublished
   constructor set and a documented one-way rotation policy. That private set has not yet
   been populated or run.
2. **Arity-1 certificate.** The hybrid T2 nontriviality certificate currently rejects
   `arity != 1`; the planned arity-2 evidence batch is therefore unreachable until the
   release scope is narrowed or the certificate is extended.
3. **E3 length control.** E3 intentionally has no 3.5× ratio gate. Any E3 analysis must use
   preregistered token-matched pairs; an unmatched raw contrast is not admissible evidence.
4. **Constructor breadth.** The bias-invariant v4 design search produced 1,730 unique
   semantic functions in 51 coarse profile buckets, correcting a 108-record overcount
   caused by adjacent-only deduplication.
   The largest bucket contains 250 functions, so profile count is not a count of independent
   semantic classes. Eight public constructors were promoted
   across four structural families and pass full-domain, hybrid-gate, reference, and
   Brainfuck step-cap regressions. This closes constructor selection, not accepted-batch
   production or external validity: a pre-optimization 8-item easy diagnostic exhausted
   512 attempts, while the first post-short-circuit single-item diagnostic accepted one
   item after 19 rejections in 78.6 seconds. The 8- and 100-item throughput gates remain
   open. Four families also remain a narrow public subset.
5. **Unmet release work.** The 100-item batch, 10k release property run, external runs, and
   independent clean-room reproduction are outstanding.

## Citation and license

Citation metadata is available in [`CITATION.cff`](CITATION.cff). Software, schemas,
documentation, and the published constructor mechanism are copyright © 2026 Muhammed
Enes Duran and licensed under [Apache-2.0](LICENSE). Public generated datasets, if and
when released, will carry a separate [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)
notice. Private constructors and answer keys are not distributed.

See [`docs/GLOSSARY.md`](docs/GLOSSARY.md) for terminology and
[`SECURITY.md`](SECURITY.md) for the execution boundary.
