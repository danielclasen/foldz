//! Channel labels and the layouts of inputs and outputs.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// Most channels in one layout. Unlabelled channels take the NMOS channel
/// symbols `U01` to `U64`.
pub const MAX_CHANNELS: usize = 64;

/// What one channel of a layout carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Label {
    /// Left.
    L,
    /// Right.
    R,
    /// Centre.
    C,
    /// Low-frequency effects.
    Lfe,
    /// Left surround; the side surround of a 3/4 layout.
    Ls,
    /// Right surround; the side surround of a 3/4 layout.
    Rs,
    /// Left side surround.
    Lss,
    /// Right side surround.
    Rss,
    /// Left rear surround.
    Lrs,
    /// Right rear surround.
    Rrs,
    /// Left back surround of a 3/4 layout.
    Lb,
    /// Right back surround of a 3/4 layout.
    Rb,
    /// Single surround of a 2/1 or 3/1 layout.
    S,
    /// Left total of a matrix-encoded Lt/Rt pair.
    Lt,
    /// Right total of a matrix-encoded Lt/Rt pair.
    Rt,
    /// Unused or unlabelled, written `-`.
    Unused,
}

impl Label {
    /// Every label, in declaration order.
    pub const ALL: [Label; 16] = [
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
        Label::Lb,
        Label::Rb,
        Label::S,
        Label::Lt,
        Label::Rt,
        Label::Unused,
    ];

    /// The label as written in a layout.
    pub fn as_str(self) -> &'static str {
        match self {
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
            Label::Lb => "Lb",
            Label::Rb => "Rb",
            Label::S => "S",
            Label::Lt => "Lt",
            Label::Rt => "Rt",
            Label::Unused => "-",
        }
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Label {
    type Err = LayoutError;

    /// Parses a label as written in a layout; case matters.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Label::ALL
            .into_iter()
            .find(|label| label.as_str() == s)
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
                "unknown channel label {s:?}, expected one of {}",
                Label::ALL.map(Label::as_str).join(", ")
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
        let names: Vec<&str> = self.0.iter().map(|l| l.as_str()).collect();
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
    fn labels_round_trip_through_their_written_form() {
        for label in Label::ALL {
            assert_eq!(label.as_str().parse::<Label>(), Ok(label));
        }
    }

    #[test]
    fn written_forms_are_the_ones_configs_use() {
        let layout =
            Layout::parse(&["L", "R", "C", "LFE", "Ls", "Rs", "Lss", "Rss", "-", "-"]).unwrap();
        assert_eq!(
            layout.labels(),
            [
                Label::L,
                Label::R,
                Label::C,
                Label::Lfe,
                Label::Ls,
                Label::Rs,
                Label::Lss,
                Label::Rss,
                Label::Unused,
                Label::Unused
            ]
        );
        assert_eq!(layout.position(Label::Lfe), Some(3));
        assert_eq!(layout.position(Label::Lb), None);
        assert_eq!(layout.to_string(), "[L, R, C, LFE, Ls, Rs, Lss, Rss, -, -]");
    }

    #[test]
    fn labels_are_case_sensitive() {
        assert_eq!(
            "lfe".parse::<Label>(),
            Err(LayoutError::UnknownLabel("lfe".into()))
        );
        assert_eq!(
            "LS".parse::<Label>(),
            Err(LayoutError::UnknownLabel("LS".into()))
        );
    }

    #[test]
    fn unused_channels_may_repeat_and_labels_may_not() {
        assert!(Layout::parse(&["-", "-", "-"]).is_ok());
        assert_eq!(
            Layout::parse(&["L", "R", "L"]),
            Err(LayoutError::Duplicate(Label::L))
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
        let err = serde_json::from_str::<Layout>(r#"["L","X"]"#).unwrap_err();
        assert!(
            err.to_string().contains("unknown channel label \"X\""),
            "{err}"
        );
        let err = serde_json::from_str::<Layout>(r#"["R","R"]"#).unwrap_err();
        assert!(
            err.to_string().contains("R appears more than once"),
            "{err}"
        );
    }
}
