# Private scoring epochs and constructor rotation

Public constructors support development and reproduction; they cannot remain secret
official-scoring hypotheses. An official scoring epoch therefore commits to a separate
private constructor population without publishing its contents.

## Lifecycle

1. **Planned:** create the private set under ignored storage, validate it with the same
   acceptance contract, and record only commitments and counts publicly. A planned record
   does not carry an activation time or validation-report hash.
2. **Active:** freeze the mechanism commit, configuration hash, private-set commitment,
   item-batch commitment, scoring window, predecessor epoch, and SHA-256 of the private
   acceptance report. No constructor may be added, replaced, or reused from a retired epoch.
3. **Closed:** stop new scoring, preserve raw run manifests, and record the closing time
   and reason. Results from different epochs remain separate.
4. **Retired:** after the scoring window is irreversibly closed, disclose constructors for
   reproduction and commit that disclosure in the public record. Disclosed constructors
   become public and are never eligible for a later official epoch. A closed set that is
   never disclosed remains `closed`, not `retired`.

The public record conforms to `schemas/scoring-epoch.schema.json`. Hash commitments do not
turn private content into public evidence; they make later substitution detectable.

Constructor and private-batch commitments must be computed over the exact retained bytes
with a private random salt and an explicit domain label; the custodian records the salt,
byte length, and digest procedure in the private validation report. The public config and
validation-report fields are ordinary SHA-256 digests of exact bytes. This separation
prevents a public commitment from becoming a substitute for disclosure or an execution
test while still allowing an authorized auditor to recompute it.

Validate a record without reading private material:

```powershell
cargo run -- validate-epoch --input path/to/public-epoch.json
```

The CLI rejects unknown fields, malformed IDs/hashes/timestamps, impossible lifecycle
combinations, self-referential predecessors, activation without a committed private
validation report, closure without a reason, and retirement without a disclosure commit.

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

## Validation boundary

The public validator proves record structure and lifecycle consistency only. It cannot
establish that an unpublished constructor bundle is executable, that its count is honest,
or that the committed private report passed the exact-domain, nontriviality, duplicate,
budget, and leak gates. An authorized custodian or auditor must retain those materials,
recompute their commitments, and sign the activation decision. The default CLI still uses
hard-coded public Rust constructors. The library-level `ConstructorProvider` boundary lets
an ignored external Rust crate inject typed private IR into the same acceptance pipeline;
it does not load opaque data or prove that a private implementation exists.

No private constructor set or active scoring epoch exists yet. This document, schema, and
validator define the fail-closed public boundary needed to create one later without
changing the current project status.
