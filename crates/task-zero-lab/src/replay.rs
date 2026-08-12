//! Credential-free replay artifacts for Epic 014 Task 5 hosted experiments.
//!
//! This format is intentionally lossy. It retains typed helper decisions and
//! sanitized inference metadata, but has no field for prompts, API keys, or
//! raw provider payloads. A reviewed live run may be converted by copying the
//! typed decisions emitted by the adapter and the report's `inferences`; the
//! resulting JSON is then reviewed before it is admitted as a fixture.

use serde::{Deserialize, Serialize};

use crate::helper::{
    HelperModel, HelperModelOutput, InferenceRecord, ScriptedHelperModel, HELPER_PROTOCOL_VERSION,
};

pub const REPLAY_SCHEMA_VERSION: &str = "task-zero-helper-replay-v1";

/// Wrap a live adapter to retain only its already-typed decisions. The raw
/// round prompt is deliberately neither cloned nor stored.
pub struct DecisionRecorder<M> {
    inner: M,
    decisions: Vec<HelperModelOutput>,
}

impl<M> DecisionRecorder<M> {
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            decisions: Vec::new(),
        }
    }

    pub fn decisions(&self) -> &[HelperModelOutput] {
        &self.decisions
    }
}

impl<M: HelperModel> HelperModel for DecisionRecorder<M> {
    fn decide(&mut self, round_prompt: &str) -> anyhow::Result<HelperModelOutput> {
        match self.inner.decide(round_prompt) {
            Ok(output) => {
                self.decisions.push(output.clone());
                Ok(output)
            }
            Err(error) => {
                // Preserve one replayable, non-sensitive typed marker while
                // returning the original error to the host's live run.
                self.decisions.push(HelperModelOutput::Malformed(
                    "sanitized adapter failure; provider details suppressed".into(),
                ));
                Err(error)
            }
        }
    }

    fn take_inference_record(&mut self) -> Option<InferenceRecord> {
        self.inner.take_inference_record()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SanitizedReplayArtifact {
    pub schema_version: String,
    pub case_id: String,
    pub input_rendering_id: String,
    pub input_rendering_hash: String,
    pub mutation_constraints_hash: String,
    pub input_rendering_provenance: String,
    pub repository_revision: String,
    pub graph_hash: String,
    pub packet_hash: String,
    pub prompt_patterns_hash: String,
    pub model_id: String,
    pub repetition: u32,
    pub policy_hash: String,
    pub prompt_template_version: String,
    pub decisions: Vec<HelperModelOutput>,
    pub inferences: Vec<InferenceRecord>,
}

impl SanitizedReplayArtifact {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == REPLAY_SCHEMA_VERSION,
            "unsupported replay schema"
        );
        anyhow::ensure!(!self.case_id.trim().is_empty(), "case_id is required");
        anyhow::ensure!(
            !self.input_rendering_id.trim().is_empty()
                && !self.input_rendering_hash.is_empty()
                && !self.mutation_constraints_hash.is_empty()
                && !self.input_rendering_provenance.trim().is_empty(),
            "input rendering identity, hash, and provenance are required"
        );
        anyhow::ensure!(
            !self.repository_revision.trim().is_empty(),
            "repository_revision is required"
        );
        anyhow::ensure!(
            !self.graph_hash.is_empty()
                && !self.packet_hash.is_empty()
                && !self.prompt_patterns_hash.is_empty()
                && !self.policy_hash.is_empty(),
            "input hashes are required"
        );
        anyhow::ensure!(!self.model_id.trim().is_empty(), "model_id is required");
        anyhow::ensure!(
            self.prompt_template_version == crate::prompt::PROMPT_TEMPLATE_VERSION,
            "prompt template mismatch"
        );
        anyhow::ensure!(self.repetition > 0, "repetition is one-based");
        anyhow::ensure!(
            self.decisions.len() == self.inferences.len(),
            "one inference record is required per decision"
        );
        for record in &self.inferences {
            anyhow::ensure!(
                record.protocol_version == HELPER_PROTOCOL_VERSION,
                "protocol mismatch"
            );
            anyhow::ensure!(
                record.requested_model == self.model_id,
                "requested model mismatch"
            );
            anyhow::ensure!(!record.prompt_hash.is_empty(), "prompt hash is required");
        }
        Ok(())
    }

