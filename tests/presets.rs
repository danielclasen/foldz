//! Preset matrices against values written out from the source tables
//! (BS.775-4 Annex 4 Table 2, A/52:2018 clause 7.8.2 and Tables 5.9/5.10,
//! TS 103 190-1 V1.4.1 Table 219), placed by hand on the test layouts.

#![allow(
    clippy::approx_constant,
    reason = "coefficients are transcribed as printed in their sources, not as 1/sqrt(2)"
)]

use foldz::layout::{Label, Layout};
use foldz::presets::{
    Clev, Fold, FoldError, Lfe, LfeInclude, Mapping, Preset, PresetName, Slev, fold,
};

const IN_5_1: [&str; 6] = ["L", "R", "C", "LFE", "Ls", "Rs"];
const IN_7_1: [&str; 8] = ["L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb"];

fn layout(names: &[&str]) -> Layout {
    Layout::parse(names).unwrap()
}

fn run(
    preset: Preset,
    input: &[&str],
    output: &[&str],
    lfe: Option<Lfe>,
) -> Result<Fold, FoldError> {
    fold(
        Mapping::Preset(preset),
        &layout(input),
        &layout(output),
        lfe,
        0.0,
    )
}

/// Asserts every gain bit for bit against `expected[output][input]`.
fn assert_gains(fold: &Fold, expected: &[&[f32]]) {
    let m = &fold.matrix;
    assert_eq!(m.outputs(), expected.len());
    for (j, row) in expected.iter().enumerate() {
        assert_eq!(m.inputs(), row.len());
        for (i, want) in row.iter().enumerate() {
            assert_eq!(
                m.gain(j, i).to_bits(),
                want.to_bits(),
                "output {j}, input {i}: {} != {want}",
                m.gain(j, i)
            );
        }
    }
}

#[test]
fn bs775_3_2_to_1_0() {
    let f = run(Preset::Bs775To1_0, &IN_5_1, &["C"], None).unwrap();
    // C = 0.7071 L + 0.7071 R + 1.0000 C + 0.5000 LS + 0.5000 RS
    assert_gains(&f, &[&[0.7071, 0.7071, 1.0, 0.0, 0.5, 0.5]]);
    assert_eq!(f.consumed, [0, 1, 2, 4, 5]);
}

#[test]
fn bs775_3_2_to_2_0() {
    let f = run(Preset::Bs775To2_0, &IN_5_1, &["L", "R"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.7071, 0.0, 0.7071, 0.0],
            &[0.0, 1.0, 0.7071, 0.0, 0.0, 0.7071],
        ],
    );
    assert_eq!(f.consumed, [0, 1, 2, 4, 5]);
}

#[test]
fn bs775_3_2_to_3_0() {
    let f = run(Preset::Bs775To3_0, &IN_5_1, &["L", "R", "C"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.0, 0.0, 0.7071, 0.0],
            &[0.0, 1.0, 0.0, 0.0, 0.0, 0.7071],
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        ],
    );
}

#[test]
fn bs775_3_2_to_2_1() {
    let f = run(Preset::Bs775To2_1, &IN_5_1, &["L", "R", "S"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.7071, 0.0, 0.0, 0.0],
            &[0.0, 1.0, 0.7071, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.7071, 0.7071],
        ],
    );
}

#[test]
fn bs775_3_2_to_3_1() {
    let f = run(Preset::Bs775To3_1, &IN_5_1, &["L", "R", "C", "S"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.7071, 0.7071],
        ],
    );
}

#[test]
fn bs775_3_2_to_2_2() {
    let f = run(Preset::Bs775To2_2, &IN_5_1, &["L", "R", "Ls", "Rs"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.7071, 0.0, 0.0, 0.0],
            &[0.0, 1.0, 0.7071, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        ],
    );
}

