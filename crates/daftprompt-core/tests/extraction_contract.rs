//! The extraction contract must fit Rig's schema-defined `submit` tool.
//!
//! Rig's `ExtractorBuilder<T>` requires `T` to be serializable,
//! deserializable, schema-generating, and thread-safe. These bounds are
//! checked at compile time here so a change to the extraction types cannot
//! silently stop working with the extractor.

use daftprompt_types::{BriefingExtraction, Extraction};
use rig::wasm_compat::{WasmCompatSend, WasmCompatSync};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

fn assert_extractor_output<T>()
where
    T: JsonSchema + DeserializeOwned + Serialize + WasmCompatSend + WasmCompatSync + 'static,
{
}

#[test]
fn extraction_satisfies_the_rig_extractor_bounds() {
    assert_extractor_output::<Extraction>();
}

#[test]
fn briefing_extraction_satisfies_the_rig_extractor_bounds() {
    assert_extractor_output::<BriefingExtraction>();
    let schema = serde_json::to_string(&schemars::schema_for!(BriefingExtraction)).unwrap();
    assert!(schema.contains("commit") && schema.contains("summary"));
}
