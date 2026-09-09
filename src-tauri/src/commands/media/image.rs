use crate::{
    commands::dialog::parented_file_dialog,
    errors::{CmdResult, CommandError},
    state::assets::image_asset::{import_image_file, SUPPORTED_IMAGE_EXTENSIONS},
};
#[cfg(test)]
use dmnote_editor_engine::web_resources::image::SVG_PARSE_BUDGET;
use dmnote_editor_engine::web_resources::image::{
    image_reader_has_supported_content, normalize_image_extension,
};
use serde::Serialize;
use std::{fs::File, io, path::Path};
use tauri::{Manager, WebviewWindow};
const INVALID_IMAGE_CONTENT: &str = "invalid-image-content";
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageLoadResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
}

/// 로컬 이미지 파일을 선택해서 앱 데이터 디렉토리로 복사한 뒤 경로를 반환합니다.
/// 저장소에는 base64 대신 파일 경로만 저장해 직렬화/역직렬화 비용을 줄입니다.
#[tauri::command]
pub async fn image_load(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> CmdResult<ImageLoadResponse> {
    let picked = parented_file_dialog(&window, "Images", SUPPORTED_IMAGE_EXTENSIONS)
        .pick_file()
        .await;

    let Some(file) = picked else {
        return Ok(ImageLoadResponse {
            success: false,
            error: None,
            error_code: None,
            image_path: None,
        });
    };

    let source_path = file.path().to_path_buf();
    let extension =
        normalize_image_extension(source_path.extension().and_then(|value| value.to_str()));
    let images_dir = app.path().app_data_dir()?.join("images");
    let imported = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<_> {
        if !image_file_has_supported_content(&source_path)? {
            return Ok(None);
        }
        import_image_file(&source_path, &images_dir, &extension).map(Some)
    })
    .await
    .map_err(|error| CommandError::msg(format!("image import task failed: {error}")))??;

    let Some(imported) = imported else {
        return Ok(ImageLoadResponse {
            success: false,
            error: Some("Selected file is not valid image content".to_string()),
            error_code: Some(INVALID_IMAGE_CONTENT.to_string()),
            image_path: None,
        });
    };

    Ok(ImageLoadResponse {
        success: true,
        error: None,
        error_code: None,
        image_path: Some(imported.path.to_string_lossy().to_string()),
    })
}

fn image_file_has_supported_content(path: &Path) -> io::Result<bool> {
    let file = File::open(path)?;
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    image_reader_has_supported_content(file, size)
}
#[cfg(test)]
mod tests {
    use super::{image_file_has_supported_content, normalize_image_extension};
    use super::{ImageLoadResponse, INVALID_IMAGE_CONTENT, SVG_PARSE_BUDGET};
    use std::fs;
    use uuid::Uuid;

    struct Fixture {
        directory: std::path::PathBuf,
        path: std::path::PathBuf,
    }

    impl Fixture {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let directory = std::env::temp_dir().join(format!("dmnote-image-{}", Uuid::new_v4()));
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join(name);
            fs::write(&path, bytes).unwrap();
            Self { directory, path }
        }

