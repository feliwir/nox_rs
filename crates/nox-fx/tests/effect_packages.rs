//! Effect packages: `.glsl` files in `NOX_EFFECTS_DIR`, listed and run like
//! Nox's own effects. A test binary of its own, as packages are read once
//! per process. Skipped without a Vulkan device unless `NOX_REQUIRE_GPU`
//! is set.

use nox_fx::{Category, Context, Effects, ParamKind, ParamValue, Unit};
use nox_gpu::compute::{Buffer, Image, MemoryUse, Recording};
use nox_gpu::{LayerHook, LayerPicture};
use nox_project::{Effect, Item, Parameters};

/// Display colors in, display colors out: inverting them.
const INVERT: &str = "// Name: Invert
// Author: Nox
// License: MIT
// Description: Inverts the colors

// @range(0, 100, %) Amount
uniform float amount; // = 100.0

vec4 effect(vec2 uv) {
  vec4 c = getColor(uv);
  return vec4(mix(c.rgb, 1.0 - c.rgb, amount / 100.0), c.a);
}
";

/// Two passes: the first doubles the input, the second halves what the
/// first returned and adds `offset` (linear light).
const TWO_PASSES: &str = "// Name: Two Passes
// Author: Nox
// License: MIT
// Description: Doubles, then halves and offsets
// Category: Blur and Sharpen
// Passes: twice, halve
// Color: linear

// @range(-1, 1) Offset
uniform float offset; // = 0.25

vec4 twice(vec2 uv) {
  return getColorAt(pixel) * 2.0;
}

vec4 halve(vec2 uv) {
  vec4 c = getPassAt(pixel) / 2.0;
  return vec4(c.rgb + offset, getColorAt(pixel).a);
}
";

const BROKEN: &str = "// Name: Broken
vec4 effect(vec2 uv) {
  return nothing;
}
";

/// `pixels` (`width` wide) through `item`'s effects, if there is a GPU.
fn run(item: &Item, pixels: &[[f32; 4]], width: u32) -> Option<Vec<[f32; 4]>> {
    let gpu = match nox_media::Accel::new() {
        Ok(accel) => accel.gpu().clone(),
        Err(err) if std::env::var_os("NOX_REQUIRE_GPU").is_some() => {
            panic!("no Vulkan GPU ({err}), but NOX_REQUIRE_GPU is set")
        }
        Err(_) => return None,
    };
    let mut effects = Effects::new(gpu.clone()).unwrap();
    let height = pixels.len() as u32 / width;
    let image = Image::new(&gpu, width, height).unwrap();
    let bytes = (pixels.len() * 16) as u64;
    let mut upload = Buffer::new(&gpu, bytes, MemoryUse::Upload).unwrap();
    upload.write(pixels.as_flattened()).unwrap();
    let mut copy = Recording::new(&gpu, 0, 0).unwrap();
    copy.copy_pixels(&upload, image.handle(), width, height, false);
    copy.run().unwrap();
    let picture = LayerPicture {
        image: image.handle(),
        view: image.view(),
        width,
        height,
    };
    let context = Context {
        clip: 1,
        frame: None,
    };
    effects
        .prepare(nox_fx::resolve(item, 0.0), context)
        .process(&picture)
        .unwrap();
    let download = Buffer::new(&gpu, bytes, MemoryUse::Download).unwrap();
    let mut copy = Recording::new(&gpu, 0, 0).unwrap();
    copy.copy_pixels(&download, image.handle(), width, height, true);
    copy.run().unwrap();
    let floats: Vec<f32> = download.read().unwrap();
    Some(floats[..pixels.len() * 4].as_chunks::<4>().0.to_vec())
}

#[test]
fn packages_are_listed_and_run() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("effect-packages");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, source) in [
        ("invert", INVERT),
        ("two_passes", TWO_PASSES),
        ("broken", BROKEN),
    ] {
        std::fs::write(dir.join(format!("{name}.glsl")), source).unwrap();
    }
    // SAFETY: set before any thread reads it (the registry loads below).
    unsafe { std::env::set_var("NOX_EFFECTS_DIR", &dir) };

    // Listed with their parameters and categories; the broken one is a
    // problem.
    let invert = nox_fx::find("user.invert").expect("listed");
    assert_eq!(
        (invert.label, invert.category),
        ("Invert", Category::Stylize)
    );
    assert!(invert.in_stack());
    assert!(matches!(
        invert.params[0].kind,
        ParamKind::Number {
            unit: Unit::Percent,
            default: 100.0,
            ..
        }
    ));
    let two = nox_fx::find("user.two_passes").expect("listed");
    assert_eq!(two.category, Category::Generic);
    assert!(nox_fx::find("user.broken").is_none());
    let problems = nox_fx::shader_effects::package_problems();
    assert_eq!(problems.len(), 1);
    assert!(problems[0].path.ends_with("broken.glsl"));
    assert!(
        problems[0].message.contains("'nothing'"),
        "{}",
        problems[0].message
    );

    // Display colors: mid gray (0.5 encoded, 0.214 linear) inverted stays
    // mid gray; black becomes white.
    let mut item = Item::new("clip");
    item.add_effect(Effect::rendering("user.invert", "Invert"));
    let pixels = [[0.0, 0.0, 0.0, 1.0], [0.214, 0.214, 0.214, 1.0]];
    let Some(out) = run(&item, &pixels, 2) else {
        return;
    };
    assert!(
        out[0][..3].iter().all(|&c| (c - 1.0).abs() < 1e-3),
        "{:?}",
        out[0]
    );
    assert!(
        out[1][..3].iter().all(|&c| (c - 0.214).abs() < 2e-3),
        "{:?}",
        out[1]
    );

    // Passes in order, a parameter reaching the last one.
    let mut item = Item::new("clip");
    let key = item.add_effect(Effect::rendering("user.two_passes", "Two"));
    item.effect_mut(&key)
        .unwrap()
        .set_param("offset", Some(ParamValue::Number(0.5).to_value()));
    let out = run(&item, &[[0.1, 0.2, 0.3, 0.5]], 1).unwrap();
    let expected = [0.6, 0.7, 0.8, 0.5];
    for (got, want) in out[0].iter().zip(expected) {
        assert!((got - want).abs() < 1e-5, "{:?}", out[0]);
    }
}
