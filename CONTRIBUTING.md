# Contributing to benchfck

benchfck is an engineering candidate, not a validated leaderboard. Keep changes small,
reproducible, and explicit about what they do and do not establish.

## Before opening a change

1. Run `cargo fmt -- --check`.
2. Run `cargo clippy --all-targets -- -D warnings`.
3. Run `cargo test`.
4. Do not commit private constructors, answer keys, oracle-bearing JSONL, `.private/`, or
   internal review material.

Any change to an acceptance gate, evaluator, task contract, encoding, or evidence policy
must update `VALIDITY.md` in the same pull request. A passing test is not, by itself, a new
validity claim.

Contributions intentionally submitted for inclusion are accepted under Apache-2.0, as
described by Section 5 of that license.
