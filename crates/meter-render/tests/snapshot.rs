//! Image snapshot tests for the tray gauge renderer.
//!
//! Renders are compared to the PNGs in `tests/snapshots/` via perceptual
//! hashes (difference + average hash over a downsampled grayscale grid), not
//! byte equality — `ClaudeMeter` PR #31 learned the hard way that pixel-exact
//! snapshots drift across OSes/rasterizer versions. A few bits of Hamming
//! distance of anti-aliasing drift pass; a real shape/fill change fails.
//!
//! Regenerate the snapshots with:
//! `UPDATE_SNAPSHOTS=1 cargo test -p meter-render --test snapshot`

#![allow(clippy::unwrap_used)]

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

use meter_core::UsageStatus;
use meter_render::{IconState, IconStyle, RenderedIcon, Scale, render_icon};

/// Max Hamming distance (out of 64 bits, per hash) still considered the same
/// image. Anti-aliasing drift flips edge cells; layout changes flip many.
const TOLERANCE: u32 = 6;

/// Grid edge for both perceptual hashes (dhash samples GRID+1 columns).
const GRID: u32 = 8;

/// Dead zone (in 0–255 gray levels) for hash comparisons. Flat regions make
/// adjacent cells near-identical, where the bit would otherwise be a coin
/// flip decided by anti-aliasing noise; comparisons inside the dead zone
/// resolve to 0 deterministically.
const DEAD_ZONE: f64 = 3.0;

fn cases() -> Vec<(&'static str, IconState)> {
    let state = |percent, status, at_risk, mono, scale| IconState {
        style: IconStyle::Battery,
        percent,
        status,
        at_risk,
        mono,
        scale,
    };
    vec![
        (
            "battery_000_safe",
            state(0, UsageStatus::Safe, false, false, Scale::X1),
        ),
        (
            "battery_035_safe",
            state(35, UsageStatus::Safe, false, false, Scale::X1),
        ),
        (
            "battery_035_safe_2x",
            state(35, UsageStatus::Safe, false, false, Scale::X2),
        ),
        (
            "battery_065_warning",
            state(65, UsageStatus::Warning, false, false, Scale::X1),
        ),
        (
            "battery_065_warning_at_risk",
            state(65, UsageStatus::Warning, true, false, Scale::X1),
        ),
        (
            "battery_092_critical",
            state(92, UsageStatus::Critical, false, false, Scale::X1),
        ),
        (
            "battery_092_critical_mono",
            state(92, UsageStatus::Critical, false, true, Scale::X1),
        ),
        (
            "battery_100_critical_at_risk_2x",
            state(100, UsageStatus::Critical, true, false, Scale::X2),
        ),
        (
            "battery_050_warning_mono_at_risk",
            state(50, UsageStatus::Warning, true, true, Scale::X1),
        ),
    ]
}

