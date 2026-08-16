use benchfck::{
    config::Defaults,
    generator::{
        BuildSpec, ConstructorCase, ConstructorProvider, GenerateError, generate_with_provider,
    },
    ir::Program,
    schema::DifficultyBand,
};

struct ExternalProvider;

impl ConstructorProvider for ExternalProvider {
    fn semantic_profile_count(&self) -> usize {
        0
    }

    fn build(&self, _seed: u64, arity: u8, size_tier: u8) -> Result<ConstructorCase, String> {
        Ok(ConstructorCase {
            program: Program {
                arity,
                output_arity: 1,
                variables: vec![],
                body: vec![],
            },
            semantic_class: "epoch-class-00".into(),
            size_tier,
            private_reference_solution: None,
        })
    }
}

#[test]
fn external_crate_can_inject_a_provider_but_cannot_bypass_its_contract() {
    let defaults = Defaults::load("config/defaults.toml").unwrap();
    let result = generate_with_provider(
        &BuildSpec {
            seed: 42,
            count: 1,
            difficulty: DifficultyBand::Easy,
            arity: 1,
            held_out: false,
            max_attempts: Some(1),
            max_items_per_cell: None,
        },
        &defaults,
        &ExternalProvider,
    );
    assert!(matches!(result, Err(GenerateError::ConstructorProvider(_))));
}
