use crate::{
    PINNED_STEP_CAP,
    backend::{Bytecode, MoveCarrier},
    bf,
    generator::PROGRAM_SIZE_TIERS,
    schema::BaseItem,
    tasks::{bpe_token_count, t2_prompt_for_source},
};
use std::{collections::BTreeMap, fmt::Write as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExplicitEncoding {
    E2,
    E3,
}

#[derive(Clone, Debug)]
struct Measurement {
    tier: u8,
    item_id: String,
    carrier: MoveCarrier,
    encoding: ExplicitEncoding,
    program_chars: usize,
    program_bpe: u64,
    prompt_chars: usize,
    prompt_bpe: u64,
    steps: u64,
    reference_steps: u64,
}

#[derive(Clone, Copy, Debug)]
struct RatioStats {
    min: f64,
    median: f64,
    max: f64,
}

fn carrier_label(carrier: MoveCarrier) -> &'static str {
    match carrier {
        MoveCarrier::Rle => "rle",
        MoveCarrier::Expanded => "expanded",
        MoveCarrier::Omitted => "omitted",
    }
}

fn encoding_label(encoding: ExplicitEncoding) -> &'static str {
    match encoding {
        ExplicitEncoding::E2 => "E2",
        ExplicitEncoding::E3 => "E3",
    }
}

fn ratio_stats(mut values: Vec<f64>) -> Result<RatioStats, String> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err("carrier pilot ratio population is empty or non-finite".into());
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    Ok(RatioStats {
        min: values[0],
        median,
        max: values[values.len() - 1],
    })
}

fn selected_items(items: &[BaseItem]) -> Result<Vec<&BaseItem>, String> {
    let mut by_tier = BTreeMap::new();
    for item in items {
        by_tier
            .entry(item.annotations.program_size_tier)
            .or_insert(item);
    }
    let missing = (0..PROGRAM_SIZE_TIERS)
        .filter(|tier| !by_tier.contains_key(tier))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "carrier pilot source is missing program-size tiers {missing:?}"
        ));
    }
    Ok((0..PROGRAM_SIZE_TIERS).map(|tier| by_tier[&tier]).collect())
}

fn measure_item(
    item: &BaseItem,
    tokenizer: &str,
    t2_token_cap: u32,
) -> Result<Vec<Measurement>, String> {
    let baseline = bf::execute(&item.encodings.e0, &item.input, PINNED_STEP_CAP, false)
        .map_err(|error| format!("{} E0 execution failed: {error}", item.item_id))?;
    if baseline.state.output != item.expected_output {
        return Err(format!(
            "{} E0 output does not match the item",
            item.item_id
        ));
    }
    if baseline.state.steps != item.annotations.n_steps {
        return Err(format!(
            "{} E0 steps {} do not match annotation {}",
            item.item_id, baseline.state.steps, item.annotations.n_steps
        ));
    }

    let mut rows = Vec::with_capacity(6);
    for carrier in [
        MoveCarrier::Rle,
        MoveCarrier::Expanded,
        MoveCarrier::Omitted,
    ] {
        let bytecode =
            Bytecode::from_e0_with_carrier(&item.encodings.e0, carrier).map_err(|error| {
                format!("{} {carrier:?} construction failed: {error}", item.item_id)
            })?;
        let sources = [
            (ExplicitEncoding::E2, bytecode.e2_source()),
            (ExplicitEncoding::E3, bytecode.e3_source()),
        ];
        for (encoding, source) in sources {
            let parsed = match encoding {
                ExplicitEncoding::E2 => Bytecode::parse_e2(&source),
                ExplicitEncoding::E3 => Bytecode::parse_e3(&source),
            }
            .map_err(|error| {
                format!(
                    "{} {carrier:?} {} parse failed: {error}",
                    item.item_id,
                    encoding_label(encoding)
                )
            })?;
            if parsed != bytecode {
                return Err(format!(
                    "{} {carrier:?} {} parser round-trip mismatch",
                    item.item_id,
                    encoding_label(encoding)
                ));
            }
            let run = parsed
                .execute(&item.input, PINNED_STEP_CAP)
                .map_err(|error| {
                    format!(
                        "{} {carrier:?} {} execution failed: {error}",
                        item.item_id,
                        encoding_label(encoding)
                    )
                })?;
            if run.output != item.expected_output {
                return Err(format!(
                    "{} {carrier:?} {} output mismatch",
                    item.item_id,
                    encoding_label(encoding)
                ));
            }
            match carrier {
                MoveCarrier::Rle | MoveCarrier::Expanded
                    if run.steps != item.annotations.n_steps =>
                {
                    return Err(format!(
                        "{} {carrier:?} {} steps {} do not match E0 {}",
                        item.item_id,
                        encoding_label(encoding),
                        run.steps,
                        item.annotations.n_steps
                    ));
                }
                MoveCarrier::Omitted if run.steps >= item.annotations.n_steps => {
                    return Err(format!(
                        "{} omitted {} steps {} are not below E0 {}",
                        item.item_id,
                        encoding_label(encoding),
                        run.steps,
                        item.annotations.n_steps
                    ));
                }
                _ => {}
            }
            let prompt = t2_prompt_for_source(item, &source, t2_token_cap);
            rows.push(Measurement {
                tier: item.annotations.program_size_tier,
                item_id: item.item_id.clone(),
                carrier,
                encoding,
                program_chars: source.chars().count(),
                program_bpe: bpe_token_count(&source, tokenizer)?,
                prompt_chars: prompt.chars().count(),
                prompt_bpe: bpe_token_count(&prompt, tokenizer)?,
                steps: run.steps,
                reference_steps: item.annotations.n_steps,
            });
        }
    }
    Ok(rows)
}

