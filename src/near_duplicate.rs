//! Preregistered duplicate and near-duplicate audit for private item exports.
//!
//! The protocol text and every threshold live in this module so the CLI can
//! write an exact, hashable protocol before it reads a batch.

use crate::{
    ir::{LoopClass, Statement},
    schema::BaseItem,
    tasks,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const PROTOCOL_VERSION: &str = "benchfck.near-duplicate.v1";
pub const SEMANTIC_DISTANCE_DENOMINATOR: usize = 64;
pub const STRUCTURAL_DISTANCE_DENOMINATOR: usize = 10;
pub const REFERENCE_DISTANCE_DENOMINATOR: usize = 10;
pub const RELEASE_MAX_FLAGGED_PAIRS: usize = 0;

pub const PROTOCOL_MARKDOWN: &str = r#"# Near-duplicate protocol

- Protocol: `benchfck.near-duplicate.v1`
- Status: frozen before the first near-duplicate audit of the release batch.
- Scope: private `benchfck.item.v3` arity-1 exports with at least 100 unique items.
- Independence statement: no observed pairwise distance from the release batch was used to choose these metrics or thresholds.

## Normalizations

1. **Semantic axis:** evaluate each already-validated canonical reference expression on the complete 256-input domain. A domain point disagrees when its complete output vector differs. The canonical reference is an exact full-domain witness checked against the stored semantic fingerprint; it is not claimed to be globally minimal.
2. **IR axis:** form a multiset of typed AST-node features, retaining variable indices, constants, loop classes, and nesting depth while ignoring variable names and statement order. Remove only the declared size-ladder intervention nodes: copies from an input into scratch variables 9/10 and the paired `+17`/`-17` drains from those scratch variables into variable 2. Distance is multiset Sørensen-Dice distance.
3. **Reference-expression axis:** constant-fold and canonical-render the accepted reference expression, remove the fixed `solve` wrapper, tokenize identifiers/numbers/operators/punctuation, then use Levenshtein distance divided by the longer token sequence.

## Fixed thresholds and linkage

- Semantic-near: disagreement on at most `1/64` of the complete domain (at most 4 of 256 arity-1 inputs).
- IR-near: normalized multiset Sørensen-Dice distance at most `0.10`.
- Reference-near: normalized canonical-expression token edit distance at most `0.10`.
- Pair rule: `semantic-near AND (IR-near OR reference-near)`.
- Exact duplicates are also reported independently on the semantic fingerprint, normalized IR representation, and canonical reference expression.
- Release rule: zero exact semantic duplicates and zero pairs satisfying the near-duplicate pair rule. The audit reports the flagged-pair count and rate over all unordered pairs.

Changing any metric, normalization, threshold, linkage rule, or release rule requires a new protocol version and a new batch; it must not be tuned against an already inspected audit.
"#;

#[derive(Clone, Debug)]
struct PreparedItem {
    item_id: String,
    semantic_class: String,
    fingerprint: String,
    outputs: Vec<Vec<u8>>,
    ir_features: BTreeMap<String, usize>,
    ir_digest: String,
    reference_tokens: Vec<String>,
    reference_digest: String,
}

#[derive(Clone, Debug)]
pub struct PairAudit {
    pub left_item_id: String,
    pub right_item_id: String,
    pub left_semantic_class: String,
    pub right_semantic_class: String,
    pub semantic_disagreements: usize,
    pub domain_size: usize,
    pub ir_distance_numerator: usize,
    pub ir_distance_denominator: usize,
    pub reference_edit_distance: usize,
    pub reference_distance_denominator: usize,
    pub semantic_near: bool,
    pub ir_near: bool,
    pub reference_near: bool,
    pub flagged: bool,
}

impl PairAudit {
    pub fn semantic_distance(&self) -> f64 {
        self.semantic_disagreements as f64 / self.domain_size.max(1) as f64
    }

    pub fn ir_distance(&self) -> f64 {
        self.ir_distance_numerator as f64 / self.ir_distance_denominator.max(1) as f64
    }

    pub fn reference_distance(&self) -> f64 {
        self.reference_edit_distance as f64 / self.reference_distance_denominator.max(1) as f64
    }
}

#[derive(Clone, Debug)]
pub struct DuplicateAudit {
    pub protocol_sha256: String,
    pub arity: u8,
    pub item_count: usize,
    pub pair_count: usize,
    pub exact_semantic_pairs: usize,
    pub exact_ir_pairs: usize,
    pub exact_reference_pairs: usize,
    pub semantic_near_pairs: usize,
    pub ir_near_pairs: usize,
    pub reference_near_pairs: usize,
    pub flagged_pairs: Vec<PairAudit>,
    pub closest_pair: Option<PairAudit>,
}

impl DuplicateAudit {
    pub fn release_passed(&self) -> bool {
        self.exact_semantic_pairs == 0 && self.flagged_pairs.len() == RELEASE_MAX_FLAGGED_PAIRS
    }
}

pub fn protocol_sha256() -> String {
    crate::lower_hex(&Sha256::digest(PROTOCOL_MARKDOWN.as_bytes()))
}

pub fn audit(items: &[BaseItem]) -> Result<DuplicateAudit, String> {
    if items.is_empty() {
        return Err("duplicate audit requires at least one item".into());
    }
    let arity = items[0].ir.arity;
    if arity != 1 {
        return Err(format!(
            "protocol {PROTOCOL_VERSION} is frozen for arity-1, found arity {arity}"
        ));
    }
    let mut ids = BTreeSet::new();
    let prepared = items
        .iter()
        .map(|item| {
            if item.ir.arity != arity {
                return Err(format!(
                    "mixed arity batch: {} has arity {}, expected {arity}",
                    item.item_id, item.ir.arity
                ));
            }
            if !ids.insert(item.item_id.clone()) {
                return Err(format!("duplicate item id: {}", item.item_id));
            }
            let expected_domain = 256u64.pow(arity as u32);
            if item.oracles.semantic_fingerprint.domain_size != expected_domain {
                return Err(format!(
                    "{} fingerprint domain is {}, expected {expected_domain}",
                    item.item_id, item.oracles.semantic_fingerprint.domain_size
                ));
            }
            let canonical = tasks::canonical_solution(&item.oracles.t2_reference_solution)
                .map_err(|error| format!("{} reference parse: {error}", item.item_id))?;
            let digest =
                tasks::solution_semantic_digest(&item.oracles.t2_reference_solution, item.ir.arity)
                    .map_err(|error| format!("{} reference digest: {error}", item.item_id))?;
            if digest != item.oracles.semantic_fingerprint.digest_hex {
                return Err(format!(
                    "{} canonical reference does not match stored complete-domain fingerprint",
                    item.item_id
                ));
            }
            let outputs =
                tasks::solution_domain_outputs(&item.oracles.t2_reference_solution, item.ir.arity)
                    .map_err(|error| format!("{} reference domain: {error}", item.item_id))?;
            let ir_features = normalized_ir_features(item);
            let ir_serialized = ir_features
                .iter()
                .map(|(feature, count)| format!("{count}\t{feature}\n"))
                .collect::<String>();
            let reference_tokens = expression_tokens(&canonical)?;
            Ok(PreparedItem {
                item_id: item.item_id.clone(),
                semantic_class: item.annotations.grammar_shape.clone(),
                fingerprint: item.oracles.semantic_fingerprint.digest_hex.clone(),
                outputs,
                ir_digest: crate::lower_hex(&Sha256::digest(ir_serialized.as_bytes())),
                ir_features,
                reference_digest: crate::lower_hex(&Sha256::digest(
                    reference_tokens.join("\u{1f}").as_bytes(),
                )),
                reference_tokens,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut exact_semantic_pairs = 0;
    let mut exact_ir_pairs = 0;
    let mut exact_reference_pairs = 0;
    let mut semantic_near_pairs = 0;
    let mut ir_near_pairs = 0;
    let mut reference_near_pairs = 0;
    let mut flagged_pairs = Vec::new();
    let mut closest_pair: Option<PairAudit> = None;
    for left in 0..prepared.len() {
        for right in (left + 1)..prepared.len() {
            let a = &prepared[left];
            let b = &prepared[right];
            exact_semantic_pairs += usize::from(a.fingerprint == b.fingerprint);
            exact_ir_pairs += usize::from(a.ir_digest == b.ir_digest);
            exact_reference_pairs += usize::from(a.reference_digest == b.reference_digest);
            let semantic_disagreements = a
                .outputs
                .iter()
                .zip(&b.outputs)
                .filter(|(x, y)| x != y)
                .count();
            let domain_size = a.outputs.len();
            let (ir_distance_numerator, ir_distance_denominator) =
                multiset_dice_distance(&a.ir_features, &b.ir_features);
            let reference_edit_distance = levenshtein(&a.reference_tokens, &b.reference_tokens);
            let reference_distance_denominator = a
                .reference_tokens
                .len()
                .max(b.reference_tokens.len())
                .max(1);
            let semantic_near = semantic_distance_is_near(semantic_disagreements, domain_size);
            let ir_near = normalized_distance_is_near(
                ir_distance_numerator,
                ir_distance_denominator,
                STRUCTURAL_DISTANCE_DENOMINATOR,
            );
            let reference_near = normalized_distance_is_near(
                reference_edit_distance,
                reference_distance_denominator,
                REFERENCE_DISTANCE_DENOMINATOR,
            );
            let flagged = semantic_near && (ir_near || reference_near);
            semantic_near_pairs += usize::from(semantic_near);
            ir_near_pairs += usize::from(ir_near);
            reference_near_pairs += usize::from(reference_near);
            let pair = PairAudit {
                left_item_id: a.item_id.clone(),
                right_item_id: b.item_id.clone(),
                left_semantic_class: a.semantic_class.clone(),
                right_semantic_class: b.semantic_class.clone(),
                semantic_disagreements,
                domain_size,
                ir_distance_numerator,
                ir_distance_denominator,
                reference_edit_distance,
                reference_distance_denominator,
                semantic_near,
                ir_near,
                reference_near,
                flagged,
            };
            if closest_pair
                .as_ref()
                .is_none_or(|current| pair_rank(&pair) < pair_rank(current))
            {
                closest_pair = Some(pair.clone());
            }
            if flagged {
                flagged_pairs.push(pair);
            }
        }
    }
    let pair_count = prepared.len() * prepared.len().saturating_sub(1) / 2;
    Ok(DuplicateAudit {
        protocol_sha256: protocol_sha256(),
        arity,
        item_count: prepared.len(),
        pair_count,
        exact_semantic_pairs,
        exact_ir_pairs,
        exact_reference_pairs,
        semantic_near_pairs,
        ir_near_pairs,
        reference_near_pairs,
        flagged_pairs,
        closest_pair,
    })
}

fn pair_rank(pair: &PairAudit) -> (usize, usize, usize) {
    (
        pair.semantic_disagreements,
        pair.ir_distance_numerator * 1_000_000 / pair.ir_distance_denominator.max(1),
        pair.reference_edit_distance * 1_000_000 / pair.reference_distance_denominator.max(1),
    )
}

fn semantic_distance_is_near(disagreements: usize, domain_size: usize) -> bool {
    disagreements * SEMANTIC_DISTANCE_DENOMINATOR <= domain_size
}

fn normalized_distance_is_near(
    numerator: usize,
    denominator: usize,
    threshold_denominator: usize,
) -> bool {
    numerator * threshold_denominator <= denominator
}

fn normalized_ir_features(item: &BaseItem) -> BTreeMap<String, usize> {
    fn visit(statement: &Statement, depth: usize, out: &mut BTreeMap<String, usize>) {
        use Statement::*;
        let feature = match statement {
            Set { dst, value } => format!("d{depth}:set:{dst}:{value}"),
            Copy { dst, src } => format!("d{depth}:copy:{dst}:{src}"),
            Add { dst, src } => format!("d{depth}:add:{dst}:{src}"),
            Sub { dst, src } => format!("d{depth}:sub:{dst}:{src}"),
            DrainScaled {
                dst,
                src,
                factor,
                subtract,
            } => format!("d{depth}:drain:{dst}:{src}:{factor}:{subtract}"),
            In { dst } => format!("d{depth}:in:{dst}"),
            Out { src } => format!("d{depth}:out:{src}"),
            While { cond, class, .. } => format!(
                "d{depth}:while:{cond}:{}",
                match class {
                    LoopClass::S0 => "s0",
                    LoopClass::S1 => "s1",
                    LoopClass::S2 => "s2",
                }
            ),
            If { cond, .. } => format!("d{depth}:if:{cond}"),
            IfNonZeroDrain { cond, .. } => format!("d{depth}:if_nonzero_drain:{cond}"),
        };
        *out.entry(feature).or_default() += 1;
        match statement {
            While { body, .. } | If { body, .. } | IfNonZeroDrain { body, .. } => {
                for nested in body {
                    visit(nested, depth + 1, out);
                }
            }
            _ => {}
        }
    }

    let mut features = BTreeMap::new();
    for statement in &item.ir.body {
        if !is_size_ladder_statement(statement, item.ir.arity) {
            visit(statement, 0, &mut features);
        }
    }
    features
}

fn is_size_ladder_statement(statement: &Statement, arity: u8) -> bool {
    match statement {
        Statement::Copy { dst, src } => matches!(*dst, 9 | 10) && *src < arity as usize,
        Statement::DrainScaled {
            dst,
            src,
            factor,
            subtract,
        } => *dst == 2 && *factor == 17 && ((*src == 9 && !subtract) || (*src == 10 && *subtract)),
        _ => false,
    }
}

fn multiset_dice_distance(
    left: &BTreeMap<String, usize>,
    right: &BTreeMap<String, usize>,
) -> (usize, usize) {
    let left_size = left.values().sum::<usize>();
    let right_size = right.values().sum::<usize>();
    let denominator = (left_size + right_size).max(1);
    let intersection = left
        .iter()
        .map(|(feature, count)| count.min(right.get(feature).unwrap_or(&0)))
        .sum::<usize>();
    (denominator.saturating_sub(2 * intersection), denominator)
}

fn expression_tokens(canonical: &str) -> Result<Vec<String>, String> {
    let body = canonical
        .strip_prefix("def solve(inputs):\n    return [")
        .and_then(|source| source.strip_suffix(']'))
        .ok_or_else(|| "canonical solution wrapper changed".to_string())?;
    let chars = body.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }
            tokens.push(chars[start..index].iter().collect());
        } else if ch == '/' && chars.get(index + 1) == Some(&'/') {
            tokens.push("//".into());
            index += 2;
        } else {
            tokens.push(ch.to_string());
            index += 1;
        }
    }
    Ok(tokens)
}

fn levenshtein(left: &[String], right: &[String]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_token) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_token) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_token != right_token));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_threshold_is_inclusive_at_four_of_256() {
        let domain_size = std::hint::black_box(256);
        assert!(semantic_distance_is_near(4, domain_size));
        assert!(!semantic_distance_is_near(5, domain_size));
    }

    #[test]
    fn edit_threshold_is_inclusive_at_ten_percent() {
        let length = std::hint::black_box(10);
        assert!(normalized_distance_is_near(
            1,
            length,
            REFERENCE_DISTANCE_DENOMINATOR
        ));
        assert!(!normalized_distance_is_near(
            2,
            length,
            REFERENCE_DISTANCE_DENOMINATOR
        ));
    }

    #[test]
    fn canonical_tokenizer_excludes_fixed_wrapper_and_keeps_floor_division() {
        let tokens =
            expression_tokens("def solve(inputs):\n    return [(inputs[0]//7+3)%256]").unwrap();
        assert!(!tokens.contains(&"solve".to_string()));
        assert!(tokens.contains(&"//".to_string()));
    }

    #[test]
    fn multiset_dice_distance_has_fixed_ten_percent_boundary() {
        let left = (0..10)
            .map(|index| (format!("f{index}"), 1))
            .collect::<BTreeMap<_, _>>();
        let mut one_edit = left.clone();
        one_edit.remove("f9");
        one_edit.insert("replacement".into(), 1);
        let (numerator, denominator) = multiset_dice_distance(&left, &one_edit);
        assert_eq!((numerator, denominator), (2, 20));
        assert!(numerator * STRUCTURAL_DISTANCE_DENOMINATOR <= denominator);
    }
}
