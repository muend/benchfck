//! Public scoring-epoch records and fail-closed lifecycle validation.
//!
//! This module deliberately validates only the public record. It cannot prove
//! that an unpublished constructor bundle is executable or that its private
//! acceptance report is truthful; activation therefore also commits to that
//! report, which remains available to an authorized auditor.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "benchfck.scoring-epoch.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EpochStatus {
    Planned,
    Active,
    Closed,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScoringEpochRecord {
    pub schema_version: String,
    pub epoch_id: String,
    pub status: EpochStatus,
    pub arity: u8,
    pub mechanism_commit: String,
    pub public_config_sha256: String,
    pub private_constructor_count: usize,
    pub private_constructor_commitment_sha256: String,
    pub private_item_batch_commitment_sha256: String,
    pub private_validation_report_sha256: Option<String>,
    pub opened_at_utc: String,
    pub activated_at_utc: Option<String>,
    pub closed_at_utc: Option<String>,
    pub previous_epoch_id: Option<String>,
    pub closure_reason: Option<String>,
    pub disclosure_commit: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_epoch_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'_' | b'-' => index > 0,
            _ => false,
        })
}

fn valid_utc_timestamp(value: &str) -> bool {
    if value.len() != 20 {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return false;
    }
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| {
        value[start..end]
            .parse::<u32>()
            .expect("digit positions were checked")
    };
    (1..=12).contains(&number(5, 7))
        && (1..=31).contains(&number(8, 10))
        && number(11, 13) <= 23
        && number(14, 16) <= 59
        && number(17, 19) <= 59
}

impl ScoringEpochRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported scoring epoch schema {}; expected {SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        if !valid_epoch_id(&self.epoch_id) {
            return Err("epoch_id must match ^[a-z0-9][a-z0-9._-]{2,63}$".into());
        }
        if self.arity == 0 {
            return Err("epoch arity must be at least one".into());
        }
        if !is_lower_hex(&self.mechanism_commit, 40) {
            return Err("mechanism_commit must be 40 lowercase hexadecimal characters".into());
        }
        for (name, value) in [
            ("public_config_sha256", Some(&self.public_config_sha256)),
            (
                "private_constructor_commitment_sha256",
                Some(&self.private_constructor_commitment_sha256),
            ),
            (
                "private_item_batch_commitment_sha256",
                Some(&self.private_item_batch_commitment_sha256),
            ),
            (
                "private_validation_report_sha256",
                self.private_validation_report_sha256.as_ref(),
            ),
        ] {
            if value.is_some_and(|hash| !is_lower_hex(hash, 64)) {
                return Err(format!(
                    "{name} must be null or 64 lowercase hexadecimal characters"
                ));
            }
        }
        if self.private_constructor_count == 0 {
            return Err("private_constructor_count must be at least one".into());
        }
        if !valid_utc_timestamp(&self.opened_at_utc)
            || self
                .activated_at_utc
                .as_ref()
                .is_some_and(|value| !valid_utc_timestamp(value))
            || self
                .closed_at_utc
                .as_ref()
                .is_some_and(|value| !valid_utc_timestamp(value))
        {
            return Err("epoch timestamps must use YYYY-MM-DDTHH:MM:SSZ UTC form".into());
        }
        if self
            .previous_epoch_id
            .as_ref()
            .is_some_and(|id| !valid_epoch_id(id) || id == &self.epoch_id)
        {
            return Err("previous_epoch_id must be a different valid epoch ID".into());
        }
        if self
            .disclosure_commit
            .as_ref()
            .is_some_and(|commit| !is_lower_hex(commit, 40))
        {
            return Err("disclosure_commit must be null or a lowercase 40-character commit".into());
        }
        if self.notes.iter().any(|note| note.trim().is_empty()) {
            return Err("epoch notes cannot contain empty entries".into());
        }
        if self
            .activated_at_utc
            .as_ref()
            .is_some_and(|activated| activated < &self.opened_at_utc)
        {
            return Err("activated_at_utc cannot precede opened_at_utc".into());
        }
        if let (Some(activated), Some(closed)) = (&self.activated_at_utc, &self.closed_at_utc)
            && closed < activated
        {
            return Err("closed_at_utc cannot precede activated_at_utc".into());
        }

