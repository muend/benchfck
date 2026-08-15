//! benchfck is a Brainfuck-based benchmark generator and exact execution harness.
//! It measures machine-state tracking, structure extraction, causal reasoning,
//! and computation compression. It does not use model-based grading.

use std::fmt::Write;

pub mod backend;
pub mod bf;
pub mod carrier_pilot;
pub mod compiler;
pub mod config;
pub mod generator;
pub mod ir;
pub mod leak_scan;
pub mod metrics;
pub mod near_duplicate;
pub mod oracle;
pub mod property;
pub mod schema;
pub mod tasks;

/// Encode bytes as stable lowercase hexadecimal without relying on digest
/// output types implementing formatting traits.
pub fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

/// F16: 600 production-schedule candidates measured a maximum 255-input E0
/// runtime of 5,678,676 steps at tier 9. Eight million leaves 40.9% margin.
pub const PINNED_STEP_CAP: u64 = 8_000_000;

pub const PINNED_SEMANTICS: &str = "cell = 8-bit unsigned, wraps at 255→0 and 0→255\ntape = 30,000 cells, pointer starts at 0, moving left of 0 is a hard error\n`,` on exhausted input sets the cell to 0\nstep cap = 8,000,000; exceeding it classifies the run as `NON_TERMINATING`";

#[cfg(test)]
mod tests {
    #[test]
    fn lowercase_hex_encoding_is_stable() {
        assert_eq!(
            super::lower_hex(&[0x00, 0x01, 0x0f, 0x10, 0xab, 0xff]),
            "00010f10abff"
        );
    }
}
