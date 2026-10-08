//! Pinned Unicode simple folding; never lowercases or expands source strings.

#[path = "case_fold_data.rs"]
mod data;

/// Version of Unicode's default (non-Turkic) simple case-fold data.
///
/// This is explicit because a platform runtime using a different Unicode
/// version may have different equivalence classes for recently added letters.
pub const UNICODE_SIMPLE_FOLD_VERSION: &str = "17.0.0";

pub(super) fn fold(ch: char) -> char {
    if ch.is_ascii() {
        return ch.to_ascii_lowercase();
    }
    data::SIMPLE_CASE_FOLD
        .binary_search_by_key(&ch, |entry| entry.0)
        .map_or(ch, |index| data::SIMPLE_CASE_FOLD[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scalar_agrees_with_pinned_official_c_and_s_mappings() {
        let mut mappings = std::collections::BTreeMap::new();
        for line in include_str!("unicode/CaseFolding-17.0.0.txt").lines() {
            let data = line.split('#').next().unwrap();
            let fields: Vec<_> = data.split(';').map(str::trim).collect();
            if fields.len() < 3 || !matches!(fields[1], "C" | "S") {
                continue;
            }
            let source = char::from_u32(u32::from_str_radix(fields[0], 16).unwrap()).unwrap();
            let target = char::from_u32(u32::from_str_radix(fields[2], 16).unwrap()).unwrap();
            assert!(mappings.insert(source, target).is_none());
        }
        assert_eq!(mappings.len(), data::SIMPLE_CASE_FOLD.len());
        for ch in (0..=0x10ffff).filter_map(char::from_u32) {
            let expected = mappings.get(&ch).copied().unwrap_or(ch);
            assert_eq!(fold(ch), expected, "U+{:04X}", ch as u32);
            assert_eq!(fold(fold(ch)), fold(ch));
        }
    }
}
