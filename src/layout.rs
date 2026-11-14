//! Channel labels and the layouts of inputs and outputs.
//!
//! Labels are the channel symbols of AMWA IS-04 v1.3
//! `APIs/schemas/source_audio.json` (from VSF TR-03 Appendix A), written
//! exactly as there, plus `-` for an unused channel.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// Most channels in one layout: the IS-04 undefined-channel symbols run
/// from `U01` to `U64`.
pub const MAX_CHANNELS: usize = 64;

/// One IS-04 v1.3 `source_audio.json` channel symbol, or unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Label {
    L,
    R,
    C,
    Lfe,
    Ls,
    Rs,
    Lss,
    Rss,
    Lrs,
    Rrs,
    Lc,
    Rc,
    Cs,
    Hi,
    Vin,
    M1,
    M2,
    Lt,
    Rt,
    Lst,
    Rst,
    S,
    /// Numbered source channel `NSCnnn`, 0 to 128 per the schema's pattern
    /// `^NSC(0[0-9][0-9]|1[0-1][0-9]|12[0-8])$`.
    Nsc(u8),
    /// Undefined channel `Unn`, 1 to 64.
    U(u8),
    /// Unused, written `-`; not an IS-04 symbol.
    Unused,
}

impl Label {
    /// The symbols of the schema's enum, in its order.
    pub const NAMED: [Label; 22] = [
        Label::L,
        Label::R,
        Label::C,
        Label::Lfe,
        Label::Ls,
        Label::Rs,
        Label::Lss,
        Label::Rss,
        Label::Lrs,
        Label::Rrs,
        Label::Lc,
        Label::Rc,
        Label::Cs,
        Label::Hi,
        Label::Vin,
        Label::M1,
        Label::M2,
        Label::Lt,
        Label::Rt,
        Label::Lst,
        Label::Rst,
        Label::S,
    ];

    fn name(self) -> Option<&'static str> {
        Some(match self {
            Label::L => "L",
            Label::R => "R",
            Label::C => "C",
            Label::Lfe => "LFE",
            Label::Ls => "Ls",
            Label::Rs => "Rs",
            Label::Lss => "Lss",
            Label::Rss => "Rss",
            Label::Lrs => "Lrs",
            Label::Rrs => "Rrs",
            Label::Lc => "Lc",
            Label::Rc => "Rc",
            Label::Cs => "Cs",
            Label::Hi => "HI",
            Label::Vin => "VIN",
            Label::M1 => "M1",
            Label::M2 => "M2",
            Label::Lt => "Lt",
            Label::Rt => "Rt",
            Label::Lst => "Lst",
            Label::Rst => "Rst",
            Label::S => "S",
            Label::Unused => "-",
            Label::Nsc(_) | Label::U(_) => return None,
        })
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Label::Nsc(n) => write!(f, "NSC{n:03}"),
            Label::U(n) => write!(f, "U{n:02}"),
            named => f.write_str(named.name().expect("named label")),
        }
    }
}

/// `digits` as a number when it is exactly `width` ASCII digits.
fn fixed_digits(digits: &str, width: usize) -> Option<u8> {
    if digits.len() == width && digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

impl FromStr for Label {
    type Err = LayoutError;

    /// Parses a symbol as IS-04 writes it; case matters.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let numbered = if let Some(d) = s.strip_prefix("NSC") {
            fixed_digits(d, 3).filter(|n| *n <= 128).map(Label::Nsc)
        } else if let Some(d) = s.strip_prefix('U') {
            fixed_digits(d, 2)
                .filter(|n| (1..=64).contains(n))
                .map(Label::U)
        } else {
            None
        };
        numbered
            .or_else(|| {
                Label::NAMED
                    .into_iter()
                    .chain([Label::Unused])
                    .find(|label| label.name() == Some(s))
            })
            .ok_or_else(|| LayoutError::UnknownLabel(s.to_owned()))
    }
}

/// Why a layout was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    Empty,
    TooManyChannels(usize),
    UnknownLabel(String),
    /// A label other than `-` appears more than once.
    Duplicate(Label),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Empty => f.write_str("layout has no channels"),
            LayoutError::TooManyChannels(n) => {
                write!(
                    f,
                    "layout has {n} channels, at most {MAX_CHANNELS} are allowed"
                )
            }
            LayoutError::UnknownLabel(s) => write!(
                f,
                "unknown channel label {s:?}, expected an IS-04 v1.3 channel symbol \
                 ({}, NSC000 to NSC128, U01 to U64) or -",
                Label::NAMED
                    .map(|l| l.name().expect("named label"))
                    .join(", ")
            ),
            LayoutError::Duplicate(label) => {
                write!(f, "channel label {label} appears more than once")
            }
        }
    }
}

impl std::error::Error for LayoutError {}

/// The labels of a flow's channels in channel order: 1 to
/// [`MAX_CHANNELS`] channels, each label other than `-` at most once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout(Vec<Label>);

