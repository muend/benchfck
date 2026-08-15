/// Central M3 property suite. The release test is ignored in the fast developer
/// set because it performs 2.56 million exhaustive input bindings.
#[test]
fn five_hundred_program_fast_property_shard() {
    benchfck::property::validate_range(benchfck::property::FAST_PROGRAMS).unwrap();
}

#[test]
#[ignore = "extended 10k-program exhaustive compiler validation"]
fn ten_thousand_ir_programs_match_every_backend_over_the_full_domain() {
    benchfck::property::validate_range(benchfck::property::RELEASE_PROGRAMS).unwrap();
}
