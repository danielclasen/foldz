//! Downmix presets and the gain matrix an output applies to its input.
//!
//! Every preset coefficient is transcribed from the document cited at its
//! table, without headroom scaling: the tables are unscaled in their
//! sources, and an output's `gainDb` is the only scaling foldz applies.

#![allow(
    clippy::approx_constant,
    reason = "coefficients are transcribed as printed in their sources, not as 1/sqrt(2)"
)]

use std::fmt;
use std::str::FromStr;

use samplez_dsp::{MAX_GAIN, Matrix};
use serde::{Deserialize, Deserializer};

use crate::layout::{Label, Layout};

use Label::{C, L, Lrs, Ls, Lss, Lt, R, Rrs, Rs, Rss, Rt, S};

/// One downmix as published: `rows[j][i]` is the gain from source channel
/// `inputs[i]` to output channel `outputs[j]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub inputs: Vec<Label>,
    pub outputs: Vec<Label>,
    pub rows: Vec<Vec<f64>>,
}

impl Table {
    fn from_static<const N: usize>(
        inputs: [Label; N],
        outputs: &[Label],
        rows: &[[f64; N]],
    ) -> Self {
        Table {
            inputs: inputs.to_vec(),
            outputs: outputs.to_vec(),
            rows: rows.iter().map(|row| row.to_vec()).collect(),
        }
    }

    /// The table of `first` followed by `second`: the matrix product
    /// `second * first`, matching each input of `second` to the output of
    /// `first` with the same label. Inputs of `first` that reach no output
    /// of `second` keep an all-zero column.
    ///
    /// # Panics
    ///
    /// If an input of `second` is not an output of `first`.
    pub fn cascade(first: &Table, second: &Table) -> Table {
        let link: Vec<usize> = second
            .inputs
            .iter()
            .map(|label| {
                first
                    .outputs
                    .iter()
                    .position(|l| l == label)
                    .unwrap_or_else(|| panic!("{label} is not an output of the first stage"))
            })
            .collect();
        let rows = second
            .rows
            .iter()
            .map(|row| {
                (0..first.inputs.len())
                    .map(|i| {
                        row.iter()
                            .zip(&link)
                            .fold(0.0, |sum, (g, &k)| sum + g * first.rows[k][i])
                    })
                    .collect()
            })
            .collect();
        Table {
            inputs: first.inputs.clone(),
            outputs: second.outputs.clone(),
            rows,
        }
    }
}

// ITU-R BS.775-4 (12/2022), "Multichannel stereophonic sound system with
// and without accompanying picture", Annex 4, clause 1, Table 2
// "Downward mixing equations for 3/2 source material". Columns L, R, C,
// LS, RS. The table has no LFE column and no headroom scaling.
const BS775_INPUTS: [Label; 5] = [L, R, C, Ls, Rs];
/// Table 2, Mono - 1/0 format.
const BS775_1_0: [[f64; 5]; 1] = [[0.7071, 0.7071, 1.0000, 0.5000, 0.5000]];
/// Table 2, Stereo - 2/0 format.
const BS775_2_0: [[f64; 5]; 2] = [
    [1.0000, 0.0000, 0.7071, 0.7071, 0.0000],
    [0.0000, 1.0000, 0.7071, 0.0000, 0.7071],
];
/// Table 2, Three channels - 3/0 format.
const BS775_3_0: [[f64; 5]; 3] = [
    [1.0000, 0.0000, 0.0000, 0.7071, 0.0000],
    [0.0000, 1.0000, 0.0000, 0.0000, 0.7071],
    [0.0000, 0.0000, 1.0000, 0.0000, 0.0000],
];
/// Table 2, Three channels - 2/1 format.
const BS775_2_1: [[f64; 5]; 3] = [
    [1.0000, 0.0000, 0.7071, 0.0000, 0.0000],
    [0.0000, 1.0000, 0.7071, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.7071, 0.7071],
];
/// Table 2, Four channels - 3/1 format.
const BS775_3_1: [[f64; 5]; 4] = [
    [1.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 1.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 1.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.7071, 0.7071],
];
/// Table 2, Four channels - 2/2 format.
const BS775_2_2: [[f64; 5]; 4] = [
    [1.0000, 0.0000, 0.7071, 0.0000, 0.0000],
    [0.0000, 1.0000, 0.7071, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 1.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 1.0000],
];