#[test]
fn a52_loro_every_level() {
    // Table 5.9 clev: 00 0.707, 01 0.595, 10 0.500.
    // Table 5.10 slev: 00 0.707, 01 0.500, 10 0.
    let cases: [(Clev, Slev, &str, f32, f32); 9] = [
        (
            Clev::Code00,
            Slev::Code00,
            "a52-loro-clev0.707-slev0.707",
            0.707,
            0.707,
        ),
        (
            Clev::Code00,
            Slev::Code01,
            "a52-loro-clev0.707-slev0.500",
            0.707,
            0.500,
        ),
        (
            Clev::Code00,
            Slev::Code10,
            "a52-loro-clev0.707-slev0",
            0.707,
            0.0,
        ),
        (
            Clev::Code01,
            Slev::Code00,
            "a52-loro-clev0.595-slev0.707",
            0.595,
            0.707,
        ),
        (
            Clev::Code01,
            Slev::Code01,
            "a52-loro-clev0.595-slev0.500",
            0.595,
            0.500,
        ),
        (
            Clev::Code01,
            Slev::Code10,
            "a52-loro-clev0.595-slev0",
            0.595,
            0.0,
        ),
        (
            Clev::Code10,
            Slev::Code00,
            "a52-loro-clev0.500-slev0.707",
            0.500,
            0.707,
        ),
        (
            Clev::Code10,
            Slev::Code01,
            "a52-loro-clev0.500-slev0.500",
            0.500,
            0.500,
        ),
        (
            Clev::Code10,
            Slev::Code10,
            "a52-loro-clev0.500-slev0",
            0.500,
            0.0,
        ),
    ];
    for (clev, slev, name, c, s) in cases {
        let preset = Preset::A52LoRo { clev, slev };
        assert_eq!(preset.name(), name);
        assert_eq!(name.parse(), Ok(PresetName::Table(preset)));
        let f = run(preset, &IN_5_1, &["L", "R"], None).unwrap();
        // Lo = 1.0 L + clev C + slev Ls ; Ro = 1.0 R + clev C + slev Rs
        assert_gains(
            &f,
            &[&[1.0, 0.0, c, 0.0, s, 0.0], &[0.0, 1.0, c, 0.0, 0.0, s]],
        );
    }
}

#[test]
fn a52_loro_without_surround_reads_no_surround() {
    let preset = Preset::A52LoRo {
        clev: Clev::Code00,
        slev: Slev::Code10,
    };
    let f = run(preset, &["L", "R", "C"], &["L", "R"], None).unwrap();
    assert_eq!(f.consumed, [0, 1, 2]);
}

#[test]
fn a52_ltrt() {
    let f = run(Preset::A52LtRt, &IN_5_1, &["Lt", "Rt"], None).unwrap();
    // Lt = 1.0 L + 0.707 C - 0.707 Ls - 0.707 Rs
    // Rt = 1.0 R + 0.707 C + 0.707 Ls + 0.707 Rs
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.707, 0.0, -0.707, -0.707],
            &[0.0, 1.0, 0.707, 0.0, 0.707, 0.707],
        ],
    );
}

#[test]
fn ac4_7_1_to_5_1() {
    let f = run(Preset::Ac4To5_1, &IN_7_1, &IN_5_1, None).unwrap();
    // Table 219 row 3/4/0.x: Ls' = 0.707 Ls + 0.707 Lb, Rs' = 0.707 Rs +
    // 0.707 Rb; L, R, C, LFE at 1.
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.707, 0.0, 0.707, 0.0],
            &[0.0, 0.0, 0.0, 0.0, 0.0, 0.707, 0.0, 0.707],
        ],
    );
    assert_eq!(f.consumed, [0, 1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn ac4_bs775_7_1_to_2_0() {
    let f = run(Preset::Ac4Bs775To2_0, &IN_7_1, &["L", "R"], None).unwrap();
    // L = 1.0 L + 0.7071 C + 0.7071 * (0.707 Ls + 0.707 Lb), R likewise.
    let s = (0.7071f64 * 0.707) as f32;
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.7071, 0.0, s, 0.0, s, 0.0],
            &[0.0, 1.0, 0.7071, 0.0, 0.0, s, 0.0, s],
        ],
    );
    assert_eq!(f.consumed, [0, 1, 2, 4, 5, 6, 7]);
}

