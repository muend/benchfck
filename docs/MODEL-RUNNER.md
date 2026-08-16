# Offline model-runner boundary

The public runner prepares and validates provider work without contacting a provider.
It is an execution control, not a model adapter, response set, score, or authorization to
spend. All outputs are diagnostics below `target/`; they cannot enter `evidence/` through
these commands.

## Frozen system manifest

`config/model-pilot-systems.json` is the machine-readable counterpart of the frozen
preregistration. The loader requires exactly these identities and lowest-reasoning modes:

| Slot | Provider | Model | Setting |
|---|---|---|---|
| M1 | OpenAI | `gpt-5.6-terra` | `none` |
| M2 | Anthropic | `claude-sonnet-5` | `disabled` |
| M3 | Google | `gemini-3.5-flash` | `minimal` |

A changed model, order, provider, or setting fails closed. A legitimate substitution
requires a preregistration amendment rather than an edit hidden inside a run.

## Network-disabled planning

```powershell
cargo run --release -- model-plan `
  --input <answer-stripped-packet.jsonl> `
  --systems config/model-pilot-systems.json `
  --scope pilot `
  --output target/model-pilot-plan.jsonl
```

The loader walks raw JSON before typed deserialization and rejects private item records,
duplicate/orphan IDs, blank records, and every forbidden answer/oracle key. Each request
record contains one exact user prompt, no system prompt, an empty tool list, no sampling
override, the frozen reasoning setting, provider transport cap, prompt/request SHA-256,
and a deterministic run ID. Task payloads are never copied into the provider request.

The preregistered private packet produces 1,200 Stage A plus 960 Stage B cells: 2,160
unique requests, 720 per system. Repeating the command over identical inputs must produce
byte-identical JSONL. `--scope matrix-once` serializes all 1,500 tasks for each frozen
system once (4,500 cells), closing the pre-network complete-packet control. `--scope full`
deterministically expands the fixed task matrix to five repeats but does not execute it.

## Immutable attempts and resume

Provider adapters added later must append one `benchfck.model-attempt.v1` record for every
attempt. A delivered record carries the raw response and its SHA-256, provider request ID,
returned model snapshot, finish reason, timestamps, latency, provider token categories,
and cost. Operational errors carry an error code and never masquerade as a model answer.

```powershell
cargo run --release -- model-resume `
  --plan target/model-pilot-plan.jsonl `
  --attempts <immutable-attempt-log.jsonl> `
  --output target/model-pending-attempts.jsonl
```

Resume validation rejects unknown run IDs, changed request hashes, duplicate or
non-contiguous attempts, bad response hashes, and any retry after a delivered response.
It schedules at most three transport retries (four total attempts), never reschedules a
delivered run, and labels an exhausted chain instead of silently replacing it.

No command in this document reads credentials or performs network I/O. Provider adapters,
billing guards, scoring-epoch activation, and explicit pilot-start authorization remain
separate gates.