// ATSC A/52:2018, "Digital Audio Compression (AC-3, E-AC-3)", clause
// 7.8.2 "Downmixing into Two Channels", for a 3/2 source, prior to the
// scaling needed to prevent overflow:
//   Lo = 1.0 * L + clev * C + slev * Ls
//   Ro = 1.0 * R + clev * C + slev * Rs
//   Lt = 1.0 * L + 0.707 * C - 0.707 * Ls - 0.707 * Rs
//   Rt = 1.0 * R + 0.707 * C + 0.707 * Ls + 0.707 * Rs
// Lt/Rt coefficients are fixed; clev and slev apply to Lo/Ro only.
const A52_INPUTS: [Label; 5] = [L, R, C, Ls, Rs];
const A52_LTRT: [[f64; 5]; 2] = [
    [1.0, 0.0, 0.707, -0.707, -0.707],
    [0.0, 1.0, 0.707, 0.707, 0.707],
];

/// Centre mix level of A/52:2018 Table 5.9 (cmixlev); the reserved code
/// `11` is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clev {
    /// cmixlev `00`: 0.707 (-3.0 dB).
    Code00,
    /// cmixlev `01`: 0.595 (-4.5 dB).
    Code01,
    /// cmixlev `10`: 0.500 (-6.0 dB).
    Code10,
}

impl Clev {
    pub const ALL: [Clev; 3] = [Clev::Code00, Clev::Code01, Clev::Code10];

    /// The gain as printed in Table 5.9.
    pub fn gain(self) -> f64 {
        match self {
            Clev::Code00 => 0.707,
            Clev::Code01 => 0.595,
            Clev::Code10 => 0.500,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Clev::Code00 => "0.707",
            Clev::Code01 => "0.595",
            Clev::Code10 => "0.500",
        }
    }
}

/// Surround mix level of A/52:2018 Table 5.10 (surmixlev); the reserved
/// code `11` is not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slev {
    /// surmixlev `00`: 0.707 (-3 dB).
    Code00,
    /// surmixlev `01`: 0.500 (-6 dB).
    Code01,
    /// surmixlev `10`: 0.
    Code10,
}

impl Slev {
    pub const ALL: [Slev; 3] = [Slev::Code00, Slev::Code01, Slev::Code10];

    /// The gain as printed in Table 5.10.
    pub fn gain(self) -> f64 {
        match self {
            Slev::Code00 => 0.707,
            Slev::Code01 => 0.500,
            Slev::Code10 => 0.0,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Slev::Code00 => "0.707",
            Slev::Code01 => "0.500",
            Slev::Code10 => "0",
        }
    }
}

// ETSI TS 103 190-1 V1.4.1 (2025-07), AC-4 part 1, clause 6.2.17.5
// "Downmixing from seven channels into five channels", Table 219, row
// 3/4/0.x: r3,3 = r4,4 = 0.707 and r3,7 = r4,8 = 0.707, with L, C, R
// (r0,0, r1,1, r2,2) and LFE (r18,18) at 1. AC-4 indexes channels L 0,
// C 1, R 2, Ls 3, Rs 4, Lb 7, Rb 8, LFE 18, where in 3/4/0 Ls and Rs are
// the side surrounds and Lb and Rb the back surrounds.
//
// Layouts carry IS-04 v1.3 channel symbols only, which have no Lb or Rb.
// A 7.1 input is labelled as SMPTE 7.1 practice labels it, side
// surrounds Lss/Rss and rear surrounds Lrs/Rrs, so AC-4 Ls/Rs (index
// 3/4) read Lss/Rss and AC-4 Lb/Rb (index 7/8) read Lrs/Rrs. The 5.1
// output's surrounds are Ls/Rs, the symbols of a 3/2 layout:
//   Ls' = 0.707 * Lss + 0.707 * Lrs ; Rs' = 0.707 * Rss + 0.707 * Rrs.
// The rows below are in foldz's channel order.
const AC4_7_1_INPUTS: [Label; 8] = [L, R, C, Label::Lfe, Lss, Rss, Lrs, Rrs];
const AC4_5_1_OUTPUTS: [Label; 6] = [L, R, C, Label::Lfe, Ls, Rs];
const AC4_7_1_TO_5_1: [[f64; 8]; 6] = [
    [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.707, 0.0, 0.707, 0.0],
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.707, 0.0, 0.707],
];

