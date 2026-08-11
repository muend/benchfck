# benchfck glossary

## E0–E3

The encoding ladder presents one execution in four machine-readable forms. E0 is canonical
Brainfuck with implicit pointer-relative operands. E1 applies a per-item symbol permutation
and supplies an operational legend. E2 is a compact, explicit-address instruction stream.
E3 preserves the explicit operations but uses verbose lexemes. E4 is not an official rung.

## T1–T3

T1 asks for machine state at an interior execution step. T2 asks for a restricted arithmetic
expression observationally equivalent to the program over the complete input domain. T3
asks for the output after a specified causal mutation. T4–T6 are schema reservations only.

## N*

N* is an interpolated trace length at which accuracy reaches 50% within a task family and
experimental condition. It is undefined until suitable external-run observations exist;
benchfck currently publishes none.

## ρ

ρ (rho) is a normalized efficiency quantity stored separately from exact correctness and
token counts. It must not be merged with task accuracy into a single leaderboard number.

## Abstraction gain

Abstraction gain measures compression only for a correct T2 response, comparing the
restricted expression representation with the represented computation. It is conditional
on correctness and is never used to rescue an incorrect answer.

## Avalanche

Avalanche is the fraction of eligible semantic-operation mutations that change the program's
observed output. The configured candidate-acceptance floor is 0.60, with at least 64 sampled
positions when full enumeration is not used.

## Semantic density

Trace semantic density is the share of executed E0 steps that are not pointer movement.
Source-text semantic density is the share of E0 source characters that are not movement
characters. Their configured floors are 0.30 and 0.35 respectively.

## Hybrid gate

The T2 hybrid nontriviality gate combines bottom-up observational enumeration through a
measured, genuinely exhausted AST depth with analytical family tests that must synthesize a
concrete parser-valid witness below the folded-expression token threshold. It does not claim
global minimality and is currently available only for arity 1.
