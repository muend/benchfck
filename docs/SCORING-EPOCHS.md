# Private scoring epochs and constructor rotation

Public constructors support development and reproduction; they cannot remain secret
official-scoring hypotheses. An official scoring epoch therefore commits to a separate
private constructor population without publishing its contents.

## Lifecycle

1. **Planned:** create the private set under ignored storage, validate it with the same
   acceptance contract, and record only commitments and counts publicly.
2. **Active:** freeze the mechanism commit, configuration hash, private-set commitment,
   item-batch commitment, scoring window, and predecessor epoch. No constructor may be
   added, replaced, or reused from a retired epoch.
3. **Closed:** stop new scoring, preserve raw run manifests, and record the closing time
   and reason. Results from different epochs remain separate.
4. **Retired:** optionally disclose constructors for reproduction after the scoring window
   is irreversibly closed. Disclosed constructors become public and are never eligible for
   a later official epoch.

The public record conforms to `schemas/scoring-epoch.schema.json`. Hash commitments do not
turn private content into public evidence; they make later substitution detectable.

## Fail-closed rules

- A scoring run whose mechanism commit, configuration hash, or private-set commitment
  differs from the active epoch is rejected rather than silently pooled.
- An epoch cannot become active until the private set passes exact full-domain,
  nontriviality, duplicate/near-duplicate, budget, and leak gates applicable to its arity.
- v0.4 epochs are arity 1. Arity 2 requires the separately versioned v0.5 exact+bivariate
  certificate and its own preregistration.
- Private constructor source, answer keys, item exports, and salts never enter the public
  epoch record.
- Model/provider changes that affect comparability create a new run cohort and, when the
  preregistration requires it, a new epoch; they are not overwritten in place.

No private constructor set or active scoring epoch exists yet. This document and schema
define the policy needed to create one later without changing the current project status.
