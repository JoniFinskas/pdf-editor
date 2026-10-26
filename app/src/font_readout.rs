use std::collections::BTreeSet;

pub(super) struct Readout {
    pub name: String,
    pub detail: String,
}

impl Readout {
    pub fn new<'a>(fonts: impl Iterator<Item = &'a str>) -> Option<Self> {
        let raw = fonts.collect::<BTreeSet<_>>();
        if raw.is_empty() {
            return None;
        }
        let names = raw
            .iter()
            .map(|name| {
                let name = name
                    .split_once('+')
                    .filter(|(prefix, _)| {
                        prefix.len() == 6 && prefix.bytes().all(|c| c.is_ascii_uppercase())
                    })
                    .map_or(*name, |(_, name)| name);
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    "Unknown".to_owned()
                } else {
                    name
                }
            })
            .collect::<BTreeSet<_>>();
        let name = if names.len() == 1 {
            names.into_iter().next().unwrap()
        } else {
            "Mixed fonts".to_owned()
        };
        let detail = raw
            .into_iter()
            .map(|name| {
                if name.trim().is_empty() {
                    "Font name is missing from the PDF.".to_owned()
                } else {
                    format!("PDF font: {name}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(Self { name, detail })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_names_remain_inspectable_without_looking_like_different_faces() {
        let info =
            Readout::new(["ABCDEF+Arial-BoldMT", "GHIJKL+Arial-BoldMT"].into_iter()).unwrap();
        assert_eq!(info.name, "Arial-BoldMT");
        assert!(info.detail.contains("ABCDEF+Arial-BoldMT"));
        assert!(info.detail.contains("GHIJKL+Arial-BoldMT"));
        for name in ["My+Font", "Abcdef+Font", "ABCDEF7+Font", "åäö字体"] {
            assert_eq!(Readout::new([name].into_iter()).unwrap().name, name);
        }
    }

    #[test]
    fn unknown_and_mixed_fonts_are_not_reported_as_a_single_known_face() {
        assert!(Readout::new([].into_iter()).is_none());
        let info = Readout::new(["", " "].into_iter()).unwrap();
        assert_eq!(info.name, "Unknown");
        assert!(!info.detail.contains("Arial"));
        let info = Readout::new(["Arial", "Arial-BoldMT", "Arial"].into_iter()).unwrap();
        assert_eq!(info.name, "Mixed fonts");
        assert_eq!(info.detail.lines().count(), 2);
        assert_eq!(
            Readout::new(["Arial\nBold\tMT"].into_iter()).unwrap().name,
            "Arial Bold MT"
        );
    }
}
