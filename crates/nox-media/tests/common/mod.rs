//! Test clips generated with the `ffmpeg` CLI.

// Not every test binary uses every helper.
#![allow(dead_code)]

use nox_media::Accel;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

/// Generates `name` in the test temp dir with the given ffmpeg arguments, or
/// returns `None` (after printing why) if ffmpeg or an encoder is missing.
pub fn fixture(name: &str, args: &[&str]) -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join(name);
    // Written under a name of its own, then renamed: tests sharing a
    // fixture (they run in parallel) never read one being written.
    let thread = format!("{:?}", std::thread::current().id());
    let digits: String = thread.chars().filter(char::is_ascii_digit).collect();
    let partial = dir.join(format!("partial-{}-{digits}-{name}", std::process::id()));
    let output = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .arg(&partial)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            std::fs::rename(&partial, &path).expect("fixture in place");
            Some(path)
        }
        // FFmpeg without that encoder (e.g. builds without libwebp).
        Ok(output)
            if ["Unknown encoder", "Encoder not found"]
                .iter()
                .any(|m| String::from_utf8_lossy(&output.stderr).contains(m)) =>
        {
            eprintln!("skipping: encoder not available for {name}");
            None
        }
        Ok(output) => panic!(
            "ffmpeg failed to generate {name}: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(err) => {
            eprintln!("skipping: ffmpeg not available ({err})");
            None
        }
    }
}

/// One shared GPU for all tests of a binary (device creation is slow);
/// `None` without Vulkan, which skips GPU tests unless `NOX_REQUIRE_GPU` is
/// set (CI on Linux, where lavapipe must work).
pub fn gpu() -> Option<(Accel, Arc<nox_gpu::GpuDevice>)> {
    static GPU: OnceLock<Option<(Accel, Arc<nox_gpu::GpuDevice>)>> = OnceLock::new();
    GPU.get_or_init(|| match Accel::new() {
        Ok(accel) => Some((accel.clone(), accel.gpu().clone())),
        Err(err) if std::env::var_os("NOX_REQUIRE_GPU").is_some() => {
            panic!("no Vulkan GPU ({err}), but NOX_REQUIRE_GPU is set")
        }
        Err(err) => {
            eprintln!("skipping GPU tests: {err}");
            None
        }
    })
    .clone()
}

/// A PGS event: its start and end (seconds) and its block (`x`, `y`,
/// width, height).
pub type PgsEvent = (f64, f64, (u16, u16, u16, u16));

/// Writes a PGS (`.sup`) file of white blocks on a `canvas`-sized screen:
/// each event's `(x, y, width, height)` from its start to its end
/// (seconds). Widths up to 63.
pub fn write_pgs(path: &Path, canvas: (u16, u16), events: &[PgsEvent]) {
    /// A segment: magic, times (90 kHz), type, payload.
    fn segment(out: &mut Vec<u8>, pts: u32, kind: u8, payload: &[u8]) {
        out.extend_from_slice(b"PG");
        out.extend_from_slice(&pts.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.push(kind);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
    }
    let mut out = Vec::new();
    let mut number = 0u16;
    for &(start, end, block) in events {
        for (seconds, shown) in [(start, Some(block)), (end, None)] {
            let pts = (seconds * 90_000.0) as u32;
            // Presentation: the screen, an epoch start showing the object
            // (or a normal one clearing it).
            let mut pcs = Vec::new();
            pcs.extend_from_slice(&canvas.0.to_be_bytes());
            pcs.extend_from_slice(&canvas.1.to_be_bytes());
            pcs.push(0x10);
            pcs.extend_from_slice(&number.to_be_bytes());
            number += 1;
            pcs.push(if shown.is_some() { 0x80 } else { 0x00 });
            pcs.extend_from_slice(&[0, 0]);
            let (x, y, w, h) = shown.unwrap_or(block);
            pcs.push(u8::from(shown.is_some()));
            if shown.is_some() {
                pcs.extend_from_slice(&[0, 0, 0, 0]);
                pcs.extend_from_slice(&x.to_be_bytes());
                pcs.extend_from_slice(&y.to_be_bytes());
            }
            segment(&mut out, pts, 0x16, &pcs);
            let mut wds = vec![1, 0];
            for v in [x, y, w, h] {
                wds.extend_from_slice(&v.to_be_bytes());
            }
            segment(&mut out, pts, 0x17, &wds);
            if shown.is_some() {
                // Palette: entry 1 white (Y Cr Cb A).
                segment(&mut out, pts, 0x14, &[0, 0, 1, 235, 128, 128, 255]);
                // Each line: `w` pixels of color 1, then its end.
                let mut rle = Vec::new();
                for _ in 0..h {
                    rle.extend_from_slice(&[0, 0x80 | w as u8, 1, 0, 0]);
                }
                let mut ods = vec![0, 0, 0, 0xc0];
                ods.extend_from_slice(&(rle.len() as u32 + 4).to_be_bytes()[1..]);
                ods.extend_from_slice(&w.to_be_bytes());
                ods.extend_from_slice(&h.to_be_bytes());
                ods.extend_from_slice(&rle);
                segment(&mut out, pts, 0x15, &ods);
            }
            segment(&mut out, pts, 0x80, &[]);
        }
    }
    std::fs::write(path, out).expect("PGS written");
}
