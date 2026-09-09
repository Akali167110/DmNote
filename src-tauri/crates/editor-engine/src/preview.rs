use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;
pub const PREVIEW_SCHEMA_VERSION: u16 = 1;
pub const MAX_PREVIEW_BYTES: usize = 64 * 1024;
pub const MAX_PREVIEW_TARGETS: usize = 512;
pub const TOMBSTONE_CAPACITY: usize = 1_024;
pub const MAX_ACTIVE_PREVIEW_SESSIONS: usize = TOMBSTONE_CAPACITY;

// keyPositionSchema(src/types/key/keys.ts) 필드와 동기 유지
// 제외: count(런타임 파생), layerName·groupId(식별자, 프리뷰 대상 아님)
pub const KEY_POSITION_PATCH_FIELDS: &[&str] = &[
    "dx",
    "dy",
    "width",
    "height",
    "rotation",
    "hidden",
    "activeImage",
    "inactiveImage",
    "soundEnabled",
    "soundPath",
    "soundVolume",
    "activeTransparent",
    "idleTransparent",
    "noteColor",
    "noteOpacity",
    "noteOpacityTop",
    "noteOpacityBottom",
    "noteBorderRadius",
    "noteWidth",
    "noteAlignment",
    "noteEffectEnabled",
    "noteGlowEnabled",
    "noteGlowSyncPaint",
    "noteGlowSize",
    "noteGlowOpacity",
    "noteGlowOpacityTop",
    "noteGlowOpacityBottom",
    "noteGlowColor",
    "noteAutoYCorrection",
    "noteOffsetX",
    "noteOffsetY",
    "noteBorderWidth",
    "noteBorderColor",
    "noteBorderOpacity",
    "noteBorderSide",
    "className",
    "zIndex",
    "counter",
    "backgroundColor",
    "activeBackgroundColor",
    "borderColor",
    "activeBorderColor",
    "backgroundGradient",
    "activeBackgroundGradient",
    "borderGradient",
    "activeBorderGradient",
    "borderWidth",
    "borderRadius",
    "shadow",
    "activeShadow",
    "fontSize",
    "fontColor",
    "activeFontColor",
    "fontGradient",
    "activeFontGradient",
    "graphAnimationEnabled",
    "fontFamily",
    "idleImageFit",
    "activeImageFit",
    "imageFit",
    "idleImageTransform",
    "activeImageTransform",
    "useInlineStyles",
    "displayText",
    "fontWeight",
    "fontItalic",
    "fontUnderline",
    "fontStrikethrough",
];

pub const SPRITE_POSITION_PATCH_FIELDS: &[&str] = &[
    "dx",
    "dy",
    "width",
    "height",
    "rotation",
    "pivot",
    "idleTransform",
    "poses",
    "pressDurationMs",
    "transitionMs",
    "transitionEasing",
    "baseImage",
    "referenceNaturalSize",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    Patch,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::enum_variant_names)]
pub enum PreviewDomain {
    KeyPosition,
    StatPosition,
    GraphPosition,
    KnobPosition,
    SpritePosition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewEnvelope {
    pub schema_version: u16,
    pub session_id: String,
    pub seq: u64,
    pub kind: PreviewKind,
    pub source_label: String,
    pub domain: PreviewDomain,
    pub mode: String,
    pub targets: Vec<u32>,
    pub patch: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewPublishRequest {
    pub schema_version: u16,
    pub session_id: String,
    pub seq: u64,
    #[serde(default = "patch_kind")]
    pub kind: PreviewKind,
    pub domain: PreviewDomain,
    pub mode: String,
    pub targets: Vec<u32>,
    pub patch: Map<String, Value>,
}

pub fn patch_kind() -> PreviewKind {
    PreviewKind::Patch
}
pub fn validate_publish_request(request: &PreviewPublishRequest) -> Result<(), String> {
    if request.schema_version != PREVIEW_SCHEMA_VERSION {
        return Err("unsupported preview schema version".to_string());
    }
    if request.kind != PreviewKind::Patch {
        return Err("editor_preview_publish only accepts patch messages".to_string());
    }
    validate_session_id(&request.session_id)?;
    if request.targets.len() > MAX_PREVIEW_TARGETS {
        return Err(format!(
            "preview target count exceeds {MAX_PREVIEW_TARGETS}"
        ));
    }
    let allowed_fields = match request.domain {
        PreviewDomain::SpritePosition => SPRITE_POSITION_PATCH_FIELDS,
        _ => KEY_POSITION_PATCH_FIELDS,
    };
    if let Some(field) = request
        .patch
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(format!("preview patch field '{field}' is not allowed"));
    }
    for (field, value) in &request.patch {
        if field.ends_with("Gradient") {
            validate_preview_gradient(field, value)?;
        }
    }
    validate_payload_size(request)
}
pub fn validate_preview_gradient(field: &str, value: &Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let Some(spec) = value.as_object() else {
        return Err(format!(
            "preview field '{field}' must be null or a gradient object"
        ));
    };
    let angle_ok = spec
        .get("angle")
        .and_then(Value::as_f64)
        .is_some_and(f64::is_finite);
    if !angle_ok {
        return Err(format!("preview field '{field}' must carry a finite angle"));
    }
    let Some(stops) = spec.get("stops").and_then(Value::as_array) else {
        return Err(format!("preview field '{field}' must carry gradient stops"));
    };
    if !(2..=8).contains(&stops.len()) {
        return Err(format!(
            "preview field '{field}' must contain between 2 and 8 stops"
        ));
    }
    for stop in stops {
        let color_ok = stop
            .get("color")
            .and_then(Value::as_str)
            .is_some_and(|color| !color.trim().is_empty());
        let pos_ok = stop
            .get("pos")
            .and_then(Value::as_f64)
            .is_some_and(|pos| pos.is_finite() && (0.0..=1.0).contains(&pos));
        if !color_ok || !pos_ok {
            return Err(format!(
                "preview field '{field}' stops must carry a color and a pos between 0 and 1"
            ));
        }
    }
    Ok(())
}
pub fn validate_session_id(session_id: &str) -> Result<(), String> {
    Uuid::parse_str(session_id)
        .map(|_| ())
        .map_err(|_| "preview sessionId must be a UUID".to_string())
}
pub fn validate_payload_size(payload: &impl Serialize) -> Result<(), String> {
    let size = serde_json::to_vec(payload)
        .map_err(|error| format!("failed to serialize preview payload: {error}"))?
        .len();
    if size > MAX_PREVIEW_BYTES {
        return Err(format!(
            "preview payload exceeds the {MAX_PREVIEW_BYTES} byte limit"
        ));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn validate_preview_json(request_json: &str, label: &str) -> Result<String, String> {
    let request: PreviewPublishRequest =
        serde_json::from_str(request_json).map_err(|e| e.to_string())?;
    validate_publish_request(&request)?;
    let envelope = PreviewEnvelope {
        schema_version: request.schema_version,
        session_id: request.session_id,
        seq: request.seq,
        kind: request.kind,
        source_label: label.to_string(),
        domain: request.domain,
        mode: request.mode,
        targets: request.targets,
        patch: request.patch,
    };
    validate_payload_size(&envelope)?;
    serde_json::to_string(&envelope).map_err(|e| e.to_string())
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn is_preview_session_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}
