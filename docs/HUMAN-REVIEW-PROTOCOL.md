# Human review protocol

This is a pre-result template for the human-review release gate. It does not authorize a
model run and contains no model result. The final sample sizes and strata must be frozen
in the model-pilot preregistration before responses are inspected.

## Review units and sampling

The immutable run manifest defines the eligible units:
`(run_id, task_id, system_snapshot, repeat)`. Selection must be reproducible from a public
salt and the run-manifest hash, for example by sorting
`SHA-256(manifest_hash || salt || unit_id)` within each frozen stratum.

The preregistration must set counts for:

- T1, T2, and T3;
- every evaluated encoding;
- exact-correct, exact-incorrect, parse-failure, refusal, timeout, and provider-error
  outcomes;
- T1 benign versus critical errors;
- every correct `rho < 1` case, or a deterministic capped census if that set is too large;
- held-out layout/codegen controls when they exist.

No stratum may be added, removed, or resized after aggregate results are seen except by a
versioned amendment that preserves the original selection.

## Blinding and materials

- Reviewers are blinded to provider/system identity, aggregate scores, exact verifier
  outcome, and other reviewer labels during the first pass. Encoding cannot always be
  blinded because representation is part of the prompt.
- The controlled review packet contains the task prompt, raw response, and a minimally
  sufficient local replay. The exact verifier outcome is revealed only after the first
  label is locked, for verifier-agreement review. Private constructor source and unrelated
  oracle fields are excluded.
- Public examples are redacted so they cannot become answer keys for an active scoring
  epoch.

## Labels

Each unit receives structured labels for response relevance, parser/verifier agreement,
first substantive error, wrap/pointer/step semantics, unsupported assumption, criticality,
and whether any apparent compression is genuine rather than formatting or leakage.
Free-text notes support the labels but never replace the exact grader.

Two reviewers label each unit independently. Disagreement is measured per field and
adjudicated by a third reviewer under the same blinding. Human labels diagnose the exact
instrument; they do not overturn exact correctness or act as an LLM-as-judge substitute.

## Release outputs

The public report contains the frozen sampling rule, reviewer counts, agreement,
adjudication rate, aggregate label tables by preregistered stratum, redacted examples, and
all protocol deviations. The private packet retains unit-level labels and answer-bearing
replay material under the same access boundary as raw model responses.

The gate is complete only after the protocol was frozen before inspection, both review
passes and adjudication finished, and the public report can be regenerated from the
private labels without exposing active answer material.