fn reference<'a>(rows: &'a [Measurement], row: &Measurement) -> Result<&'a Measurement, String> {
    rows.iter()
        .find(|candidate| {
            candidate.tier == row.tier
                && candidate.carrier == MoveCarrier::Rle
                && candidate.encoding == row.encoding
        })
        .ok_or_else(|| {
            format!(
                "missing RLE reference for tier {} {}",
                row.tier,
                encoding_label(row.encoding)
            )
        })
}

/// Runs the preregistered carrier comparison on one deterministic item from
/// every occupied size tier and returns a public, answer-free Markdown report.
pub fn render(
    items: &[BaseItem],
    tokenizer: &str,
    t2_token_cap: u32,
    source_sha256: &str,
) -> Result<String, String> {
    if items.is_empty() {
        return Err("carrier pilot source contains no private items".into());
    }
    if items.iter().any(|item| item.ir.arity != 1) {
        return Err("carrier pilot release source must contain only arity-1 items".into());
    }
    if items
        .iter()
        .any(|item| item.annotations.move_carrier != MoveCarrier::Rle)
    {
        return Err("carrier pilot source must use the official RLE carrier".into());
    }
    let selected = selected_items(items)?;
    let mut rows = Vec::with_capacity(selected.len() * 6);
    for item in &selected {
        rows.extend(measure_item(item, tokenizer, t2_token_cap)?);
    }

    let mut report = String::new();
    writeln!(report, "# Explicit movement-carrier pilot\n").unwrap();
    writeln!(report, "- Schema: `benchfck.carrier-pilot.v1`").unwrap();
    writeln!(report, "- Status: **PASS**").unwrap();
    writeln!(report, "- Private source SHA-256: `{source_sha256}`").unwrap();
    writeln!(report, "- Source items: {} (arity 1)", items.len()).unwrap();
    writeln!(report, "- Selected items: {}", selected.len()).unwrap();
    writeln!(
        report,
        "- Selection rule: first source item in each program-size tier 0 through 9"
    )
    .unwrap();
    writeln!(report, "- Tokenizer: `{tokenizer}`").unwrap();
    writeln!(
        report,
        "- Prompt measured: exact production T2 template with the same item-level response cap"
    )
    .unwrap();
    writeln!(
        report,
        "- Reference carrier: RLE; all ratios below are paired within item and encoding"
    )
    .unwrap();
    writeln!(
        report,
        "- Semantic checks: E0 baseline, E2 parser, and E3 parser all reproduce the item output"
    )
    .unwrap();
    writeln!(report, "- Step checks: RLE and expanded equal E0 weighted steps exactly; omitted is strictly lower for every selected item").unwrap();
    writeln!(report, "- Release interpretation: RLE remains the only official carrier. Expanded and omitted are diagnostics; omitted is never admissible to the matched-step ladder.\n").unwrap();

    writeln!(report, "## Paired BPE ratio summary\n").unwrap();
    writeln!(report, "| Carrier | Encoding | Program BPE / RLE min | median | max | Prompt BPE / RLE min | median | max |").unwrap();
    writeln!(report, "|---|---|---:|---:|---:|---:|---:|---:|").unwrap();
    for carrier in [
        MoveCarrier::Rle,
        MoveCarrier::Expanded,
        MoveCarrier::Omitted,
    ] {
        for encoding in [ExplicitEncoding::E2, ExplicitEncoding::E3] {
            let matching = rows
                .iter()
                .filter(|row| row.carrier == carrier && row.encoding == encoding)
                .collect::<Vec<_>>();
            let program = ratio_stats(
                matching
                    .iter()
                    .map(|row| {
                        reference(&rows, row)
                            .map(|rle| row.program_bpe as f64 / rle.program_bpe as f64)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?;
            let prompt = ratio_stats(
                matching
                    .iter()
                    .map(|row| {
                        reference(&rows, row)
                            .map(|rle| row.prompt_bpe as f64 / rle.prompt_bpe as f64)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?;
            writeln!(
                report,
                "| {} | {} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} | {:.3} |",
                carrier_label(carrier),
                encoding_label(encoding),
                program.min,
                program.median,
                program.max,
                prompt.min,
                prompt.median,
                prompt.max
            )
            .unwrap();
        }
    }

    writeln!(report, "\n## Per-item paired measurements\n").unwrap();
    writeln!(report, "| Tier | Item | Carrier | Encoding | Program chars | Program BPE | / RLE | Prompt chars | Prompt BPE | / RLE | Steps | / E0 |").unwrap();
    writeln!(
        report,
        "|---:|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|"
    )
    .unwrap();
    for row in &rows {
        let rle = reference(&rows, row)?;
        writeln!(
            report,
            "| {} | `{}` | {} | {} | {} | {} | {:.3} | {} | {} | {:.3} | {} | {:.3} |",
            row.tier,
            row.item_id,
            carrier_label(row.carrier),
            encoding_label(row.encoding),
            row.program_chars,
            row.program_bpe,
            row.program_bpe as f64 / rle.program_bpe as f64,
            row.prompt_chars,
            row.prompt_bpe,
            row.prompt_bpe as f64 / rle.prompt_bpe as f64,
            row.steps,
            row.steps as f64 / row.reference_steps as f64,
        )
        .unwrap();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_summary_uses_the_middle_pair_for_even_populations() {
        let stats = ratio_stats(vec![4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.median, 2.5);
        assert_eq!(stats.max, 4.0);
    }
}
