//! Transition packages: `.glsl` files in `NOX_TRANSITIONS_DIR`, drawn from
//! both clips' pictures on the GPU. A test binary of its own, as the
//! packages are read once per process.

mod common;

use common::{FPS, clip, f, solid};
use nox_engine::{Engine, EngineEvent, FrameImage, RenderedFrame};
use nox_project::{Edge, ItemRef, Parameters, Timeline};
use std::sync::{Arc, mpsc};
use std::time::Duration;

fn pixel(frame: &RenderedFrame, x: u32, y: u32) -> [u8; 3] {
    let FrameImage::Pixels(image) = &frame.image else {
        panic!("expected pixels");
    };
    let i = y as usize * image.stride + x as usize * 4;
    [image.data[i], image.data[i + 1], image.data[i + 2]]
}

fn close(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 3)
}

fn render(engine: &Engine, events: &mpsc::Receiver<EngineEvent>, frame: i64) -> Arc<RenderedFrame> {
    if let Some(cached) = engine.request(frame).filter(|f| f.frame == frame) {
        return cached;
    }
    loop {
        match events
            .recv_timeout(Duration::from_secs(20))
            .expect("frame in time")
        {
            EngineEvent::Frame(rendered) if rendered.frame == frame && rendered.exact => {
                return rendered;
            }
            EngineEvent::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

const RED: [u8; 3] = [0xC0, 0x30, 0x20];
const BLUE: [u8; 3] = [0x20, 0x60, 0xE0];
const GREEN: [u8; 3] = [0x30, 0xA0, 0x40];

const TOP: &str = "// Name: Top First
// Author: Nox
// License: MIT
// Description: The incoming picture in the top half

vec4 transition(vec2 uv) {
  return uv.y > 0.5 ? getToColor(uv) : getFromColor(uv);
}
";

const LEFT: &str = "// Name: Left First
// Author: Nox
// License: MIT
// Description: The incoming picture left of a split
// Category: Wipes

// @range(0, 1) Split
uniform float split; // = 0.5

vec4 transition(vec2 uv) {
  return uv.x < split ? getToColor(uv) : getFromColor(uv);
}
";

const CLEAR: &str = "// Name: Clear
// Author: Nox
// License: MIT
// Description: Nothing, so what's below shows

vec4 transition(vec2 uv) {
  return vec4(0.0);
}
";

const BROKEN: &str = "// Name: Broken
vec4 transition(vec2 uv) {
  return nothing;
}
";

/// Frames of a red clip, then a blue one, with transition `id` (with
/// `params`) from frame 10 to 30, over a green clip on the track below.
fn red_to_blue(
    id: &str,
    params: &[(&str, nox_project::Value)],
    frames: &[i64],
) -> Vec<Arc<RenderedFrame>> {
    let red = solid("red-package.mkv", "0xC03020", "320x180").expect("fixture");
    let blue = solid("blue-package.mkv", "0x2060E0", "320x180").expect("fixture");
    let green = solid("green-package.mkv", "0x30A040", "320x180").expect("fixture");
    let mut timeline = Timeline::new("t", FPS);
    timeline.place(0, clip(&green, 0, 40), f(0)).unwrap();
    let top = timeline.add_track(nox_project::TrackKind::Video);
    timeline.place(top, clip(&red, 20, 20), f(0)).unwrap();
    timeline.place(top, clip(&blue, 20, 20), f(20)).unwrap();
    let def = nox_fx::transitions::find_transition(id).expect("transition");
    let mut like = nox_fx::transitions::new_transition(def);
    for (name, value) in params {
        like.set_param(name, Some(value.clone()));
    }
    timeline
        .add_transition(
            ItemRef {
                track: top,
                index: 0,
            },
            Edge::End,
            f(20),
            &like,
        )
        .unwrap();
    let (sender, events) = mpsc::channel();
    let _slot = common::engine_slot();
    let engine = Engine::new(Vec::new(), move |event| {
        let _ = sender.send(event);
    });
    engine.set_size(160, 90);
    engine.set_timeline(Arc::new(timeline));
    frames
        .iter()
        .map(|&n| render(&engine, &events, n))
        .collect()
}

#[test]
fn packages_draw_transitions() {
    if !common::have_gpu() {
        eprintln!("skipping: needs a GPU");
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("transition-packages");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, source) in [
        ("top", TOP),
        ("left", LEFT),
        ("clear", CLEAR),
        ("broken", BROKEN),
    ] {
        std::fs::write(dir.join(format!("{name}.glsl")), source).unwrap();
    }
    // SAFETY: set before any thread reads it (the registry loads below).
    unsafe { std::env::set_var("NOX_TRANSITIONS_DIR", &dir) };

    // Loaded: the working ones listed (by category), the broken one a
    // problem.
    let left = nox_fx::transitions::find_transition("user.left").expect("listed");
    assert_eq!(left.label, "Left First");
    assert_eq!(left.category, nox_fx::transitions::TransitionCategory::Wipe);
    assert!(nox_fx::transitions::find_transition("user.top").is_some());
    assert!(nox_fx::transitions::find_transition("user.broken").is_none());
    let problems = nox_fx::shader_transitions::package_problems();
    assert_eq!(problems.len(), 1);
    assert!(problems[0].path.ends_with("broken.glsl"));

    // GL's uv: y from the bottom, so the top half is the incoming clip.
    let frames = red_to_blue("user.top", &[], &[20]);
    assert!(
        close(pixel(&frames[0], 80, 20), BLUE),
        "{:?}",
        pixel(&frames[0], 80, 20)
    );
    assert!(
        close(pixel(&frames[0], 80, 70), RED),
        "{:?}",
        pixel(&frames[0], 80, 70)
    );

    // Parameters reach the shader.
    let frames = red_to_blue("user.left", &[("split", 0.25.into())], &[20]);
    assert!(
        close(pixel(&frames[0], 30, 45), BLUE),
        "{:?}",
        pixel(&frames[0], 30, 45)
    );
    assert!(
        close(pixel(&frames[0], 60, 45), RED),
        "{:?}",
        pixel(&frames[0], 60, 45)
    );

    // Where the transition is transparent, the track below shows; before
    // and after it, the clips as they are.
    let frames = red_to_blue("user.clear", &[], &[20, 5, 35]);
    assert!(
        close(pixel(&frames[0], 80, 45), GREEN),
        "{:?}",
        pixel(&frames[0], 80, 45)
    );
    assert!(close(pixel(&frames[1], 80, 45), RED));
    assert!(close(pixel(&frames[2], 80, 45), BLUE));
}
