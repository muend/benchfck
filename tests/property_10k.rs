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

/// CI-only entry point. The workflow fixes both variables and publishes the
/// exact half-open range in the job log. The full release-evidence entry point
/// above remains unchanged.
#[test]
#[ignore = "CI selects one deterministic release-population shard"]
fn ci_release_shard_from_environment() {
    let index_raw = std::env::var("BENCHFCK_PROPERTY_SHARD_INDEX");
    let count_raw = std::env::var("BENCHFCK_PROPERTY_SHARD_COUNT");
    if matches!(&index_raw, Err(std::env::VarError::NotPresent))
        && matches!(&count_raw, Err(std::env::VarError::NotPresent))
    {
        eprintln!("CI property shard skipped because no shard environment was selected");
        return;
    }
    let index = index_raw
        .expect("BENCHFCK_PROPERTY_SHARD_INDEX must accompany BENCHFCK_PROPERTY_SHARD_COUNT")
        .parse::<u64>()
        .expect("BENCHFCK_PROPERTY_SHARD_INDEX must be an integer");
    let count = count_raw
        .expect("BENCHFCK_PROPERTY_SHARD_COUNT must accompany BENCHFCK_PROPERTY_SHARD_INDEX")
        .parse::<u64>()
        .expect("BENCHFCK_PROPERTY_SHARD_COUNT must be an integer");
    let bounds =
        benchfck::property::shard_bounds(benchfck::property::RELEASE_PROGRAMS, index, count)
            .unwrap();
    eprintln!(
        "property release shard {index}/{count}: programs {}..{} ({} programs)",
        bounds.start,
        bounds.end,
        bounds.end - bounds.start
    );
    benchfck::property::validate_shard(benchfck::property::RELEASE_PROGRAMS, index, count).unwrap();
}