        fn accepted(&self) -> bool {
            image_file_has_supported_content(&self.path).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn gif_extension_is_kept_without_format_conversion() {
        assert_eq!(normalize_image_extension(Some("GIF")), "gif");
    }

    #[test]
    fn unknown_extension_uses_existing_png_fallback() {
        assert_eq!(normalize_image_extension(Some("unknown")), "png");
        assert_eq!(normalize_image_extension(None), "png");
    }

    #[test]
    fn binary_image_signatures_are_accepted() {
        assert!(Fixture::new("a.png", b"\x89PNG\r\n\x1a\n").accepted());
        assert!(Fixture::new("a.gif", b"GIF89a").accepted());
    }

    #[test]
    fn text_file_with_png_extension_is_rejected() {
        assert!(!Fixture::new("not-an-image.png", b"plain text").accepted());
    }

    #[test]
    fn long_non_markup_file_is_rejected() {
        let bytes = "plain text ".repeat(2000);
        assert!(!Fixture::new("not-an-image.png", bytes.as_bytes()).accepted());
    }

    #[test]
    fn plain_svg_root_is_accepted() {
        let svg =
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="64" height="64"/></svg>"#;
        assert!(Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn svg_after_declaration_comment_and_doctype_is_accepted() {
        let svg = br#"<?xml version="1.0" encoding="UTF-8"?>
<!-- icon -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        assert!(Fixture::new("a.svg", svg).accepted());
    }

    // XML prolog는 스펙상 길이 상한이 없다 - 고정 prefix로 판정하던 시절의 오탐 회귀
    #[test]
    fn svg_behind_a_comment_longer_than_the_signature_prefix_is_accepted() {
        let mut svg = format!("<!--{}-->", "c".repeat(9000)).into_bytes();
        svg.extend_from_slice(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn svg_behind_whitespace_longer_than_the_signature_prefix_is_accepted() {
        let mut svg = " ".repeat(9000).into_bytes();
        svg.extend_from_slice(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn svg_with_utf8_bom_is_accepted() {
        let mut svg = b"\xef\xbb\xbf".to_vec();
        svg.extend_from_slice(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn namespace_prefixed_svg_root_is_accepted() {
        let svg = br#"<svg:svg xmlns:svg="http://www.w3.org/2000/svg"/>"#;
        assert!(Fixture::new("a.svg", svg).accepted());
    }

    // local name만 보면 통과하지만 SVG namespace가 아니면 브라우저도 그리지 못한다
    #[test]
    fn svg_local_name_in_a_foreign_namespace_is_rejected() {
        let svg = br#"<foo:svg xmlns:foo="http://example.com/not-svg"/>"#;
        assert!(!Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn svg_without_namespace_is_rejected() {
        assert!(!Fixture::new("a.svg", br#"<svg><rect/></svg>"#).accepted());
    }

    #[test]
    fn second_root_element_is_rejected() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"/><svg xmlns="http://www.w3.org/2000/svg"/>"#;
        assert!(!Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn trailing_text_after_the_root_is_rejected() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>trailing"#;
        assert!(!Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn trailing_whitespace_and_comment_after_the_root_are_allowed() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>
<!-- done -->
"#;
        assert!(Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn utf16_with_an_odd_trailing_byte_is_rejected() {
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.push(0x20);
        assert!(!Fixture::new("a.svg", &bytes).accepted());
    }

    #[test]
    fn utf16_with_a_lone_surrogate_is_rejected() {
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        // 짝 없는 high surrogate
        bytes.extend_from_slice(&0xD800_u16.to_le_bytes());
        assert!(!Fixture::new("a.svg", &bytes).accepted());
    }

    #[test]
    fn truncated_document_after_the_root_is_rejected() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><g>"#;
        assert!(!Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn utf16le_svg_is_accepted() {
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#;
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(Fixture::new("a.svg", &bytes).accepted());
    }

    #[test]
    fn utf16be_svg_is_accepted() {
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#;
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        assert!(Fixture::new("a.svg", &bytes).accepted());
    }

    #[test]
    fn multibyte_content_does_not_break_detection() {
        let mut svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><text>"#.to_vec();
        svg.extend_from_slice("가".repeat(4096).as_bytes());
        svg.extend_from_slice(b"</text></svg>");
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn markup_whose_root_is_not_svg_is_rejected() {
        assert!(!Fixture::new("a.svg", br#"<html><svg></svg></html>"#).accepted());
    }

    // 예산을 넘겨도 루트를 이미 확인했으면 통과시킨다 - 큰 SVG를 거절하지 않는 계약
    #[test]
    fn document_larger_than_the_parse_budget_is_accepted_after_the_root() {
        let mut svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><text>"#.to_vec();
        svg.resize(SVG_PARSE_BUDGET as usize + 4096, b'x');
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    // 지도나 트레이스 내보내기는 path 하나가 예산을 넘는다.
    // 경계가 속성값 한가운데면 파서가 오류를 내는데 그건 문서가 잘못된 것이 아니다
    #[test]
    fn budget_boundary_inside_an_attribute_still_accepts() {
        let mut svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><path d=""#.to_vec();
        svg.resize(SVG_PARSE_BUDGET as usize + 4096, b'1');
        svg.extend_from_slice(br#""/></svg>"#);
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn budget_boundary_inside_a_comment_still_accepts() {
        let mut svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><!--"#.to_vec();
        svg.resize(SVG_PARSE_BUDGET as usize + 4096, b'c');
        svg.extend_from_slice(br#"--></svg>"#);
        assert!(Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn utf16_document_larger_than_the_budget_is_accepted_after_the_root() {
        let head = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>"#;
        let mut bytes = vec![0xFF, 0xFE];
        for unit in head.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        while (bytes.len() as u64) < SVG_PARSE_BUDGET + 4096 {
            bytes.extend_from_slice(&(b'x' as u16).to_le_bytes());
        }
        assert!(Fixture::new("a.svg", &bytes).accepted());
    }

    // 잘리지 않은 문서의 파싱 오류는 그대로 거절이어야 한다
    #[test]
    fn malformed_small_document_is_still_rejected() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><path d="unclosed"#;
        assert!(!Fixture::new("a.svg", svg).accepted());
    }

    #[test]
    fn prolog_beyond_the_parse_budget_is_rejected() {
        let mut svg = format!("<!--{}-->", "c".repeat(SVG_PARSE_BUDGET as usize)).into_bytes();
        svg.extend_from_slice(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#);
        assert!(!Fixture::new("a.svg", &svg).accepted());
    }

    #[test]
    fn response_serializes_error_code_and_keeps_cancellation_quiet() {
        let invalid = serde_json::to_value(ImageLoadResponse {
            success: false,
            error: Some("invalid image".to_string()),
            error_code: Some(INVALID_IMAGE_CONTENT.to_string()),
            image_path: None,
        })
        .unwrap();
        assert_eq!(invalid["errorCode"], INVALID_IMAGE_CONTENT);

        let cancelled = serde_json::to_value(ImageLoadResponse {
            success: false,
            error: None,
            error_code: None,
            image_path: None,
        })
        .unwrap();
        assert!(cancelled.get("errorCode").is_none());
        assert!(cancelled.get("error").is_none());
    }
}