/// A downmix defined by a published table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// `bs775-3_2-to-1_0`: BS.775-4 Table 2, 3/2 to 1/0.
    Bs775To1_0,
    /// `bs775-3_2-to-2_0`: BS.775-4 Table 2, 3/2 to 2/0.
    Bs775To2_0,
    /// `bs775-3_2-to-3_0`: BS.775-4 Table 2, 3/2 to 3/0.
    Bs775To3_0,
    /// `bs775-3_2-to-2_1`: BS.775-4 Table 2, 3/2 to 2/1.
    Bs775To2_1,
    /// `bs775-3_2-to-3_1`: BS.775-4 Table 2, 3/2 to 3/1.
    Bs775To3_1,
    /// `bs775-3_2-to-2_2`: BS.775-4 Table 2, 3/2 to 2/2.
    Bs775To2_2,
    /// `a52-loro-clev<clev>-slev<slev>`: A/52:2018 clause 7.8.2 Lo/Ro, 3/2
    /// to 2/0, output labelled `[L, R]`.
    A52LoRo { clev: Clev, slev: Slev },
    /// `a52-ltrt`: A/52:2018 clause 7.8.2 Lt/Rt, 3/2 to 2/0.
    A52LtRt,
    /// `ac4-7_1-to-5_1`: TS 103 190-1 V1.4.1 Table 219, row 3/4/0.x.
    Ac4To5_1,
    /// `ac4-bs775-7_1-to-2_0`: the cascade of `ac4-7_1-to-5_1` and
    /// `bs775-3_2-to-2_0`.
    Ac4Bs775To2_0,
}

impl Preset {
    /// Every preset.
    pub fn all() -> Vec<Preset> {
        let mut all = vec![
            Preset::Bs775To1_0,
            Preset::Bs775To2_0,
            Preset::Bs775To3_0,
            Preset::Bs775To2_1,
            Preset::Bs775To3_1,
            Preset::Bs775To2_2,
        ];
        for clev in Clev::ALL {
            for slev in Slev::ALL {
                all.push(Preset::A52LoRo { clev, slev });
            }
        }
        all.extend([Preset::A52LtRt, Preset::Ac4To5_1, Preset::Ac4Bs775To2_0]);
        all
    }

    /// The name a config uses.
    pub fn name(self) -> String {
        match self {
            Preset::Bs775To1_0 => "bs775-3_2-to-1_0".into(),
            Preset::Bs775To2_0 => "bs775-3_2-to-2_0".into(),
            Preset::Bs775To3_0 => "bs775-3_2-to-3_0".into(),
            Preset::Bs775To2_1 => "bs775-3_2-to-2_1".into(),
            Preset::Bs775To3_1 => "bs775-3_2-to-3_1".into(),
            Preset::Bs775To2_2 => "bs775-3_2-to-2_2".into(),
            Preset::A52LoRo { clev, slev } => {
                format!("a52-loro-clev{}-slev{}", clev.name(), slev.name())
            }
            Preset::A52LtRt => "a52-ltrt".into(),
            Preset::Ac4To5_1 => "ac4-7_1-to-5_1".into(),
            Preset::Ac4Bs775To2_0 => "ac4-bs775-7_1-to-2_0".into(),
        }
    }

