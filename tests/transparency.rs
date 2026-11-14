//! `select` and `shuffle` at 0 dB pass every input sample through the
//! samplez-dsp matrix unchanged, bit for bit, on every matrix path and
//! with FTZ/DAZ on and off.

use foldz::layout::Layout;
use foldz::presets::{Mapping, fold};
use samplez_dsp::{Path, PlanarBuffer, restore_mxcsr, set_ftz_daz};

/// Bit patterns a multiply or an add could change: signed zero, quiet and
/// signalling NaNs with payloads, denormals, infinities and extremes.
const SPECIAL: [u32; 12] = [
    0x8000_0000, // -0.0
    0x0000_0000, // +0.0
    0x7fc0_1234, // quiet NaN, payload
    0xffc0_0001, // negative quiet NaN, payload
    0x7fa0_5678, // signalling NaN, payload
    0x0000_0001, // smallest denormal
    0x807f_ffff, // largest negative denormal
    0x7f80_0000, // +inf
    0xff80_0000, // -inf
    0x7f7f_ffff, // f32::MAX
    0x3f80_0000, // 1.0
    0xbeaa_aaab, // -1/3
];

/// Whether `path` runs here; a path named in `SAMPLEZ_REQUIRE_PATHS` that
/// this CPU lacks fails the test.
fn path_runs(path: Path) -> bool {
    let required = std::env::var("SAMPLEZ_REQUIRE_PATHS").unwrap_or_default();
    let named = required
        .split(',')
        .any(|n| n.trim().eq_ignore_ascii_case(&format!("{path:?}")));
    assert!(
        path.is_supported() || !named,
        "{path:?} is required but not supported by this CPU"
    );
    path.is_supported()
}

fn check(mapping: Mapping<'_>, input: &[&str], output: &[&str], copies: &[usize]) {
    let f = fold(
        mapping,
        &Layout::parse(input).unwrap(),
        &Layout::parse(output).unwrap(),
        None,
        0.0,
    )
    .unwrap();
    let frames = 61;
    let mut inbuf = PlanarBuffer::new(input.len(), frames);
    for ch in 0..input.len() {
        for (n, s) in inbuf.channel_mut(ch)[..frames].iter_mut().enumerate() {
            *s = f32::from_bits(SPECIAL[(n + 5 * ch) % SPECIAL.len()]);
        }
    }
    let mut ran = 0;
    for path in Path::ALL {
        if !path_runs(path) {
            continue;
        }
        for ftz_daz in [false, true] {
            let saved = set_ftz_daz(ftz_daz);
            let mut matrix = f.matrix.to_dsp();
            let mut outbuf = PlanarBuffer::new(output.len(), frames);
            matrix.process_on(path, &inbuf, &mut outbuf, frames);
            restore_mxcsr(saved);
            for (j, &i) in copies.iter().enumerate() {
                let got: Vec<u32> = outbuf.channel(j)[..frames]
                    .iter()
                    .map(|s| s.to_bits())
                    .collect();
                let want: Vec<u32> = inbuf.channel(i)[..frames]
                    .iter()
                    .map(|s| s.to_bits())
                    .collect();
                assert_eq!(
                    got, want,
                    "{path:?}, ftz/daz {ftz_daz}, output {j} from input {i}"
                );
            }
            ran += 1;
        }
    }
    assert!(ran > 0, "no matrix path ran");
}

#[test]
fn select_is_bit_transparent() {
    check(
        Mapping::Select,
        &["L", "R", "C", "LFE", "Ls", "Rs"],
        &["Rs", "C", "L"],
        &[5, 2, 0],
    );
}

#[test]
fn shuffle_is_bit_transparent() {
    let source = [3, 0, 7, 1, 1, 6];
    check(Mapping::Shuffle(&source), &["-"; 8], &["-"; 6], &source);
}
