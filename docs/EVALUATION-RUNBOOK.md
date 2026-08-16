# Evaluation runbook

This runbook defines the boundary between offline harness validation and any paid model
evaluation. It is operational guidance, not a preregistration and not evidence that a
model run occurred.

## Current authorization boundary

The project owner resumed the planning phase on 2026-08-16 and approved the three-system,
H1-only, 2,160-call pilot design with a $40 pilot ceiling. The immutable plan is published
as `evidence/preregistration.md`. No provider call has occurred, and freezing the plan does
not authorize one. Paid evaluation remains blocked on the answer-stripped provider runner,
offline controls, account/billing checks, scoring-epoch activation, and an explicit start
authorization immediately before the first call. The provisional $700 full-run ceiling
requires a separate post-pilot go/no-go.

## Offline work allowed before paid execution

1. Verify the hashes and structural contract of the published Phase 2 package:

   ```powershell
   ./scripts/verify-evidence.ps1
   ```

2. With the ignored private batch present, exercise the exact perfect and flawed mock
   paths over a family-complete item:

   ```powershell
   ./scripts/verify-local-controls.ps1
   ```

   Outputs stay under `target/local-controls/` and are diagnostics, never evidence.

3. Run formatting, lint, unit, CLI, and the release-mode 500-program property shard.
   These checks use no model service and incur no provider cost.

## Resume gate

The paid phase may start only after all of the following are true:

- the final hypotheses, outcomes, exclusions, repeat count, decoding policy, stopping
  rule, and analysis plan are frozen in a preregistration before any response is seen;
- exact provider model identifiers, context/output limits, pricing, and data-retention
  terms are recorded from current primary documentation;
- the project owner approves both the pilot matrix and a hard spend ceiling;
- credentials are supplied at runtime through ignored environment configuration and are
  never written to prompts, logs, artifacts, command history, or Git;
- a network-disabled dry run proves task selection, ordering, resume/idempotency,
  response serialization, exact scoring, and failure classification without replacing
  real responses with answers from the private export.

## Fail-closed execution boundary

Provider requests may contain only the public task prompt and provider-required decoding
parameters. Private item records, expected answers, semantic fingerprints, reference
solutions, verifier details, and mock outputs remain local. Raw responses are immutable;
normalization and exact scoring produce separate derived artifacts. Retries preserve the
same task/system/repeat identity and are recorded rather than silently replacing failed
attempts.

Stop the run when authorization, provenance, model identity, pricing, output caps, or
response persistence differs from the frozen plan. A partial run is retained and labeled
partial; it is never silently promoted to a complete benchmark result.
