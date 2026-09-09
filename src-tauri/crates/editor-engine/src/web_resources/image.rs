use quick_xml::{
    events::Event,
    name::{Namespace, ResolveResult},
    NsReader,
};
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek};
const IMAGE_PREFIX_LENGTH: u64 = 8192;
pub const SVG_PARSE_BUDGET: u64 = 16 << 20;
const SVG_NAMESPACE: Namespace<'static> = Namespace(b"http://www.w3.org/2000/svg");
pub fn image_reader_has_supported_content<R: Read + Seek>(
    mut reader: R,
    size: u64,
) -> io::Result<bool> {
    let mut prefix = Vec::with_capacity(IMAGE_PREFIX_LENGTH as usize);
    reader
        .by_ref()
        .take(IMAGE_PREFIX_LENGTH)
        .read_to_end(&mut prefix)?;
    if has_image_signature(&prefix) {
        return Ok(true);
    }
    reader.rewind()?;
    Ok(is_svg_document(reader, size))
}
pub fn image_bytes_have_supported_content(bytes: &[u8]) -> bool {
    image_reader_has_supported_content(Cursor::new(bytes), bytes.len() as u64).unwrap_or(false)
}
fn has_image_signature(bytes: &[u8]) -> bool {
    infer::get(bytes).is_some_and(|kind| kind.matcher_type() == infer::MatcherType::Image)
}

/// SVG에는 매직넘버가 없다. RFC 7303 §9.1과 SVG 미디어 타입 등록서 모두 magic number를
/// 비워 두고 있고, XML prolog는 길이 상한이 없어 고정 prefix 스캔으로는 판정할 수 없다.
/// 그래서 실제로 파싱해 루트가 SVG namespace의 `svg`인지 확인한다
fn is_svg_document<R: Read + Seek>(mut file: R, size: u64) -> bool {
    let mut bom = [0u8; 2];
    let Ok(read) = read_prefix(&mut file, &mut bom) else {
        return false;
    };
    if file.rewind().is_err() {
        return false;
    }
    if read == 2 && (bom == [0xFF, 0xFE] || bom == [0xFE, 0xFF]) {
        let Some((utf8, truncated)) = transcode_utf16(file, bom[0] == 0xFF) else {
            return false;
        };
        return scan_svg(NsReader::from_reader(Cursor::new(utf8)), truncated);
    }
    scan_svg(
        NsReader::from_reader(BufReader::new(file.take(SVG_PARSE_BUDGET))),
        size > SVG_PARSE_BUDGET,
    )
}

fn read_prefix(file: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            count => filled += count,
        }
    }
    Ok(filled)
}

fn transcode_utf16(file: impl Read, little_endian: bool) -> Option<(Vec<u8>, bool)> {
    let mut raw = Vec::new();
    file.take(SVG_PARSE_BUDGET).read_to_end(&mut raw).ok()?;
    // 예산에 걸려 잘린 경우에만 관대하게 본다. 온전한 파일은 엄격히 판정한다
    let truncated = raw.len() as u64 >= SVG_PARSE_BUDGET;
    let body = raw.get(2..)?;
    if !truncated && body.len() % 2 != 0 {
        return None;
    }

    let units = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let bytes = [pair[0], pair[1]];
            if little_endian {
                u16::from_le_bytes(bytes)
            } else {
                u16::from_be_bytes(bytes)
            }
        })
        .collect::<Vec<_>>();

    let text = if truncated {
        char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
    } else {
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok()?
    };
    Some((text.into_bytes(), truncated))
}

fn scan_svg<R: BufRead>(mut reader: NsReader<R>, truncated: bool) -> bool {
    let mut buffer = Vec::new();
    let mut root_seen = false;
    let mut root_closed = false;
    let mut depth = 0_usize;

    loop {
        buffer.clear();
        match reader.read_resolved_event_into(&mut buffer) {
            Ok((namespace, Event::Start(tag))) => {
                // 루트가 닫힌 뒤의 요소는 두 번째 루트다
                if root_closed {
                    return false;
                }
                if !root_seen {
                    if !is_svg_root(namespace, tag.local_name().as_ref()) {
                        return false;
                    }
                    root_seen = true;
                }
                depth += 1;
            }
            Ok((namespace, Event::Empty(tag))) => {
                if root_closed {
                    return false;
                }
                if !root_seen {
                    if !is_svg_root(namespace, tag.local_name().as_ref()) {
                        return false;
                    }
                    // 빈 루트는 그 자리에서 닫힌다
                    root_seen = true;
                    root_closed = true;
                }
            }
            Ok((_, Event::End(_))) => {
                depth = depth.saturating_sub(1);
                if root_seen && depth == 0 {
                    root_closed = true;
                }
            }
            // 루트 바깥에 올 수 있는 것은 공백뿐이다 (XML 1.0 Misc)
            Ok((_, Event::Text(text))) => {
                if (!root_seen || root_closed) && !text.iter().all(u8::is_ascii_whitespace) {
                    return false;
                }
            }
            Ok((_, Event::CData(_))) => {
                if !root_seen || root_closed {
                    return false;
                }
            }
            // 예산에 걸려 잘린 문서는 뒷부분을 판정하지 않는다.
            // 루트를 이미 확인했으면 통과시킨다 - 큰 SVG를 거절하지 않기 위한 계약
            Ok((_, Event::Eof)) => return if truncated { root_seen } else { root_closed },
            // prolog(선언, PI, 주석, DOCTYPE)는 그대로 지나간다
            Ok(_) => {}
            // 예산 경계가 태그나 주석 한가운데를 자르면 파싱 오류로 나온다.
            // 잘린 문서의 오류는 문서가 잘못됐다는 근거가 아니다
            Err(_) => return truncated && root_seen,
        }
    }
}

fn is_svg_root(namespace: ResolveResult, local_name: &[u8]) -> bool {
    matches!(namespace, ResolveResult::Bound(value) if value == SVG_NAMESPACE)
        && local_name == b"svg"
}

pub fn normalize_image_extension(extension: Option<&str>) -> String {
    match extension
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" => "jpg".to_string(),
        "jpeg" => "jpeg".to_string(),
        "webp" => "webp".to_string(),
        "gif" => "gif".to_string(),
        "bmp" => "bmp".to_string(),
        "svg" => "svg".to_string(),
        "ico" => "ico".to_string(),
        "avif" => "avif".to_string(),
        "png" => "png".to_string(),
        _ => "png".to_string(),
    }
}