#[test]
fn cascade_is_the_product_of_its_stages() {
    let ac4 = Preset::Ac4To5_1.table();
    let bs775 = Preset::Bs775To2_0.table();
    // BS.775 inputs L, R, C, LS, RS are AC-4 outputs 0, 1, 2, 4, 5.
    assert_eq!(
        bs775.inputs,
        [Label::L, Label::R, Label::C, Label::Ls, Label::Rs]
    );
    assert_eq!(
        ac4.outputs,
        [
            Label::L,
            Label::R,
            Label::C,
            Label::Lfe,
            Label::Ls,
            Label::Rs
        ]
    );
    let link = [0, 1, 2, 4, 5];
    let mut product = vec![vec![0.0f64; 8]; 2];
    for (j, row) in product.iter_mut().enumerate() {
        for (i, cell) in row.iter_mut().enumerate() {
            for (k, &a) in link.iter().enumerate() {
                *cell += bs775.rows[j][k] * ac4.rows[a][i];
            }
        }
    }
    let cascade = Preset::Ac4Bs775To2_0.table();
    assert_eq!(cascade.inputs, ac4.inputs);
    assert_eq!(cascade.outputs, bs775.outputs);
    assert_eq!(cascade.rows, product);
}

#[test]
fn lfe_is_dropped_by_default_and_on_request() {
    let none = run(Preset::Bs775To2_0, &IN_5_1, &["L", "R"], None).unwrap();
    let drop = run(Preset::Bs775To2_0, &IN_5_1, &["L", "R"], Some(Lfe::Drop)).unwrap();
    assert_eq!(none, drop);
    assert_eq!(none.matrix.gain(0, 3), 0.0);
    assert!(!none.consumed.contains(&3));
}

#[test]
fn lfe_include_adds_lfe_to_every_output_at_its_gain() {
    let include = Some(Lfe::Include(LfeInclude { gain_db: -10.0 }));
    // 10^(-10/20)
    let g = 0.316_227_766_016_837_94_f64 as f32;
    let f = run(Preset::Bs775To2_0, &IN_5_1, &["L", "R"], include).unwrap();
    assert_gains(
        &f,
        &[
            &[1.0, 0.0, 0.7071, g, 0.7071, 0.0],
            &[0.0, 1.0, 0.7071, g, 0.0, 0.7071],
        ],
    );
    assert_eq!(f.consumed, [0, 1, 2, 3, 4, 5]);

    let f = run(Preset::Bs775To1_0, &IN_5_1, &["C"], include).unwrap();
    assert_gains(&f, &[&[0.7071, 0.7071, 1.0, g, 0.5, 0.5]]);

    let f = run(Preset::Ac4Bs775To2_0, &IN_7_1, &["L", "R"], include).unwrap();
    assert_eq!(f.matrix.gain(0, 3).to_bits(), g.to_bits());
    assert_eq!(f.matrix.gain(1, 3).to_bits(), g.to_bits());
}

#[test]
fn lfe_include_is_offered_for_1_0_and_2_0_only() {
    let include = Some(Lfe::Include(LfeInclude { gain_db: 0.0 }));
    assert_eq!(
        run(Preset::Bs775To3_0, &IN_5_1, &["L", "R", "C"], include),
        Err(FoldError::LfeIncludeNotOffered {
            preset: Preset::Bs775To3_0
        })
    );
}

#[test]
fn lfe_include_needs_an_lfe_input() {
    let include = Some(Lfe::Include(LfeInclude { gain_db: 0.0 }));
    assert_eq!(
        run(
            Preset::Bs775To2_0,
            &["L", "R", "C", "Ls", "Rs"],
            &["L", "R"],
            include
        ),
        Err(FoldError::MissingInput {
            mapping: "bs775-3_2-to-2_0".into(),
            label: Label::Lfe
        })
    );
}

#[test]
fn lfe_setting_is_refused_where_the_output_carries_lfe() {
    for lfe in [Lfe::Drop, Lfe::Include(LfeInclude { gain_db: 0.0 })] {
        assert_eq!(
            run(Preset::Ac4To5_1, &IN_7_1, &IN_5_1, Some(lfe)),
            Err(FoldError::LfeNotApplicable {
                mapping: "ac4-7_1-to-5_1".into()
            })
        );
    }
}

