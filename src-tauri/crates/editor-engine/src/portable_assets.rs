use crate::local_asset_path::{file_url_to_path, FileUrlPath};
const APP_DATA_MARKER: &str = "com.dmnote.desktop";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetCategory {
    Sounds,
    Fonts,
    Images,
}

impl AssetCategory {
    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Sounds => "sounds",
            Self::Fonts => "fonts",
            Self::Images => "images",
        }
    }

    fn from_directory_name(value: &str) -> Option<Self> {
        match value {
            "sounds" => Some(Self::Sounds),
            "fonts" => Some(Self::Fonts),
            "images" => Some(Self::Images),
            _ => None,
        }
    }
}

pub fn parse_portable_asset_reference(raw: &str) -> Option<(AssetCategory, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("blob:")
        || lower.starts_with("asset:")
        || lower.starts_with("tauri:")
    {
        return None;
    }

    let portable = match file_url_to_path(trimmed) {
        FileUrlPath::Path(path) => path.to_string_lossy().into_owned(),
        FileUrlPath::Invalid => return None,
        FileUrlPath::NotFileUrl => {
            let bytes = trimmed.as_bytes();
            let windows_drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            let verbatim_drive = lower.starts_with(r"\\?\")
                && bytes.get(4).is_some_and(u8::is_ascii_alphabetic)
                && bytes.get(5) == Some(&b':')
                && bytes
                    .get(6)
                    .is_some_and(|byte| matches!(byte, b'\\' | b'/'));
            let network = trimmed.starts_with(r"\\") || trimmed.starts_with("//");
            if !trimmed.starts_with('/') && !windows_drive && !verbatim_drive && !network {
                return None;
            }
            trimmed.to_string()
        }
    };

    let components = portable
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| *component == "." || *component == "..")
    {
        return None;
    }
    let marker_index = components
        .iter()
        .rposition(|component| component.eq_ignore_ascii_case(APP_DATA_MARKER))?;
    if components.len() != marker_index + 3 {
        return None;
    }
    let category = AssetCategory::from_directory_name(components[marker_index + 1])?;
    let file_name = components[marker_index + 2];
    if file_name.is_empty() {
        return None;
    }
    Some((category, file_name.to_string()))
}