impl Layout {
    pub fn new(labels: Vec<Label>) -> Result<Self, LayoutError> {
        if labels.is_empty() {
            return Err(LayoutError::Empty);
        }
        if labels.len() > MAX_CHANNELS {
            return Err(LayoutError::TooManyChannels(labels.len()));
        }
        for (i, label) in labels.iter().enumerate() {
            if *label != Label::Unused && labels[..i].contains(label) {
                return Err(LayoutError::Duplicate(*label));
            }
        }
        Ok(Layout(labels))
    }

    /// Parses a layout from labels as written, such as `["L", "R", "-"]`.
    pub fn parse<S: AsRef<str>>(names: &[S]) -> Result<Self, LayoutError> {
        let labels = names
            .iter()
            .map(|name| name.as_ref().parse())
            .collect::<Result<Vec<Label>, _>>()?;
        Layout::new(labels)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false: a layout has at least one channel.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn labels(&self) -> &[Label] {
        &self.0
    }

    /// The channel carrying `label`, which must not be [`Label::Unused`].
    pub fn position(&self, label: Label) -> Option<usize> {
        debug_assert_ne!(label, Label::Unused);
        self.0.iter().position(|l| *l == label)
    }
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = self.0.iter().map(Label::to_string).collect();
        write!(f, "[{}]", names.join(", "))
    }
}

impl<'de> Deserialize<'de> for Layout {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(deserializer)?;
        Layout::parse(&names).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_are_the_is04_v1_3_enum() {
        // source_audio.json, "symbol", enum in schema order.
        let schema = [
            "L", "R", "C", "LFE", "Ls", "Rs", "Lss", "Rss", "Lrs", "Rrs", "Lc", "Rc", "Cs", "HI",
            "VIN", "M1", "M2", "Lt", "Rt", "Lst", "Rst", "S",
        ];
        let written: Vec<String> = Label::NAMED.iter().map(Label::to_string).collect();
        assert_eq!(written, schema);
        for name in schema {
            assert_eq!(name.parse::<Label>().unwrap().to_string(), name);
        }
    }

    #[test]
    fn numbered_symbols_follow_the_schema_patterns() {
        for (ok, label) in [
            ("NSC000", Label::Nsc(0)),
            ("NSC001", Label::Nsc(1)),
            ("NSC128", Label::Nsc(128)),
            ("U01", Label::U(1)),
            ("U64", Label::U(64)),
        ] {
            assert_eq!(ok.parse(), Ok(label));
            assert_eq!(label.to_string(), ok);
        }
        for bad in [
            "NSC129", "NSC1", "NSC0001", "NSC+01", "nsc001", "U00", "U65", "U1", "U001", "U+1",
        ] {
            assert_eq!(
                bad.parse::<Label>(),
                Err(LayoutError::UnknownLabel(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn non_is04_names_are_refused() {
        for bad in ["Lb", "Rb", "lfe", "LS", "Hi", "X", ""] {
            assert_eq!(
                bad.parse::<Label>(),
                Err(LayoutError::UnknownLabel(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn layouts_parse_and_print_as_written() {
        let layout = Layout::parse(&[
            "L", "R", "C", "LFE", "Lss", "Rss", "Lrs", "Rrs", "-", "NSC007", "U12",
        ])
        .unwrap();
        assert_eq!(layout.position(Label::Lfe), Some(3));
        assert_eq!(layout.position(Label::Nsc(7)), Some(9));
        assert_eq!(layout.position(Label::Ls), None);
        assert_eq!(
            layout.to_string(),
            "[L, R, C, LFE, Lss, Rss, Lrs, Rrs, -, NSC007, U12]"
        );
    }

    #[test]
    fn unused_channels_may_repeat_and_labels_may_not() {
        assert!(Layout::parse(&["-", "-", "-"]).is_ok());
        assert_eq!(
            Layout::parse(&["L", "R", "L"]),
            Err(LayoutError::Duplicate(Label::L))
        );
        assert_eq!(
            Layout::parse(&["U03", "U03"]),
            Err(LayoutError::Duplicate(Label::U(3)))
        );
    }

    #[test]
    fn channel_count_is_bounded() {
        let empty: [&str; 0] = [];
        assert_eq!(Layout::parse(&empty), Err(LayoutError::Empty));
        assert!(Layout::parse(&["-"; MAX_CHANNELS]).is_ok());
        assert_eq!(
            Layout::parse(&["-"; MAX_CHANNELS + 1]),
            Err(LayoutError::TooManyChannels(MAX_CHANNELS + 1))
        );
    }

    #[test]
    fn deserializing_validates() {
        let ok: Layout = serde_json::from_str(r#"["L","R"]"#).unwrap();
        assert_eq!(ok.labels(), [Label::L, Label::R]);
        let err = serde_json::from_str::<Layout>(r#"["L","Lb"]"#).unwrap_err();
        assert!(
            err.to_string().contains("unknown channel label \"Lb\""),
            "{err}"
        );
        let err = serde_json::from_str::<Layout>(r#"["R","R"]"#).unwrap_err();
        assert!(
            err.to_string().contains("R appears more than once"),
            "{err}"
        );
    }
}
