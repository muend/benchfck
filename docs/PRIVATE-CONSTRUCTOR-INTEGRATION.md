# Private constructor provider boundary

benchfck's default CLI uses the published development constructors. Official scoring must
instead compile an unpublished constructor implementation in a separate, ignored Rust
crate and inject it through `generator::ConstructorProvider`.

This is a code boundary, not a data-file format or dynamic-plugin loader. The provider can
construct typed IR, but it cannot replace compilation, input selection, complete-domain
cross-backend validation, nontriviality checks, semantic deduplication, response-budget
checks, tier coverage, or matched-pair gates. Those remain in the single public
`generate_with_provider` pipeline.

## Minimal integration shape

```rust,no_run
use benchfck::{
    config::Defaults,
    generator::{
        BuildSpec, ConstructorCase, ConstructorProvider, generate_with_provider,
    },
    ir::Program,
};

struct PrivateProvider;

impl ConstructorProvider for PrivateProvider {
    fn semantic_profile_count(&self) -> usize {
        8
    }

    fn build(
        &self,
        seed: u64,
        arity: u8,
        size_tier: u8,
    ) -> Result<ConstructorCase, String> {
        let program: Program = build_private_ir(seed, arity, size_tier)?;
        Ok(ConstructorCase {
            program,
            // Use an opaque epoch-local label, never a constructor/formula name.
            semantic_class: opaque_class_for(seed),
            size_tier,
            // Required when the private family is outside the public G2 search.
            private_reference_solution: Some(private_reference_for(seed)),
        })
    }
}

# fn build_private_ir(_: u64, _: u8, _: u8) -> Result<Program, String> {
#     unimplemented!()
# }
# fn opaque_class_for(_: u64) -> String { "epoch-class-00".into() }
# fn private_reference_for(_: u64) -> String { unimplemented!() }
# fn run(spec: &BuildSpec, defaults: &Defaults) -> Result<(), Box<dyn std::error::Error>> {
let private_items = generate_with_provider(spec, defaults, &PrivateProvider)?;
# let _ = private_items;
# Ok(())
# }
```

The private crate should pin benchfck to the exact scoring mechanism commit. It must live
outside the repository or below an ignored private root. Do not add a public CLI flag that
loads arbitrary source, shared libraries, or serialized programs at runtime.

## Fail-closed provider contract

- `semantic_profile_count` is positive and, for a 100-item release population, every
  declared profile must appear.
- `semantic_class` is a 1–64 character lowercase opaque label using `[a-z0-9._-]`; names
  that reveal a formula or constructor family violate the privacy boundary even if they
  pass syntax validation.
- Returned arity and size tier exactly match the requested schedule cell; output arity is
  exactly one for v0.4.
- A supplied private reference is parsed and evaluated over the complete domain. Its digest
  must exactly match the compiled constructor, its constant-folded grammar length must be
  25–384 tokens, and its lexical upper bound must fit the T2 response cap.
- The same mechanism commit, provider source, salt, config, seed, arity, and tier produce
  byte-identical typed IR. Determinism must be checked in the private validation report.
- Provider errors and contract mismatches abort generation. They are configuration faults,
  not candidate rejections that can be sampled around.

## Activation checklist

Before a scoring epoch can become `active`, the trusted custodian must retain and commit to:

1. the exact private provider source/bundle, private salt, dependency lock, and mechanism
   commit;
2. a deterministic repeat showing identical IR for the full scheduled candidate sequence;
3. a 100-item private population passing the same exact-domain, nontriviality, step-cap,
   semantic-profile, ten-tier, duplicate/near-duplicate, budget, matched-pair, and leak
   gates;
4. a private validation report containing commands, toolchain, hashes, counts, failures,
   and deviations;
5. the public epoch record validated with `benchfck validate-epoch`.

The answer-bearing export, provider source, labels-to-constructor mapping, salts, and private
report stay private. A public task packet may be produced only after the recursive leak scan
and must expose no semantic constructor names. A hash commitment makes later substitution
detectable; it does not by itself prove any of these checks passed.

## Current status

The provider boundary is implemented and the default public provider is regression-tested
against the existing deterministic constructor source. No private provider, private
constructor population, validation report, or active scoring epoch exists yet.
