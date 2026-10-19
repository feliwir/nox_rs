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
