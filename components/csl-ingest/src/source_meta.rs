//! ITA → short-code mapping for the CSL `source` field.
//!
//! The real ITA endpoint emits long human strings like
//! "Specially Designated Nationals (SDN) - Treasury Department". The UI
//! and storage want short codes ("SDN") plus an outbound link to the
//! authoritative agency reference page. This map is the single source of
//! truth.
//!
//! Per PLAN.md §4 this should ship as data, not code. M5 moves it to
//! `interfaces/ocelaudit/source-meta.json` so ops can edit without a
//! rebuild; the current Rust constant is fine for M3.

#[derive(Debug, Clone, Copy)]
pub struct SourceMeta {
    pub code: &'static str,
    pub long_name: &'static str,
    pub agency_url: &'static str,
}

pub const ALL: &[SourceMeta] = &[
    SourceMeta {
        code: "SDN",
        long_name: "Specially Designated Nationals",
        agency_url: "https://ofac.treasury.gov/specially-designated-nationals-and-blocked-persons-list-sdn-human-readable-lists",
    },
    SourceMeta {
        code: "EL",
        long_name: "Entity List",
        agency_url: "https://www.bis.doc.gov/index.php/policy-guidance/lists-of-parties-of-concern/entity-list",
    },
    SourceMeta {
        code: "UVL",
        long_name: "Unverified List",
        agency_url: "https://www.bis.doc.gov/index.php/policy-guidance/lists-of-parties-of-concern/unverified-list",
    },
    SourceMeta {
        code: "DPL",
        long_name: "Denied Persons List",
        agency_url: "https://www.bis.doc.gov/index.php/policy-guidance/lists-of-parties-of-concern/denied-persons-list",
    },
    SourceMeta {
        code: "ITAR-DPL",
        long_name: "ITAR Debarred",
        agency_url: "https://www.pmddtc.state.gov/ddtc_public?id=ddtc_kb_article_page&sys_id=c22d1833dbb8d300d0a370131f9619f0",
    },
    SourceMeta {
        code: "SSI",
        long_name: "Sectoral Sanctions Identifications",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/sectoral-sanctions-identifications-ssi-list",
    },
    SourceMeta {
        code: "FSE",
        long_name: "Foreign Sanctions Evaders",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/foreign-sanctions-evaders-fse-list",
    },
    SourceMeta {
        code: "PLC",
        long_name: "Palestinian Legislative Council List",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/palestinian-legislative-council-list-plc",
    },
    SourceMeta {
        code: "NS-MBS",
        long_name: "Non-SDN Menu-Based Sanctions List",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/non-sdn-menu-based-sanctions-list-ns-mbs-list",
    },
    SourceMeta {
        code: "NS-ISA",
        long_name: "Non-SDN Iran Sanctions Act List",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/non-sdn-iran-sanctions-act-list-ns-isa-list",
    },
    SourceMeta {
        code: "CAPTA",
        long_name: "Correspondent Account Or Payable-Through Account Sanctions",
        agency_url: "https://ofac.treasury.gov/consolidated-sanctions-list-non-sdn-lists/correspondent-account-or-payable-through-account-sanctions-capta-list",
    },
];

/// Match a long ITA source string to its short code. Returns `None` if
/// no entry contains the long_name as a substring; the caller should
/// then pass the source string through verbatim.
pub fn short_code(long: &str) -> Option<&'static str> {
    let lower = long.to_ascii_lowercase();
    for meta in ALL {
        if lower.contains(&meta.long_name.to_ascii_lowercase()) {
            return Some(meta.code);
        }
    }
    None
}

pub fn meta_for_code(code: &str) -> Option<&'static SourceMeta> {
    ALL.iter().find(|m| m.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_official_strings() {
        assert_eq!(short_code("Specially Designated Nationals (SDN) - Treasury Department"), Some("SDN"));
        assert_eq!(short_code("Entity List (EL) - Bureau of Industry and Security"), Some("EL"));
        assert_eq!(short_code("Sectoral Sanctions Identifications List - Treasury Department"), Some("SSI"));
    }

    #[test]
    fn returns_none_for_unknown() {
        assert_eq!(short_code("Made-Up List - Nowhere"), None);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(short_code("specially designated nationals"), Some("SDN"));
    }

    #[test]
    fn meta_lookup_round_trips() {
        let m = meta_for_code("SDN").unwrap();
        assert!(m.agency_url.contains("ofac.treasury.gov"));
    }
}
