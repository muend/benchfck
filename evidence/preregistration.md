# benchfck v0.4 model pilot preregistration — FROZEN

> **FROZEN BEFORE RESPONSES · NO MODEL CALL HAS OCCURRED**
>
> The project owner approved the three-system family, H1-only confirmatory design,
> 2,160-call pilot, $40 pilot ceiling, and provisional $700 full-run ceiling on
> 2026-08-16. The $700 ceiling is not permission to run the full matrix: scale-up requires
> a separate post-pilot go/no-go. Publishing and hashing this document does not itself
> authorize a provider call.

Freeze date: 2026-08-16

Mechanism commit: `dae92411da1d82d73ef01c3c77801f6576934fe7`

Planned scoring epoch: `v0.4-private-001` (public record commit `ba9f5c4`)

Official pilot scope: v0.4 arity 1, engineering model pilot only

## 1. Purpose and claims boundary

This pilot asks whether fixed language-model systems can return exact answers for
machine-state tasks under controlled program encodings. Grading is deterministic and
programmatic. No LLM judge, human preference score, or post-hoc answer interpretation is
admissible.

The scoring constructor family and answer-bearing batch remain private under the planned
epoch commitment. This removes the known public-constructor fitting path, but the pilot
still does **not** create a leaderboard or completed benchmark release. Official scoring
remains blocked until the epoch is activated by an authorized custodian after this
preregistration is frozen; human review and independent reproduction remain separate
release gates.

## 2. Frozen source materials

This document freezes these inputs by digest:

| Material | Frozen identity |
|---|---|
| Public epoch record | `epochs/v0.4-private-001.json` at commit `ba9f5c4` |
| Private item-batch commitment | `9ba00262fc9929372d15998695c746148d6c82bcca6e9fd6c475aa726b57b1cb` |
| Answer-stripped private task packet SHA-256 | `8cbde6ff4940947e7fd4c0c1cf052f2088a8c3a352e972aae81b1abd51bd4dda` |
| Private matched-pair table SHA-256 | `f9991aff179427a3b6c9c50f339caa91dabe7058c01166bb7bf788caea9d1d87` |
| Private validation-report SHA-256 | `b9ad25c2b73494164a165aca375a05b9557702236ea2d0d2ade4f3be9887eab4` |
| Validity contract | `VALIDITY.md` at the preregistration commit |
| Task prompt bytes | Exact `prompt` strings in the retained answer-stripped packet; no custom system prompt |
| Local matching tokenizer | `cl100k_base` |
| Release carrier | RLE only; expanded and omitted remain diagnostic |

The packet contains 100 public-metadata records and 1,500 fixed task records per
model/repeat. Contents remain private until the epoch disclosure policy permits release:

| Family | E0 | E1 | E2 | E3 | Total |
|---|---:|---:|---:|---:|---:|
| T1 | 300 | 300 | 180 | 120 | 900 |
| T2 | 100 | 100 | 60 | 40 | 300 |
| T3 | 100 | 100 | 60 | 40 | 300 |
| **Total** | **500** | **500** | **300** | **200** | **1,500** |

Tier-dependent rendering is part of the frozen design, not missing data.

## 3. Systems and eligibility

The confirmatory pilot uses exactly three fixed systems from three independently trained
model families. A system is eligible only if it:

1. exposes a pinned or stable version identity, and returns a model version that can be
   archived when the request ID alone is not immutable;
2. can run without browsing, tools, code execution, retrieval, or external memory;
3. reports provider-side input/output token usage;
4. accepts an explicit output-token ceiling;
5. is generally available through a documented API at freeze time; and
6. has not been selected using any benchfck response result.

Frozen system identities:

| Slot | Provider | Frozen model ID | Lowest reasoning setting | Status |
|---|---|---|---|---|
| M1 | OpenAI | `gpt-5.6-terra` | `reasoning.effort=none` | approved, frozen |
| M2 | Anthropic | `claude-sonnet-5` | `thinking.type=disabled` | approved, frozen |
| M3 | Google | `gemini-3.5-flash` | `thinking_level=minimal` | approved, frozen |

Selection evidence retrieved 2026-08-16:

- [OpenAI GPT-5.6 Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra)
  is the balanced intelligence/cost member of the current family and exposes `none` as a
  reasoning effort.