        let activation =
            self.activated_at_utc.is_some() && self.private_validation_report_sha256.is_some();
        let closure = self.closed_at_utc.is_some()
            && self
                .closure_reason
                .as_ref()
                .is_some_and(|reason| !reason.trim().is_empty());
        match self.status {
            EpochStatus::Planned => {
                if activation
                    || self.activated_at_utc.is_some()
                    || self.private_validation_report_sha256.is_some()
                    || self.closed_at_utc.is_some()
                    || self.closure_reason.is_some()
                    || self.disclosure_commit.is_some()
                {
                    return Err(
                        "planned epochs cannot contain activation, closure, or disclosure fields"
                            .into(),
                    );
                }
            }
            EpochStatus::Active => {
                if !activation
                    || self.closed_at_utc.is_some()
                    || self.closure_reason.is_some()
                    || self.disclosure_commit.is_some()
                {
                    return Err("active epochs require activation time and validation-report hash, with closure and disclosure fields null".into());
                }
            }
            EpochStatus::Closed => {
                if !activation || !closure || self.disclosure_commit.is_some() {
                    return Err("closed epochs require activation and closure fields, with disclosure_commit null".into());
                }
            }
            EpochStatus::Retired => {
                if !activation || !closure || self.disclosure_commit.is_none() {
                    return Err(
                        "retired epochs require activation, closure, and disclosure fields".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(status: EpochStatus) -> ScoringEpochRecord {
        ScoringEpochRecord {
            schema_version: SCHEMA_VERSION.into(),
            epoch_id: "epoch-001".into(),
            status,
            arity: 1,
            mechanism_commit: "a".repeat(40),
            public_config_sha256: "b".repeat(64),
            private_constructor_count: 8,
            private_constructor_commitment_sha256: "c".repeat(64),
            private_item_batch_commitment_sha256: "d".repeat(64),
            private_validation_report_sha256: None,
            opened_at_utc: "2026-08-16T12:00:00Z".into(),
            activated_at_utc: None,
            closed_at_utc: None,
            previous_epoch_id: None,
            closure_reason: None,
            disclosure_commit: None,
            notes: vec!["test record".into()],
        }
    }

    #[test]
    fn planned_epoch_is_valid_without_claiming_activation() {
        assert!(record(EpochStatus::Planned).validate().is_ok());
    }

    #[test]
    fn active_epoch_requires_private_validation_commitment() {
        let mut epoch = record(EpochStatus::Active);
        assert!(epoch.validate().is_err());
        epoch.activated_at_utc = Some("2026-08-16T12:10:00Z".into());
        epoch.private_validation_report_sha256 = Some("e".repeat(64));
        assert!(epoch.validate().is_ok());
    }

    #[test]
    fn retirement_requires_prior_closure_and_disclosure_commit() {
        let mut epoch = record(EpochStatus::Retired);
        epoch.activated_at_utc = Some("2026-08-16T12:10:00Z".into());
        epoch.private_validation_report_sha256 = Some("e".repeat(64));
        assert!(epoch.validate().is_err());
        epoch.closed_at_utc = Some("2026-09-16T12:00:00Z".into());
        epoch.closure_reason = Some("scoring window ended".into());
        epoch.disclosure_commit = Some("f".repeat(40));
        assert!(epoch.validate().is_ok());
    }

    #[test]
    fn lifecycle_timestamps_are_monotone() {
        let mut epoch = record(EpochStatus::Active);
        epoch.activated_at_utc = Some("2026-08-15T12:00:00Z".into());
        epoch.private_validation_report_sha256 = Some("e".repeat(64));
        assert!(epoch.validate().is_err());
    }
}
