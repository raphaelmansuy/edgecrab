use std::ops::Range;

use lsp_types::Position;

pub struct PositionEncoder;

impl PositionEncoder {
    pub fn to_byte_offset(text: &str, pos: Position) -> Option<usize> {
        let mut lines = text.split('\n');
        let mut offset = 0usize;

        for _ in 0..pos.line {
            let line = lines.next()?;
            offset = offset.checked_add(line.len() + 1)?;
        }

        let line = lines.next().unwrap_or("");
        let mut remaining = pos.character as usize;
        let mut byte_offset = 0usize;
        for (idx, ch) in line.char_indices() {
            if remaining == 0 {
                return Some(offset + idx);
            }
            remaining = remaining.saturating_sub(ch.len_utf16());
            byte_offset = idx + ch.len_utf8();
        }

        if remaining == 0 {
            Some(offset + byte_offset)
        } else {
            None
        }
    }

    pub fn to_position(text: &str, byte_offset: usize) -> Option<Position> {
        if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
            return None;
        }

        let prefix = &text[..byte_offset];
        let line = prefix.as_bytes().iter().filter(|b| **b == b'\n').count() as u32;
        let line_start = prefix.rfind('\n').map_or(0, |idx| idx + 1);
        let line_slice = &text[line_start..byte_offset];
        let character = line_slice.chars().map(char::len_utf16).sum::<usize>() as u32;
        Some(Position { line, character })
    }

    pub fn to_byte_range(text: &str, range: lsp_types::Range) -> Option<Range<usize>> {
        let start = Self::to_byte_offset(text, range.start)?;
        let end = Self::to_byte_offset(text, range.end)?;
        Some(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_roundtrip_handles_unicode() {
        let text = "fn main() {\n  let crab = \"🦀\";\n}\n";
        let offset = text.find('🦀').expect("emoji offset");
        let pos = PositionEncoder::to_position(text, offset).expect("to position");
        let back = PositionEncoder::to_byte_offset(text, pos).expect("to offset");
        assert_eq!(offset, back);
    }
}