    /// The document, revision and table the coefficients come from.
    pub fn citation(self) -> &'static str {
        match self {
            Preset::Bs775To1_0
            | Preset::Bs775To2_0
            | Preset::Bs775To3_0
            | Preset::Bs775To2_1
            | Preset::Bs775To3_1
            | Preset::Bs775To2_2 => "ITU-R BS.775-4 (12/2022), Annex 4, Table 2",
            Preset::A52LoRo { .. } => {
                "ATSC A/52:2018, clause 7.8.2 (Lo/Ro), clev from Table 5.9, slev from Table 5.10"
            }
            Preset::A52LtRt => "ATSC A/52:2018, clause 7.8.2 (Lt/Rt)",
            Preset::Ac4To5_1 => "ETSI TS 103 190-1 V1.4.1, clause 6.2.17.5, Table 219, row 3/4/0.x",
            Preset::Ac4Bs775To2_0 => {
                "ETSI TS 103 190-1 V1.4.1, clause 6.2.17.5, Table 219, row 3/4/0.x, \
                 followed by ITU-R BS.775-4 (12/2022), Annex 4, Table 2, 2/0 format"
            }
        }
    }

    /// The coefficients, in the channel order of the source document's
    /// equations.
    pub fn table(self) -> Table {
        let bs775 =
            |rows: &[[f64; 5]], outputs: &[Label]| Table::from_static(BS775_INPUTS, outputs, rows);
        match self {
            Preset::Bs775To1_0 => bs775(&BS775_1_0, &[C]),
            Preset::Bs775To2_0 => bs775(&BS775_2_0, &[L, R]),
            Preset::Bs775To3_0 => bs775(&BS775_3_0, &[L, R, C]),
            Preset::Bs775To2_1 => bs775(&BS775_2_1, &[L, R, S]),
            Preset::Bs775To3_1 => bs775(&BS775_3_1, &[L, R, C, S]),
            Preset::Bs775To2_2 => bs775(&BS775_2_2, &[L, R, Ls, Rs]),
            Preset::A52LoRo { clev, slev } => {
                let (c, s) = (clev.gain(), slev.gain());
                let rows = [[1.0, 0.0, c, s, 0.0], [0.0, 1.0, c, 0.0, s]];
                Table::from_static(A52_INPUTS, &[L, R], &rows)
            }
            Preset::A52LtRt => Table::from_static(A52_INPUTS, &[Lt, Rt], &A52_LTRT),
            Preset::Ac4To5_1 => {
                Table::from_static(AC4_7_1_INPUTS, &AC4_5_1_OUTPUTS, &AC4_7_1_TO_5_1)
            }
            Preset::Ac4Bs775To2_0 => {
                Table::cascade(&Preset::Ac4To5_1.table(), &Preset::Bs775To2_0.table())
            }
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// The `preset` of an output: a published downmix, or a routing by label
/// (`select`) or by channel index (`shuffle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetName {
    Table(Preset),
    Select,
    Shuffle,
}

impl FromStr for PresetName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "select" => Ok(PresetName::Select),
            "shuffle" => Ok(PresetName::Shuffle),
            _ => Preset::all()
                .into_iter()
                .find(|p| p.name() == s)
                .map(PresetName::Table)
                .ok_or_else(|| {
                    let names: Vec<String> = Preset::all().into_iter().map(Preset::name).collect();
                    format!(
                        "unknown preset {s:?}, expected select, shuffle or one of {}",
                        names.join(", ")
                    )
                }),
        }
    }
}

impl fmt::Display for PresetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PresetName::Table(p) => p.fmt(f),
            PresetName::Select => f.write_str("select"),
            PresetName::Shuffle => f.write_str("shuffle"),
        }
    }
}

impl<'de> Deserialize<'de> for PresetName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What a preset does with the input's LFE channel when its output has no
/// LFE channel.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum Lfe {
    /// Leave LFE out, as BS.775-4 Table 2 and the A/52:2018 clause 7.8.2
    /// equations do.
    Drop,
    /// Add LFE to every output channel at `gainDb`. The output's own
    /// `gainDb` scales the whole matrix after that, so LFE reaches the
    /// output at the sum of both. Offered for 1/0 and 2/0 outputs only,
    /// the output modes A/52:2018 Annex E clause 3.9 mixes LFE into.
    Include(LfeInclude),
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LfeInclude {
    pub gain_db: f64,
}