#[test]
fn rendered_icons_match_snapshots() {
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut failures = Vec::new();

    for (name, state) in cases() {
        let icon = render_icon(&state).unwrap();
        let path = snapshot_path(name);

        if update {
            write_png(&path, &icon);
            continue;
        }

        let Some(expected) = read_png(&path) else {
            failures.push(format!(
                "{name}: missing snapshot {path:?} — run with UPDATE_SNAPSHOTS=1"
            ));
            continue;
        };

        let (d, a) = (
            hamming(dhash(&icon), dhash(&expected)),
            hamming(ahash(&icon), ahash(&expected)),
        );
        if d > TOLERANCE || a > TOLERANCE {
            failures.push(format!(
                "{name}: perceptual drift too large (dhash {d}, ahash {a}, tolerance {TOLERANCE})"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "snapshot mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn distinct_states_hash_differently() {
    // Guard against a degenerate hash that would wave everything through:
    // materially different icons must sit well outside the tolerance.
    let render = |percent, status, mono| {
        render_icon(&IconState {
            style: IconStyle::Battery,
            percent,
            status,
            at_risk: false,
            mono,
            scale: Scale::X1,
        })
        .unwrap()
    };
    let empty = render(0, UsageStatus::Safe, false);
    let full = render(100, UsageStatus::Critical, false);
    let d = hamming(dhash(&empty), dhash(&full));
    let a = hamming(ahash(&empty), ahash(&full));
    assert!(d > TOLERANCE && a > TOLERANCE, "dhash {d}, ahash {a}");
}

#[test]
fn scales_are_perceptually_equivalent() {
    // The same state at 1x and 2x is one template rasterized twice; the
    // perceptual hashes are resolution-independent, so they must agree.
    let at = |scale| {
        render_icon(&IconState {
            style: IconStyle::Battery,
            percent: 65,
            status: UsageStatus::Warning,
            at_risk: true,
            mono: false,
            scale,
        })
        .unwrap()
    };
    let (x1, x2) = (at(Scale::X1), at(Scale::X2));
    let d = hamming(dhash(&x1), dhash(&x2));
    let a = hamming(ahash(&x1), ahash(&x2));
    assert!(d <= TOLERANCE && a <= TOLERANCE, "dhash {d}, ahash {a}");
}

// --- perceptual hashing ----------------------------------------------------

/// Downsample to a small grayscale grid with a box filter (pixels weighted by
/// their fractional overlap with each cell, so the grid is scale-invariant),
/// compositing the straight-alpha RGBA over white so alpha-only mono art
/// keeps a shape.
fn gray_grid(icon: &RenderedIcon, cols: u32, rows: u32) -> Vec<f64> {
    let (w, h) = (icon.width, icon.height);
    let gray: Vec<f64> = icon
        .rgba
        .chunks_exact(4)
        .map(|px| {
            let alpha = f64::from(px[3]) / 255.0;
            let lum = f64::from(px[0]).mul_add(
                0.299,
                f64::from(px[1]).mul_add(0.587, 0.114 * f64::from(px[2])),
            );
            lum.mul_add(alpha, 255.0 * (1.0 - alpha))
        })
        .collect();

    let overlap = |px: u32, lo: f64, hi: f64| -> f64 {
        (f64::from(px) + 1.0).min(hi) - f64::from(px).max(lo)
    };

    let mut grid = Vec::with_capacity((cols * rows) as usize);
    for gy in 0..rows {
        let (ya, yb) = cell_bounds(gy, rows, h);
        for gx in 0..cols {
            let (xa, xb) = cell_bounds(gx, cols, w);
            let mut sum = 0.0;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            for y in (ya.floor() as u32)..(yb.ceil() as u32).min(h) {
                let wy = overlap(y, ya, yb);
                for x in (xa.floor() as u32)..(xb.ceil() as u32).min(w) {
                    sum = (gray[(y * w + x) as usize] * wy).mul_add(overlap(x, xa, xb), sum);
                }
            }
            grid.push(sum / ((xb - xa) * (yb - ya)));
        }
    }
    grid
}

fn cell_bounds(cell: u32, cells: u32, extent: u32) -> (f64, f64) {
    let step = f64::from(extent) / f64::from(cells);
    (f64::from(cell) * step, f64::from(cell + 1) * step)
}

/// Difference hash: each bit compares horizontally adjacent grid cells.
fn dhash(icon: &RenderedIcon) -> u64 {
    let grid = gray_grid(icon, GRID + 1, GRID);
    let mut hash = 0_u64;
    for row in 0..GRID as usize {
        for col in 0..GRID as usize {
            let i = row * (GRID as usize + 1) + col;
            hash = (hash << 1) | u64::from(grid[i] - grid[i + 1] > DEAD_ZONE);
        }
    }
    hash
}

/// Average hash: each bit compares a grid cell against the global mean.
fn ahash(icon: &RenderedIcon) -> u64 {
    let grid = gray_grid(icon, GRID, GRID);
    let mean = grid.iter().sum::<f64>() / f64::from(GRID * GRID);
    grid.iter().fold(0_u64, |hash, &v| {
        (hash << 1) | u64::from(v - mean > DEAD_ZONE)
    })
}

const fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

// --- snapshot I/O ----------------------------------------------------------

fn snapshot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.png"))
}

fn read_png(path: &PathBuf) -> Option<RenderedIcon> {
    let decoder = png::Decoder::new(BufReader::new(File::open(path).ok()?));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    assert_eq!(info.color_type, png::ColorType::Rgba, "snapshots are RGBA");
    buf.truncate(info.buffer_size());
    Some(RenderedIcon {
        width: info.width,
        height: info.height,
        rgba: buf,
        is_template: false,
    })
}

fn write_png(path: &PathBuf, icon: &RenderedIcon) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, icon.width, icon.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&icon.rgba).unwrap();
}
