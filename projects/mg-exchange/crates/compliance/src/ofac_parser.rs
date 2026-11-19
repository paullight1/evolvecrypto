//! OFAC SDN XML/text parser.
//!
//! Parses the OFAC SDN (Specially Designated Nationals) list to extract:
//! - Names (lastName, firstName) for sanctions screening
//! - Digital currency addresses for blockchain screening
//!
//! The parser is deliberately simple — it uses basic string scanning rather
//! than a full XML parser to avoid adding heavy dependencies. The OFAC XML
//! format is stable and well-documented.
//!
//! # Source
//! - XML: https://sanctionslist.ofac.treas.gov/Home/SdnList
//! - Reference: OFAC SDN XML Schema v2.0

/// Parsed OFAC SDN entry.
#[derive(Debug, Clone)]
pub struct SdnEntry {
    /// Full name (lastName + firstName).
    pub name: String,
    /// Digital currency addresses associated with this entry.
    pub addresses: Vec<String>,
}

/// Parse OFAC SDN XML content and extract names + crypto addresses.
///
/// This is a lightweight line-by-line parser that extracts:
/// - `<lastName>...</lastName>` and `<firstName>...</firstName>` → combined name
/// - `<digitalCurrencyAddress>` ID values → crypto addresses
///
/// Returns a list of entries. On malformed input, extracts what it can
/// (never panics, never returns empty on valid input).
pub fn parse_sdn_xml(xml: &str) -> Vec<SdnEntry> {
    let mut entries = Vec::new();
    let mut current_last_name = String::new();
    let mut current_first_name = String::new();
    let mut current_addresses: Vec<String> = Vec::new();
    let mut in_sdn_entry = false;

    for line in xml.lines() {
        let trimmed = line.trim();

        if trimmed.contains("<sdnEntry>") {
            in_sdn_entry = true;
            current_last_name.clear();
            current_first_name.clear();
            current_addresses.clear();
        }

        if in_sdn_entry {
            if let Some(val) = extract_tag_value(trimmed, "lastName") {
                current_last_name = val;
            }
            if let Some(val) = extract_tag_value(trimmed, "firstName") {
                current_first_name = val;
            }
            // Digital currency addresses appear as:
            // <id_type>Digital Currency Address - XBT</id_type>
            // <id_number>1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa</id_number>
            if trimmed.contains("Digital Currency Address") {
                // The address is in the next <id_number> tag — we'll catch it.
            }
            if let Some(val) = extract_tag_value(trimmed, "id_number") {
                // Heuristic: if it looks like a crypto address (hex or base58, 20+ chars)
                let clean = val.trim().to_string();
                if clean.len() >= 20 && !clean.contains(' ') {
                    current_addresses.push(clean);
                }
            }
        }

        if trimmed.contains("</sdnEntry>") && in_sdn_entry {
            in_sdn_entry = false;
            let name = if current_first_name.is_empty() {
                current_last_name.clone()
            } else {
                format!("{current_first_name} {current_last_name}")
            };
            if !name.trim().is_empty() {
                entries.push(SdnEntry {
                    name,
                    addresses: current_addresses.clone(),
                });
            }
        }
    }

    entries
}

/// Extract the text content of a simple XML tag: `<tag>value</tag>` → `value`.
fn extract_tag_value(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = line.find(&open)?;
    let end = line.find(&close)?;

    if start + open.len() <= end {
        Some(line[start + open.len()..end].to_string())
    } else {
        None
    }
}

/// Convert parsed SDN entries into name and address lists suitable for
/// `SanctionsScreener::bulk_load()`.
pub fn entries_to_lists(entries: &[SdnEntry]) -> (Vec<String>, Vec<String>) {
    let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let addresses: Vec<String> = entries
        .iter()
        .flat_map(|e| e.addresses.iter().cloned())
        .collect();
    (names, addresses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_sdn_entry() {
        let xml = r#"
        <sdnList>
          <sdnEntry>
            <firstName>JOHN</firstName>
            <lastName>DOE</lastName>
          </sdnEntry>
        </sdnList>
        "#;

        let entries = parse_sdn_xml(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "JOHN DOE");
    }

    #[test]
    fn parse_entry_with_crypto_address() {
        let xml = r#"
        <sdnEntry>
            <lastName>EVIL CORP</lastName>
            <id_type>Digital Currency Address - XBT</id_type>
            <id_number>1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa</id_number>
        </sdnEntry>
        "#;

        let entries = parse_sdn_xml(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].addresses.len(), 1);
        assert_eq!(entries[0].addresses[0], "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa");
    }

    #[test]
    fn parse_multiple_entries() {
        let xml = r#"
        <sdnEntry>
            <lastName>PERSON ONE</lastName>
        </sdnEntry>
        <sdnEntry>
            <lastName>PERSON TWO</lastName>
        </sdnEntry>
        <sdnEntry>
            <firstName>JANE</firstName>
            <lastName>THREE</lastName>
        </sdnEntry>
        "#;

        let entries = parse_sdn_xml(xml);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].name, "JANE THREE");
    }

    #[test]
    fn entries_to_lists_splits_correctly() {
        let entries = vec![
            SdnEntry { name: "Alice".into(), addresses: vec!["0xabc".into()] },
            SdnEntry { name: "Bob".into(), addresses: vec!["0xdef".into(), "0x123".into()] },
        ];

        let (names, addrs) = entries_to_lists(&entries);
        assert_eq!(names.len(), 2);
        assert_eq!(addrs.len(), 3);
    }

    #[test]
    fn empty_xml_returns_empty() {
        assert!(parse_sdn_xml("").is_empty());
        assert!(parse_sdn_xml("<root></root>").is_empty());
    }

    #[test]
    fn short_id_numbers_ignored() {
        // ID numbers shorter than 20 chars are not crypto addresses.
        let xml = r#"
        <sdnEntry>
            <lastName>TEST</lastName>
            <id_number>12345</id_number>
        </sdnEntry>
        "#;

        let entries = parse_sdn_xml(xml);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].addresses.is_empty());
    }
}
