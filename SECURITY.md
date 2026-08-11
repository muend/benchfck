# Security policy

## Supported version

Only the latest pre-release branch is maintained. The project has not reached a stable
security-supported release.

## Execution boundary

The harness never executes model-provided Python or arbitrary model-provided code. T2
responses are parsed into a restricted internal expression AST and evaluated by the Rust
verifier. Imports, calls, loops, conditionals, lookup tables, and prose are rejected.

Private constructors, answer keys, oracle-bearing exports, credentials, and internal
diagnostics must never be committed. Report suspected disclosure privately to the repository
owner before opening a public issue.

## Reporting

Use GitHub's private vulnerability-reporting channel when enabled. Until then, contact the
repository owner through the private contact method listed on the `muend` GitHub profile.
