use crate::web_preset::{is_web_asset_key, WebAssetMap};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

pub(crate) const MAX_CSS_BYTES: usize = 1024 * 1024;

fn validate_path(path: &str) -> Result<(), &'static str> {
    if !is_web_asset_key(path) || !path.starts_with("/assets/css/") {
        return Err("PATH_NOT_AUTHORIZED");
    }
    if !path.to_ascii_lowercase().ends_with(".css") {
        return Err("INVALID_EXTENSION");
    }
    Ok(())
}

pub(crate) fn validate_css_bytes(path: &str, bytes: &[u8]) -> Result<(), &'static str> {
    validate_path(path)?;
    if bytes.len() > MAX_CSS_BYTES {
        return Err("TOO_LARGE");
    }
    std::str::from_utf8(bytes).map_err(|_| "INVALID_UTF8")?;
    Ok(())
}

pub(crate) fn css_content(path: &str, assets: &WebAssetMap) -> Result<String, &'static str> {
    validate_path(path)?;
    let encoded = assets.get(path).ok_or("NOT_FOUND")?;
    let bytes = BASE64_STANDARD.decode(encoded).map_err(|_| "IO_ERROR")?;
    validate_css_bytes(path, &bytes)?;
    String::from_utf8(bytes).map_err(|_| "INVALID_UTF8")
}