#[test]
fn lfe_gain_must_be_finite() {
    let include = Some(Lfe::Include(LfeInclude { gain_db: f64::NAN }));
    assert!(matches!(
        run(Preset::Bs775To2_0, &IN_5_1, &["L", "R"], include),
        Err(FoldError::GainDb {
            what: "lfe gainDb",
            ..
        })
    ));
}

#[test]
fn output_layout_must_be_the_presets() {
    assert_eq!(
        run(Preset::Bs775To2_0, &IN_5_1, &["R", "L"], None),
        Err(FoldError::OutputLayout {
            preset: Preset::Bs775To2_0,
            expected: vec![Label::L, Label::R]
        })
    );
    assert!(run(Preset::A52LtRt, &IN_5_1, &["L", "R"], None).is_err());
}

#[test]
fn input_must_carry_every_channel_the_preset_reads() {
    assert_eq!(
        run(
            Preset::Bs775To2_0,
            &["L", "R", "Ls", "Rs"],
            &["L", "R"],
            None
        ),
        Err(FoldError::MissingInput {
            mapping: "bs775-3_2-to-2_0".into(),
            label: Label::C
        })
    );
    // The cascade does not read LFE, so a 7.0 input is enough.
    assert!(
        run(
            Preset::Ac4Bs775To2_0,
            &["L", "R", "C", "Ls", "Rs", "Lb", "Rb"],
            &["L", "R"],
            None
        )
        .is_ok()
    );
}

#[test]
fn input_channels_are_found_by_label_in_any_order() {
    let input = ["Rs", "-", "Ls", "C", "R", "L"];
    let f = run(Preset::Bs775To2_0, &input, &["L", "R"], None).unwrap();
    assert_gains(
        &f,
        &[
            &[0.0, 0.0, 0.7071, 0.7071, 0.0, 1.0],
            &[0.7071, 0.0, 0.0, 0.7071, 1.0, 0.0],
        ],
    );
    assert_eq!(f.consumed, [0, 2, 3, 4, 5]);
}

#[test]
fn gain_db_scales_every_gain() {
    let input = layout(&IN_5_1);
    let f = fold(
        Mapping::Preset(Preset::Bs775To2_0),
        &input,
        &layout(&["L", "R"]),
        None,
        -6.0,
    )
    .unwrap();
    // 10^(-6/20)
    let g = 0.501_187_233_627_272_2_f64;
    assert_eq!(f.matrix.gain(0, 0).to_bits(), (g as f32).to_bits());
    assert_eq!(
        f.matrix.gain(0, 2).to_bits(),
        ((0.7071 * g) as f32).to_bits()
    );
    assert!(matches!(
        fold(
            Mapping::Select,
            &input,
            &layout(&["L"]),
            None,
            f64::INFINITY
        ),
        Err(FoldError::GainDb { what: "gainDb", .. })
    ));
    // 0.7071 * 10^(200/20) exceeds samplez-dsp's MAX_GAIN of 1e6.
    assert!(matches!(
        fold(
            Mapping::Preset(Preset::Bs775To2_0),
            &input,
            &layout(&["L", "R"]),
            None,
            200.0
        ),
        Err(FoldError::Coefficient {
            output: 0,
            input: 0,
            ..
        })
    ));
}

#[test]
fn select_copies_by_label() {
    let f = fold(
        Mapping::Select,
        &layout(&IN_5_1),
        &layout(&["Rs", "L"]),
        None,
        0.0,
    )
    .unwrap();
    assert_gains(
        &f,
        &[
            &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ],
    );
    assert_eq!(f.consumed, [0, 5]);
    assert_eq!(
        fold(
            Mapping::Select,
            &layout(&IN_5_1),
            &layout(&["Lb"]),
            None,
            0.0
        ),
        Err(FoldError::MissingInput {
            mapping: "select".into(),
            label: Label::Lb
        })
    );
    assert_eq!(
        fold(
            Mapping::Select,
            &layout(&IN_5_1),
            &layout(&["L", "-"]),
            None,
            0.0
        ),
        Err(FoldError::SelectUnlabelled { output: 1 })
    );
    assert_eq!(
        fold(
            Mapping::Select,
            &layout(&IN_5_1),
            &layout(&["L"]),
            Some(Lfe::Drop),
            0.0
        ),
        Err(FoldError::LfeNotApplicable {
            mapping: "select".into()
        })
    );
}