/// How an output's channels are made from its input's.
#[derive(Debug, Clone, Copy)]
pub enum Mapping<'a> {
    Preset(Preset),
    /// Each output channel copies the input channel with its label.
    Select,
    /// Output channel `j` copies input channel `source[j]` (0-based).
    Shuffle(&'a [usize]),
    /// `rows[j][i]` is the gain from input channel `i` to output channel
    /// `j`.
    Matrix(&'a [Vec<f64>]),
}

/// Why a mapping does not fit its layouts.
#[derive(Debug, Clone, PartialEq)]
pub enum FoldError {
    /// The input layout lacks a channel the mapping reads.
    MissingInput {
        mapping: String,
        label: Label,
    },
    /// The output layout differs from the channels the preset produces.
    OutputLayout {
        preset: Preset,
        expected: Vec<Label>,
    },
    /// `lfe` is set for a mapping it does not apply to.
    LfeNotApplicable {
        mapping: String,
    },
    /// `lfe: include` with a preset whose output is not 1/0 or 2/0.
    LfeIncludeNotOffered {
        preset: Preset,
    },
    /// `select` with an unlabelled output channel.
    SelectUnlabelled {
        output: usize,
    },
    ShuffleLength {
        expected: usize,
        got: usize,
    },
    ShuffleIndex {
        output: usize,
        index: usize,
        inputs: usize,
    },
    MatrixRows {
        expected: usize,
        got: usize,
    },
    MatrixColumns {
        row: usize,
        expected: usize,
        got: usize,
    },
    /// A gain in dB is not a finite number.
    GainDb {
        what: &'static str,
        value: f64,
    },
    /// A coefficient, after `gainDb`, is not finite or exceeds
    /// [`MAX_GAIN`] in magnitude.
    Coefficient {
        output: usize,
        input: usize,
        value: f64,
    },
}

impl fmt::Display for FoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels = |v: &[Label]| {
            v.iter()
                .map(Label::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            FoldError::MissingInput { mapping, label } => {
                write!(
                    f,
                    "{mapping} reads channel {label}, which the input layout lacks"
                )
            }
            FoldError::OutputLayout { preset, expected } => write!(
                f,
                "{preset} produces [{}]; the output layout must be exactly that",
                labels(expected)
            ),
            FoldError::LfeNotApplicable { mapping } => write!(f, "lfe does not apply to {mapping}"),
            FoldError::LfeIncludeNotOffered { preset } => write!(
                f,
                "lfe include is offered for 1/0 and 2/0 outputs only, and {preset} is neither"
            ),
            FoldError::SelectUnlabelled { output } => {
                write!(f, "select needs a label on output channel {output}")
            }
            FoldError::ShuffleLength { expected, got } => write!(
                f,
                "shuffle source has {got} entries, the output layout {expected} channels"
            ),
            FoldError::ShuffleIndex {
                output,
                index,
                inputs,
            } => write!(
                f,
                "shuffle source of output channel {output} is input channel {index}, \
                 the input has {inputs} channels (0-based)"
            ),
            FoldError::MatrixRows { expected, got } => write!(
                f,
                "matrix has {got} rows, one per output channel needs {expected}"
            ),
            FoldError::MatrixColumns { row, expected, got } => write!(
                f,
                "matrix row {row} has {got} entries, one per input channel needs {expected}"
            ),
            FoldError::GainDb { what, value } => write!(f, "{what} {value} is not finite"),
            FoldError::Coefficient {
                output,
                input,
                value,
            } => write!(
                f,
                "gain {value} from input channel {input} to output channel {output} \
                 is not finite or exceeds {MAX_GAIN} in magnitude"
            ),
        }
    }
}

impl std::error::Error for FoldError {}

/// An `outputs x inputs` gain matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMatrix {
    inputs: usize,
    outputs: usize,
    gains: Vec<f32>,
}

impl GainMatrix {
    pub fn inputs(&self) -> usize {
        self.inputs
    }

    pub fn outputs(&self) -> usize {
        self.outputs
    }

    /// The gain from `input` to `output`.
    pub fn gain(&self, output: usize, input: usize) -> f32 {
        assert!(input < self.inputs && output < self.outputs);
        self.gains[output * self.inputs + input]
    }

    /// A samplez-dsp [`Matrix`] holding these gains.
    pub fn to_dsp(&self) -> Matrix {
        let mut matrix = Matrix::new(self.inputs, self.outputs);
        for output in 0..self.outputs {
            for input in 0..self.inputs {
                let gain = self.gain(output, input);
                if gain != 0.0 {
                    assert!(
                        matrix.set(input, output, gain, 0),
                        "fold validated the gain"
                    );
                }
            }
        }
        matrix
    }
}

/// The gain matrix of one output and the input channels it reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Fold {
    pub matrix: GainMatrix,
    /// Input channels with a nonzero gain to any output channel, ascending.
    pub consumed: Vec<usize>,
}