    pub fn scripted_model(&self) -> anyhow::Result<ScriptedHelperModel> {
        self.validate()?;
        Ok(ScriptedHelperModel::new(self.decisions.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inference() -> InferenceRecord {
        InferenceRecord {
            round: 1,
            requested_model: "author/model".into(),
            returned_model: "author/model".into(),
            upstream_provider: Some("Pinned".into()),
            provider_order: vec!["Pinned".into()],
            allow_fallbacks: false,
            require_parameters: true,
            data_collection: "deny".into(),
            zdr: true,
            temperature: 0.0,
            output_limit: 128,
            protocol_version: HELPER_PROTOCOL_VERSION.into(),
            prompt_hash: "xxh3-prompt".into(),
            raw_response_hash: "xxh3-response".into(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            elapsed_ms: 12,
            stop_reason: Some("stop".into()),
            validation_diagnostics: vec![],
        }
    }

    #[test]
    fn artifact_round_trips_into_scripted_model_without_sensitive_fields() {
        let artifact = SanitizedReplayArtifact {
            schema_version: REPLAY_SCHEMA_VERSION.into(),
            case_id: "C01".into(),
            input_rendering_id: "manifest:C01:expert".into(),
            input_rendering_hash: "request-hash".into(),
            mutation_constraints_hash: "constraints-hash".into(),
            input_rendering_provenance: "manifest.md §3 C01 expert rendering".into(),
            repository_revision: "abc123".into(),
            graph_hash: "graph-hash".into(),
            packet_hash: "packet-hash".into(),
            prompt_patterns_hash: "patterns-hash".into(),
            model_id: "author/model".into(),
            repetition: 1,
            policy_hash: "policy-hash".into(),
            prompt_template_version: crate::prompt::PROMPT_TEMPLATE_VERSION.into(),
            decisions: vec![HelperModelOutput::Malformed("invalid schema".into())],
            inferences: vec![inference()],
        };
        let json = serde_json::to_string(&artifact).unwrap();
        assert!(!json.contains("api_key"));
        assert!(!json.contains("round_prompt"));
        assert!(!json.contains("raw_response\""));
        let restored: SanitizedReplayArtifact = serde_json::from_str(&json).unwrap();
        restored.scripted_model().unwrap();
    }

    /// The artifact needs no new field to carry the truncation/schema-violation
    /// distinction: `decisions` is `Vec<HelperModelOutput>`, so the adapter's
    /// typed `Truncated` step is retained and replayed as itself, distinctly
    /// from `Malformed`. This test pins that so the distinction cannot be
    /// silently flattened by a future schema change.
    #[test]
    fn artifact_retains_truncated_and_malformed_as_distinct_replayable_steps() {
        let mut record = inference();
        record.stop_reason = Some("length".into());
        let artifact = SanitizedReplayArtifact {
            schema_version: REPLAY_SCHEMA_VERSION.into(),
            case_id: "C01".into(),
            input_rendering_id: "manifest:C01:expert".into(),
            input_rendering_hash: "request-hash".into(),
            mutation_constraints_hash: "constraints-hash".into(),
            input_rendering_provenance: "manifest.md §3 C01 expert rendering".into(),
            repository_revision: "abc123".into(),
            graph_hash: "graph-hash".into(),
            packet_hash: "packet-hash".into(),
            prompt_patterns_hash: "patterns-hash".into(),
            model_id: "author/model".into(),
            repetition: 1,
            policy_hash: "policy-hash".into(),
            prompt_template_version: crate::prompt::PROMPT_TEMPLATE_VERSION.into(),
            decisions: vec![
                HelperModelOutput::Truncated("stopped at the output limit".into()),
                HelperModelOutput::Malformed("invalid schema".into()),
            ],
            inferences: vec![record, inference()],
        };
        let json = serde_json::to_string(&artifact).unwrap();
        assert!(json.contains(r#""step":"truncated""#));
        assert!(json.contains(r#""step":"malformed""#));
        let restored: SanitizedReplayArtifact = serde_json::from_str(&json).unwrap();
        let mut model = restored.scripted_model().unwrap();
        assert!(matches!(
            model.decide("sanitized offline replay").unwrap(),
            HelperModelOutput::Truncated(_)
        ));
        assert!(matches!(
            model.decide("sanitized offline replay").unwrap(),
            HelperModelOutput::Malformed(_)
        ));
    }

    #[test]
    fn artifact_rejects_unpaired_or_wrong_model_metadata() {
        let mut record = inference();
        record.requested_model = "other/model".into();
        let artifact = SanitizedReplayArtifact {
            schema_version: REPLAY_SCHEMA_VERSION.into(),
            case_id: "C01".into(),
            input_rendering_id: "manifest:C01:expert".into(),
            input_rendering_hash: "request-hash".into(),
            mutation_constraints_hash: "constraints-hash".into(),
            input_rendering_provenance: "manifest.md §3 C01 expert rendering".into(),
            repository_revision: "abc".into(),
            graph_hash: "g".into(),
            packet_hash: "p".into(),
            prompt_patterns_hash: "q".into(),
            model_id: "author/model".into(),
            repetition: 1,
            policy_hash: "h".into(),
            prompt_template_version: crate::prompt::PROMPT_TEMPLATE_VERSION.into(),
            decisions: vec![],
            inferences: vec![record],
        };
        assert!(artifact.validate().is_err());
    }
}
