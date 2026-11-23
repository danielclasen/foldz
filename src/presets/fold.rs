//! An output's gain matrix from its mapping and layouts.

use super::*;

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
