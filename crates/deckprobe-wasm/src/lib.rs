use deckprobe_core::{MemorySource, ProbeOptions};
use js_sys::Uint8Array;
use serde::Serialize;
use wasm_bindgen::prelude::*;

fn to_js_value(value: &impl Serialize) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value.serialize(&serializer).map_err(|error| {
        js_sys::Error::new(&format!("cannot serialize DeckProbe result: {error}")).into()
    })
}

fn options_from_js(options: JsValue) -> Result<ProbeOptions, JsValue> {
    if options.is_null() || options.is_undefined() {
        return Ok(ProbeOptions::default());
    }
    serde_wasm_bindgen::from_value(options).map_err(|error| {
        js_sys::TypeError::new(&format!("invalid DeckProbe options: {error}")).into()
    })
}

/// Probe host-owned bytes. Engine failures are returned as schema-v2 error
/// reports; only an invalid JS options object throws.
///
/// `source_kind` labels where the bytes came from in the report. It defaults to
/// `browser_bytes`; a Node caller that read a file from disk passes
/// `local_file` so the report matches what the native CLI would have written.
#[wasm_bindgen(js_name = probe)]
pub fn probe_js(
    display_name: String,
    bytes: Uint8Array,
    options: JsValue,
    source_kind: Option<String>,
) -> Result<JsValue, JsValue> {
    let options = options_from_js(options)?;
    let bytes = bytes.to_vec();
    let source = MemorySource::with_kind(
        display_name,
        source_kind.as_deref().unwrap_or("browser_bytes"),
        bytes,
    );
    match deckprobe_engine::probe_source(source, options) {
        Ok(report) => to_js_value(&report),
        Err(error) => to_js_value(&deckprobe_engine::error_report(&error)),
    }
}

#[wasm_bindgen(js_name = formats)]
pub fn formats_js() -> Result<JsValue, JsValue> {
    to_js_value(&deckprobe_engine::formats_report())
}

#[wasm_bindgen(js_name = targets)]
pub fn targets_js(format: String) -> Result<JsValue, JsValue> {
    match deckprobe_engine::targets_report(&format) {
        Ok(report) => to_js_value(&report),
        Err(error) => to_js_value(&deckprobe_engine::error_report(&error)),
    }
}

#[wasm_bindgen(js_name = schema)]
pub fn schema_js() -> Result<JsValue, JsValue> {
    match deckprobe_engine::report_schema() {
        Ok(schema) => to_js_value(&schema),
        Err(error) => to_js_value(&deckprobe_engine::error_report(&error)),
    }
}

#[wasm_bindgen]
pub fn version() -> String {
    deckprobe_engine::TOOL_VERSION.to_owned()
}

#[cfg(test)]
mod tests {
    use deckprobe_core::{ProbeLevel, ProbeOptions};

    #[test]
    fn wasm_boundary_uses_the_same_engine_request_defaults() {
        let options = ProbeOptions {
            targets: vec!["@header".to_owned()],
            level: ProbeLevel::Header,
            ..ProbeOptions::default()
        };
        let source = deckprobe_core::MemorySource::with_kind(
            "browser.pdf",
            "browser_bytes",
            &b"%PDF-1.7\n"[..],
        );
        let report = deckprobe_engine::probe_source(source, options).expect("memory report");
        assert_eq!(report.input.source_kind, "browser_bytes");
        assert_eq!(report.driver.id, "pdf");
    }
}
