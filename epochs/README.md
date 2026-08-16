# Scoring epoch records

This directory contains public, content-free commitment records for private scoring
epochs. Records conform to [`../schemas/scoring-epoch.schema.json`](../schemas/scoring-epoch.schema.json)
and can be checked with `benchfck validate-epoch`.

| Epoch | Status | Meaning |
|---|---|---|
| [`v0.4-private-001`](v0.4-private-001.json) | `planned` | A private arity-1 population and custodian validation report exist under ignored storage; scoring is not active and no model has been called. |

A commitment does not publish or prove the private material. Activation requires an
authorized custodian to bind the exact validation-report hash and activation timestamp;
preregistration remains a separate prerequisite for any model call.