/// Builds the gain matrix that makes `output` from `input` by `mapping`,
/// with the input's LFE handled per `lfe` and every gain scaled by
/// `gain_db`.
///
/// `lfe` applies only to presets whose output has no LFE channel; absent,
/// it is [`Lfe::Drop`] for those.
pub fn fold(
    mapping: Mapping<'_>,
    input: &Layout,
    output: &Layout,
    lfe: Option<Lfe>,
    gain_db: f64,
) -> Result<Fold, FoldError> {
    let (n, m) = (input.len(), output.len());
    let mut rows = vec![vec![0.0f64; n]; m];
    let find = |mapping: &dyn fmt::Display, label: Label| {
        input.position(label).ok_or(FoldError::MissingInput {
            mapping: mapping.to_string(),
            label,
        })
    };
    let no_lfe = |name: &str| match lfe {
        Some(_) => Err(FoldError::LfeNotApplicable {
            mapping: name.to_owned(),
        }),
        None => Ok(()),
    };
    match mapping {
        Mapping::Preset(preset) => {
            let table = preset.table();
            if output.labels() != table.outputs {
                return Err(FoldError::OutputLayout {
                    preset,
                    expected: table.outputs,
                });
            }
            for (i, label) in table.inputs.iter().enumerate() {
                if table.rows.iter().all(|row| row[i] == 0.0) {
                    continue;
                }
                let at = find(&preset, *label)?;
                for (row, from) in rows.iter_mut().zip(&table.rows) {
                    row[at] = from[i];
                }
            }
            if table.outputs.contains(&Label::Lfe) {
                no_lfe(&preset.name())?;
            } else if let Some(Lfe::Include(include)) = lfe {
                if m > 2 {
                    return Err(FoldError::LfeIncludeNotOffered { preset });
                }
                let gain = db_to_gain("lfe gainDb", include.gain_db)?;
                let at = find(&preset, Label::Lfe)?;
                for row in &mut rows {
                    row[at] = gain;
                }
            }
        }
        Mapping::Select => {
            no_lfe("select")?;
            for (j, label) in output.labels().iter().enumerate() {
                if *label == Label::Unused {
                    return Err(FoldError::SelectUnlabelled { output: j });
                }
                rows[j][find(&"select", *label)?] = 1.0;
            }
        }
        Mapping::Shuffle(source) => {
            no_lfe("shuffle")?;
            if source.len() != m {
                return Err(FoldError::ShuffleLength {
                    expected: m,
                    got: source.len(),
                });
            }
            for (j, &index) in source.iter().enumerate() {
                if index >= n {
                    return Err(FoldError::ShuffleIndex {
                        output: j,
                        index,
                        inputs: n,
                    });
                }
                rows[j][index] = 1.0;
            }
        }
        Mapping::Matrix(given) => {
            no_lfe("matrix")?;
            if given.len() != m {
                return Err(FoldError::MatrixRows {
                    expected: m,
                    got: given.len(),
                });
            }
            for (j, row) in given.iter().enumerate() {
                if row.len() != n {
                    return Err(FoldError::MatrixColumns {
                        row: j,
                        expected: n,
                        got: row.len(),
                    });
                }
                rows[j].clone_from(row);
            }
        }
    }

    let scale = db_to_gain("gainDb", gain_db)?;
    let mut gains = Vec::with_capacity(m * n);
    for (output, row) in rows.iter().enumerate() {
        for (input, &g) in row.iter().enumerate() {
            let value = g * scale;
            let gain = value as f32;
            if !gain.is_finite() || gain.abs() > MAX_GAIN {
                return Err(FoldError::Coefficient {
                    output,
                    input,
                    value,
                });
            }
            gains.push(gain);
        }
    }
    let consumed = (0..n)
        .filter(|&i| (0..m).any(|j| gains[j * n + i] != 0.0))
        .collect();
    Ok(Fold {
        matrix: GainMatrix {
            inputs: n,
            outputs: m,
            gains,
        },
        consumed,
    })
}

/// `10^(db/20)`; 0 dB is exactly 1.
fn db_to_gain(what: &'static str, db: f64) -> Result<f64, FoldError> {
    if !db.is_finite() {
        return Err(FoldError::GainDb { what, value: db });
    }
    Ok(10f64.powf(db / 20.0))
}