- [Claude Sonnet 5](https://platform.claude.com/docs/en/about-claude/models/whats-new-sonnet-5)
  is the speed/intelligence-balanced Claude 5 model; thinking can be explicitly disabled.
- [Gemini 3.5 Flash](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash)
  is a stable, production-scale Flash model. `minimal` is its lowest documented thinking
  level but does not guarantee zero reasoning, so provider reasoning remains part of the
  frozen system rather than a cross-provider controlled variable.

Model substitution after freezing is not allowed. A discontinued model creates a
preregistered amendment with a new hash; it is never silently replaced.

## 4. Decoding and request protocol

Frozen settings:

- one exact task prompt as one user message;
- no custom system message;
- omit temperature, top-p, and top-k for all three systems; current Claude Sonnet 5 and
  Gemini 3.5 Flash reject non-default sampling parameters, so explicit `0` would not be a
  common protocol;
- no provider seed unless the same semantic is supported by all three systems;
- no tools, browsing, code interpreter, retrieval, function calling, or response repair;
- one candidate per request;
- provider transport ceiling: 1,024 total output tokens for T2 and 512 for T1/T3;
- the task's visible answer cap remains enforced locally by the exact verifier and is not
  enlarged by the provider transport ceiling;
- reasoning mode uses the lowest documented setting listed in §3. These settings are not
  semantically identical across providers, so inference is within-system first and the
  three-system aggregate is explicitly conditional on these frozen systems;
- provider-side caching disabled when configurable and separately recorded otherwise;
- five independent API requests per task/system in the full run.

Repeated requests under the default sampling surface measure residual provider/model
nondeterminism; they do not pretend to be independent model families.

## 5. Hypotheses and estimands

### H1 — primary, directional

At approximately equal **total T2 prompt length**, compact explicit addressing (E2) has a
higher probability of an exact T2 solution than implicit Brainfuck addressing (E0) in the
fixed private `v0.4-private-001` population.

- Null: `Δ_E2−E0 ≤ 0`.
- Alternative: `Δ_E2−E0 > 0`.
- Fixed design: the 34 disjoint E0↔E2 rows in the committed private pair table.
- Primary response: exact programmatic T2 correctness; parse failures and refusals are
  incorrect responses.
- Primary estimand: equal-model, equal-pair mean of `correct(E2) − correct(E0)`, averaged
  over five repeats.
- Primary uncertainty: paired cluster bootstrap over pair rows, retaining all systems and
  repeats within a resampled pair; 10,000 deterministic bootstrap draws.
- Decision rule: H1 is supported only if the two-sided 95% interval is entirely above zero.
  An interval containing zero is inconclusive; an interval entirely below zero is evidence
  in the opposite direction.

This estimand is conditional on the three frozen systems and 34 fixed pairs. With only
three systems, it does not identify an effect over all language models.

### H1 sensitivity model

The matched rows deliberately compare a larger-tier E0 item with a smaller-tier E2 item.
Only 5/34 E2 pairs share the same opaque semantic-class label. Therefore the pair contrast is not
described as a pure causal encoding effect. A preregistered logistic sensitivity model
adjusts for:

- log total prompt BPE;
- log `n_steps`;
- program-size tier;
- semantic-class categorical indicators;
- nesting depth and working set;
- pointer volatility;
- minimum loop iterations;
- trace and text semantic density; and
- frozen system indicators.

Pair ID and item/program ID are clustering units. The primary paired estimator remains the
decision statistic; this model reports sensitivity to measured imbalance.

### Secondary representation contrasts

1. E0↔E3 exact T2 correctness on the 34 frozen disjoint token-matched pairs.
2. Same-item E0↔E1 differences for T1/T2/T3 as a surface-symbol contamination diagnostic.
3. Family-specific E2↔E0 effects for M1, M2, and M3.

Secondary intervals are reported with Holm correction within this three-contrast family.
No raw unmatched E0/E2 or E0/E3 mean is used to support H1.

### H2 — blocked confirmatory claim

The roadmap asks whether T1, T2, and T3 measure distinct latent abilities. Three systems ×
five repeats provide only 15 system-run response profiles; repeats are not independent
systems. That sample is insufficient for a defensible confirmatory factor model or IRT
calibration.

For this pilot, H2 is **descriptive only**: report the three family score vectors, their
rank correlations, and repeat stability. Confirmatory factor analysis and IRT remain
blocked until a separate preregistration freezes a substantially larger independent model
panel and performs a power/identifiability check. The pilot must not convert 15 repeated
runs into 15 independent model families.

## 6. Outcomes

### Primary

- T2 `correct`: candidate restricted expression parses, stays inside the accepted grammar
  and cap, and matches the program on the complete 256-input domain.

### Secondary

- T1 exact state correctness at each of the three fixed probes;
- T3 exact counterfactual outcome correctness;
- parse-failure, refusal, truncation, and delivered-error rates;
- provider output tokens and local folded/lexical response tokens, stored separately;
- input tokens, reasoning tokens when exposed, latency, cache status, and monetary cost;
- overhead ratio `ρ = response_tokens / n_ideal` and abstraction gain only under their
  current schema definitions, without treating them as universal cognitive measures;
- accuracy by semantic class, size tier, encoding, task family, and system.

### T1 divergence and hazard analysis

The fixed export has three probes per item/encoding. Adaptive binary search would require
additional model calls and assumes monotone correctness across independently sampled
answers; that assumption is not established. It is therefore **not** part of this frozen
pilot. First-observed failed probe and fixed-probe accuracy are descriptive. Cox/IRT claims
requiring an exact first-divergence time are deferred to a separate design rather than
manufactured from three coarse probes.

## 7. Pilot stages and exact call budget

### Stage A — cost/transport pilot

- deterministic first 20 private-packet metadata items;
- all 400 task records for those items;
- M1/M2/M3;
- one request per task/system;
- **1,200 base calls**.

### Stage B — repeat-variance pilot

- the first private-packet item in each tier 0, 1, 2, and 3, ordered by JSONL appearance;
- all 80 task records for those four items;
- add repeats 2–5 for all three systems;
- **960 additional calls**.

Stages A+B total **2,160 calls**. If no protocol amendment occurs, these calls retain their
preassigned run IDs and count toward the full five-repeat matrix.

The repository's `cl100k_base` tokenizer measures **12,262,137 input tokens** across the
three-system two-stage pilot. The sum of visible task answer ceilings is 319,500 tokens,
using the fixed 128-token local T1 accounting cap and each task's T2/T3 hard cap.

### Full fixed-task run

`1,500 tasks × 3 systems × 5 repeats = 22,500 base calls`.

The same local tokenizer measures **222,553,905 input tokens** for this matrix; visible
task answer ceilings sum to 3,337,170 tokens.

This excludes retries for transport/rate-limit failures and excludes any future adaptive
T1 study. The previous 18,000-call planning estimate is retired.

Using standard non-batch prices reverified from official provider documentation on
2026-08-16—OpenAI Terra $2/$12, Claude Sonnet 5 $2/$10, and Gemini 3.5 Flash $1.50/$9 per
million input/output tokens—the local-token, visible-cap estimates are:

| Scope | OpenAI | Anthropic | Google | Total |
|---|---:|---:|---:|---:|
| Two-stage pilot | $9.45 | $9.24 | $7.09 | **$25.78** |
| Full fixed matrix | $161.72 | $159.49 | $121.29 | **$442.50** |

Sources: [OpenAI pricing](https://developers.openai.com/api/docs/models/gpt-5.6-terra),
[Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing), and
[Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing).

These are planning estimates, not billing guarantees. Provider tokenizers differ, Claude's
current tokenizer may count materially more tokens, Gemini bills thinking as output, and
retry/request overhead is excluded. The approved hard user ceilings therefore include
buffer:

- two-stage pilot: **$40 total, approved**;
- full fixed matrix: **$700 total, provisionally approved as a ceiling only**, requiring a
  separate go/no-go after the pilot.

No Batch API discount is assumed; all three systems use their standard API service class.

## 8. Ordering, stopping, and failure handling

Task order is deterministic but interleaved by hash so time drift does not align with an
encoding or family. For each system and repeat, sort by:

`SHA256("benchfck-v0.4-model-pilot" || system_id || repeat || task_id)`.

No outcome-based early stopping is allowed. Work may pause only for:

- projected cost exceeding the user-approved ceiling;
- provider outage or unresolved transport failure above 2% of attempted calls;
- evidence that the request serializer changed prompt bytes or decoding settings;
- leakage of a private/oracle field;
- model snapshot drift; or
- a safety incident.

Transport errors and rate limits are retried at most three times with recorded exponential
backoff. A final unresolved transport error is missing operational data, not an incorrect
answer. A delivered refusal, prose-only answer, invalid JSON/code, cap truncation, or other
model-produced non-answer is an incorrect response and keeps its own failure label.

If more than 2% of planned cells for one system remain operationally missing, that system's
run is invalidated and reported; cells are not selectively replaced after inspecting
accuracy. Any rerun uses a new run identity and preserves the failed run.

## 9. Analysis and multiplicity

1. Run the primary H1 estimator exactly once on the frozen 34-pair E0↔E2 table.
2. Report absolute probability difference, paired odds ratio, 95% interval, and raw counts.
3. Report each system separately before any equal-weight aggregate.
4. Apply Holm correction only to the three named secondary representation contrasts.
5. Treat all class/tier/covariate subgroup plots as descriptive unless named above.
6. Do not remove parse failures, refusals, truncated outputs, or low-performing systems.
7. Do not tune prompt wording, caps, or decoding after viewing accuracy.
8. Record every amendment with the old hash, new hash, reason, and whether prior responses
   are discarded or retained; outcome-motivated amendments invalidate confirmatory status.

## 10. Runner and data-boundary requirements

The current in-process mock `ModelAdapter` accepts a private `BaseItem`; a provider adapter
must not use that boundary. Before Stage A, implement a provider runner whose request
constructor accepts only the answer-stripped public `TaskRecord` and sends only `prompt`.

Each attempt record must include:

- run, system, repeat, task, item, family, and encoding IDs;
- exact model snapshot and request-parameter object;
- SHA-256 of prompt bytes and raw response bytes;
- raw response, finish reason, provider request ID, timestamps, latency, and retry chain;
- provider input/output/reasoning/cache token usage when available;
- local token counts in separate fields;
- verifier result and typed failure category; and
- cost in provider billing units and normalized currency using the price frozen at call time.

Secrets, private items, expected answers, fingerprints, reference solutions, and oracle
payloads are forbidden from request records and provider logs. Raw runs remain under
`.private/` until a generated-run leak scan and publication decision are complete.

## 11. Controls

Before any provider request:

1. replay perfect and all flawed mocks over every family/encoding represented in Stage A;
2. assert prompt SHA values against the retained answer-stripped packet;
3. assert no request object contains any of the 28 forbidden answer/oracle keys;
4. verify T2/T3 output caps are identical across available encodings of one item;
5. dry-run and serialize all 1,500 requests without network access; and
6. verify resume/idempotency does not schedule an already completed run cell.

## 12. Frozen approvals and remaining execution gates

Approved by the project owner on 2026-08-16:

- [x] M1=`gpt-5.6-terra`, M2=`claude-sonnet-5`, and M3=`gemini-3.5-flash`.
- [x] Lowest-reasoning, within-system-first comparison rule.
- [x] $40 two-stage pilot ceiling.
- [x] Provisional $700 full-run ceiling, without authorizing post-pilot scale-up.
- [x] 2,160-call two-stage pilot design.
- [x] H1 as the only confirmatory hypothesis; H2, Cox, and IRT remain descriptive or
  deferred rather than underidentified confirmatory claims.
- [x] Cross-semantic-class matched-pair limitation is retained explicitly, with the frozen
  sensitivity model and conditional claims boundary in §5.
- [x] Promotion to `evidence/preregistration.md` and manifest hashing.

Operational gates that remain open after freeze:

- [ ] Verify paid API access, billing controls, and rate limits for all three providers.
- [ ] Implement and pass the answer-stripped provider-runner controls in §§10–11.
- [ ] Activate `v0.4-private-001` through a separately authorized custodian action.
- [ ] Obtain an explicit start authorization immediately before making the first paid
  provider call; this freeze alone is not that authorization.
- [ ] After the pilot, present cost, transport, drift, and control evidence and obtain a
  separate go/no-go before any full-matrix call.