#[test]
fn shuffle_copies_by_index() {
    let input = layout(&["-", "-", "-", "-"]);
    let f = fold(
        Mapping::Shuffle(&[3, 0, 0]),
        &input,
        &layout(&["-", "-", "-"]),
        None,
        0.0,
    )
    .unwrap();
    assert_gains(
        &f,
        &[
            &[0.0, 0.0, 0.0, 1.0],
            &[1.0, 0.0, 0.0, 0.0],
            &[1.0, 0.0, 0.0, 0.0],
        ],
    );
    assert_eq!(f.consumed, [0, 3]);
    assert_eq!(
        fold(
            Mapping::Shuffle(&[0]),
            &input,
            &layout(&["-", "-"]),
            None,
            0.0
        ),
        Err(FoldError::ShuffleLength {
            expected: 2,
            got: 1
        })
    );
    assert_eq!(
        fold(Mapping::Shuffle(&[4]), &input, &layout(&["-"]), None, 0.0),
        Err(FoldError::ShuffleIndex {
            output: 0,
            index: 4,
            inputs: 4
        })
    );
}

#[test]
fn explicit_matrix_is_taken_as_given() {
    let input = layout(&["-", "-", "-"]);
    let output = layout(&["L", "R"]);
    let rows = vec![vec![0.25, -1.0, 0.0], vec![0.0, 0.5, 2.0]];
    let f = fold(Mapping::Matrix(&rows), &input, &output, None, 0.0).unwrap();
    assert_gains(&f, &[&[0.25, -1.0, 0.0], &[0.0, 0.5, 2.0]]);
    assert_eq!(
        fold(Mapping::Matrix(&rows[..1]), &input, &output, None, 0.0),
        Err(FoldError::MatrixRows {
            expected: 2,
            got: 1
        })
    );
    let short = vec![vec![1.0, 0.0, 0.0], vec![1.0]];
    assert_eq!(
        fold(Mapping::Matrix(&short), &input, &output, None, 0.0),
        Err(FoldError::MatrixColumns {
            row: 1,
            expected: 3,
            got: 1
        })
    );
    for bad in [f64::NAN, f64::INFINITY, 2.0e6] {
        let rows = vec![vec![1.0, 0.0, 0.0], vec![0.0, bad, 0.0]];
        assert!(matches!(
            fold(Mapping::Matrix(&rows), &input, &output, None, 0.0),
            Err(FoldError::Coefficient {
                output: 1,
                input: 1,
                ..
            })
        ));
    }
}

#[test]
fn preset_names_are_the_config_names() {
    let mut names: Vec<String> = Preset::all().into_iter().map(Preset::name).collect();
    names.sort();
    let mut want = vec![
        "bs775-3_2-to-1_0",
        "bs775-3_2-to-2_0",
        "bs775-3_2-to-3_0",
        "bs775-3_2-to-2_1",
        "bs775-3_2-to-3_1",
        "bs775-3_2-to-2_2",
        "a52-loro-clev0.707-slev0.707",
        "a52-loro-clev0.707-slev0.500",
        "a52-loro-clev0.707-slev0",
        "a52-loro-clev0.595-slev0.707",
        "a52-loro-clev0.595-slev0.500",
        "a52-loro-clev0.595-slev0",
        "a52-loro-clev0.500-slev0.707",
        "a52-loro-clev0.500-slev0.500",
        "a52-loro-clev0.500-slev0",
        "a52-ltrt",
        "ac4-7_1-to-5_1",
        "ac4-bs775-7_1-to-2_0",
    ];
    want.sort();
    assert_eq!(names, want);
    for preset in Preset::all() {
        assert_eq!(preset.name().parse(), Ok(PresetName::Table(preset)));
        assert!(!preset.citation().is_empty());
    }
    assert_eq!("select".parse(), Ok(PresetName::Select));
    assert_eq!("shuffle".parse(), Ok(PresetName::Shuffle));
    assert!("a52-loro".parse::<PresetName>().is_err());
    assert!("bs775-3_2-to-2_0 ".parse::<PresetName>().is_err());
}
