//! Effects written in GLSL: Nox's own picture effects (`effects/*.glsl`)
//! and the user's packages, `.glsl` files in `nox/effects` in the data
//! folder (`NOX_EFFECTS_DIR` overrides). The header and uniforms are as
//! in `glsl`, plus:
//!
//! ```glsl
//! // Category: Blur and Sharpen     (Blur and Sharpen, Keying, Stylize; default Stylize)
//! // Passes: horizontal, vertical   (default one pass, `effect`; at most four)
//! // Color: linear                  (default display: sRGB-encoded BT.709)
//! // Outputs: picture, matte        (also a matte: the result's alpha)
//!
//! vec4 horizontal(vec2 uv) { … getColorAt(pixel + ivec2(i, 0)) … }
//! vec4 vertical(vec2 uv) { … getPassAt(pixel + ivec2(0, i)) … }
//! ```
//!
//! Each pass is a function run for every pixel of the result, `uv` (0–1)
//! from the top left. It has `pixel` (an `ivec2`), `resolution` and `time`
//! (media seconds), and reads `getColor(uv)` / `getColorAt(pixel)` (the
//! effect's input, straight alpha, in the file's colors; bilinear / exact,
//! clamped to the edges) and `getPass(uv)` / `getPassAt(pixel)` (what the
//! pass before returned, as it returned it). The last pass returns the
//! result, straight alpha. `workingFromDisplay(rgb)` and
//! `displayFromWorking(rgb)` convert colors (e.g. a color parameter, which
//! is as picked, for a linear effect). Every effect can be masked.

use nox_gpu::compute::{BindingKind, Kernel};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use crate::glsl::Problem;
use crate::glsl::{GlslSource, MAX_PARAMS};
use crate::registry::{ParamDef, ParamValue};

/// The most passes a file can have.
const MAX_PASSES: usize = 4;

/// Push constants of the template.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Push {
    size: [i32; 2],
    time: f32,
    pad: f32,
    params: [f32; MAX_PARAMS],
}

/// An effect in GLSL, parsed.
#[derive(Debug)]
pub struct ShaderEffect {
    pub name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    /// From the header's `Category`, if any.
    pub category: Option<String>,
    /// The parameters, as the registry has them.
    pub params: Vec<ParamDef>,
    /// The passes' functions, in order.
    pub passes: Vec<String>,
    /// Works in the linear working space, not on display colors.
    pub linear: bool,
    /// Also outputs a matte (the result's alpha).
    pub matte: bool,
    /// Reads `time` (so a still with it changes over time).
    pub uses_time: bool,
    source: GlslSource,
}

impl ShaderEffect {
    /// Parses `source` (see the module's description).
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = GlslSource::parse(source)?;
        let passes: Vec<String> = match source.header("Passes") {
            Some(list) => list
                .split(',')
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect(),
            None => vec!["effect".to_owned()],
        };
        if passes.is_empty() || passes.len() > MAX_PASSES {
            return Err(format!("1 to {MAX_PASSES} passes, not {}", passes.len()));
        }
        for pass in &passes {
            if !source.body.contains(&format!("{pass}(")) {
                return Err(format!("no function {pass}(vec2 uv) for its pass"));
            }
        }
        let linear = match source.header("Color") {
            None | Some("display") => false,
            Some("linear") => true,
            Some(other) => return Err(format!("Color: {other} (linear or display)")),
        };
        let matte = match source.header("Outputs") {
            None | Some("picture") => false,
            Some(outputs) => {
                let outputs: Vec<&str> = outputs.split(',').map(str::trim).collect();
                match outputs.as_slice() {
                    ["picture", "matte"] | ["matte", "picture"] => true,
                    _ => return Err("Outputs: picture, or picture, matte".into()),
                }
            }
        };
        Ok(Self {
            name: source.get("Name"),
            description: source.get("Description"),
            author: source.get("Author"),
            license: source.get("License"),
            category: source.header("Category").map(str::to_owned),
            params: source.params.clone(),
            uses_time: source
                .body
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "time"),
            passes,
            linear,
            matte,
            source,
        })
    }

    /// The compute shader running pass `pass`.
    pub fn template(&self, pass: usize) -> String {
        let (globals, assignments) = self.source.globals("nox_p");
        let last = pass + 1 == self.passes.len();
        // Display effects see encoded colors and return them.
        let (read, write) = if self.linear {
            ("", "")
        } else {
            (
                "c.rgb = nox_encode(c.rgb);",
                "nox_c.rgb = nox_decode(nox_c.rgb);",
            )
        };
        format!(
            r#"#version 450
layout(local_size_x = 16, local_size_y = 16) in;
layout(set = 0, binding = 0, rgba32f) readonly uniform image2D nox_input;
layout(set = 0, binding = 1, rgba32f) readonly uniform image2D nox_previous;
layout(set = 0, binding = 2, rgba32f) writeonly uniform image2D nox_output;
layout(push_constant) uniform NoxPush {{
    ivec2 nox_size;
    float nox_time;
    float nox_pad;
    float nox_params[{MAX_PARAMS}];
}} nox_p;

{colors}
vec3 workingFromDisplay(vec3 rgb) {{ return nox_decode(rgb); }}
vec3 displayFromWorking(vec3 rgb) {{ return nox_encode(rgb); }}
ivec2 pixel;
vec2 resolution;
float time;
vec4 getColorAt(ivec2 at) {{
    vec4 c = imageLoad(nox_input, clamp(at, ivec2(0), nox_p.nox_size - 1));
    {read}
    return c;
}}
vec4 getPassAt(ivec2 at) {{
    return imageLoad(nox_previous, clamp(at, ivec2(0), nox_p.nox_size - 1));
}}
// Bilinear at uv (0-1 from the top left); colors weighted by alpha.
vec4 getColor(vec2 uv) {{
    vec2 texel = uv * vec2(nox_p.nox_size) - 0.5;
    ivec2 base = ivec2(floor(texel));
    vec2 f = texel - vec2(base);
    vec4 a = getColorAt(base);
    vec4 b = getColorAt(base + ivec2(1, 0));
    vec4 c = getColorAt(base + ivec2(0, 1));
    vec4 d = getColorAt(base + ivec2(1, 1));
    vec4 m = mix(mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), f.x),
                 mix(vec4(c.rgb * c.a, c.a), vec4(d.rgb * d.a, d.a), f.x), f.y);
    return m.a > 0.0 ? vec4(m.rgb / m.a, m.a) : vec4(0.0);
}}
vec4 getPass(vec2 uv) {{
    vec2 texel = uv * vec2(nox_p.nox_size) - 0.5;
    ivec2 base = ivec2(floor(texel));
    vec2 f = texel - vec2(base);
    return mix(mix(getPassAt(base), getPassAt(base + ivec2(1, 0)), f.x),
               mix(getPassAt(base + ivec2(0, 1)), getPassAt(base + ivec2(1, 1)), f.x), f.y);
}}
{globals}#line 1
{body}
void main() {{
    ivec2 nox_pos = ivec2(gl_GlobalInvocationID.xy);
    if (any(greaterThanEqual(nox_pos, nox_p.nox_size))) {{
        return;
    }}
    pixel = nox_pos;
    resolution = vec2(nox_p.nox_size);
    time = nox_p.nox_time;
{assignments}
    vec4 nox_c = {function}((vec2(nox_pos) + 0.5) / resolution);
    {finish}
    imageStore(nox_output, nox_pos, nox_c);
}}
"#,
            colors = crate::glsl::color_functions(),
            body = self.source.body,
            function = self.passes[pass],
            finish = if last { write } else { "" },
        )
    }

    /// Every pass compiled to SPIR-V (glslang).
    pub fn compile(&self) -> Result<Vec<Vec<u32>>, String> {
        (0..self.passes.len())
            .map(|pass| {
                crate::glsl::compile(self.template(pass))
                    .map_err(|e| format!("pass {}: {e}", self.passes[pass]))
            })
            .collect()
    }

    /// The push constants for `values` (of `params`) at media time `time`,
    /// over a picture of `size`.
    pub(crate) fn push(&self, values: &[ParamValue], size: (u32, u32), time: f64) -> Vec<u8> {
        bytemuck::bytes_of(&Push {
            size: [size.0 as i32, size.1 as i32],
            time: time as f32,
            pad: 0.0,
            params: self.source.pack(values, size.1),
        })
        .to_vec()
    }

    /// The passes as kernels on `gpu`: input, previous pass and output
    /// images.
    pub(crate) fn kernels(
        &self,
        gpu: &std::sync::Arc<nox_gpu::GpuDevice>,
    ) -> Result<Vec<Kernel>, String> {
        self.compile()?
            .into_iter()
            .map(|words| {
                Kernel::new(
                    gpu,
                    bytemuck::cast_slice(&words),
                    &[BindingKind::Image; 3],
                    size_of::<Push>(),
                )
                .map_err(|e| e.to_string())
            })
            .collect()
    }
}

/// Nox's own, in `effects/` (id, source).
pub(crate) const BUILT_IN: &[(&str, &str)] = &[
    (
        crate::registry::GAUSSIAN_BLUR,
        include_str!("../effects/gaussian_blur.glsl"),
    ),
    (
        crate::registry::SHARPEN,
        include_str!("../effects/sharpen.glsl"),
    ),
    (
        crate::registry::PAINTERLY,
        include_str!("../effects/painterly.glsl"),
    ),
    (
        crate::registry::CHROMA_KEY,
        include_str!("../effects/chroma_key.glsl"),
    ),
];

/// Where users put effect packages.
pub fn packages_dir() -> PathBuf {
    match std::env::var_os("NOX_EFFECTS_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => nox_ai::models::data_dir().join("effects"),
    }
}

/// The effect packages in `dir` (parsed and compiled) and problems.
pub(crate) fn load_packages(dir: &Path) -> (Vec<(String, ShaderEffect)>, Vec<Problem>) {
    crate::glsl::load_packages(dir, "effect", |source| {
        let parsed = ShaderEffect::parse(source)?;
        parsed.compile()?;
        Ok(parsed)
    })
}

/// Effect packages that failed to load, and why (`effects()` loads them).
pub fn package_problems() -> &'static [Problem] {
    crate::registry::effects();
    PROBLEMS.get().map_or(&[], Vec::as_slice)
}

pub(crate) static PROBLEMS: OnceLock<Vec<Problem>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_passes_colors_and_outputs() {
        let source = "// Name: Two\n// Passes: first, second\n// Color: linear\n// Outputs: picture, matte\n\
            vec4 first(vec2 uv) { return getColor(uv); }\nvec4 second(vec2 uv) { return getPassAt(pixel) * time; }\n";
        let effect = ShaderEffect::parse(source).unwrap();
        assert_eq!(effect.passes, ["first", "second"]);
        assert!(effect.linear && effect.matte && effect.uses_time);
        assert_eq!(effect.compile().unwrap().len(), 2);
        // The default: one pass, display colors, a picture.
        let plain = ShaderEffect::parse("vec4 effect(vec2 uv) { return getColor(uv); }").unwrap();
        assert_eq!(plain.passes, ["effect"]);
        assert!(!plain.linear && !plain.matte && !plain.uses_time);
        plain.compile().unwrap();
        for (bad, why) in [
            (
                "vec4 other(vec2 uv) { return vec4(0.0); }",
                "function effect",
            ),
            (
                "// Color: log\nvec4 effect(vec2 uv) { return vec4(0.0); }",
                "Color",
            ),
            (
                "// Outputs: matte\nvec4 effect(vec2 uv) { return vec4(0.0); }",
                "Outputs",
            ),
            (
                "// Passes: a, b, c, d, e\nvec4 effect(vec2 uv) { return vec4(0.0); }",
                "passes",
            ),
        ] {
            let err = ShaderEffect::parse(bad).unwrap_err();
            assert!(err.contains(why), "{bad}: {err}");
        }
    }

    /// Every built-in effect parses, follows the file layout and compiles.
    #[test]
    fn built_in_effects_compile() {
        for (id, source) in BUILT_IN {
            let effect = ShaderEffect::parse(source).unwrap_or_else(|e| panic!("{id}: {e}"));
            for key in [
                "// Name:",
                "// Author:",
                "// License:",
                "// Description:",
                "// Category:",
            ] {
                assert!(source.contains(key), "{id}: {key}");
            }
            assert!(effect.linear, "{id} works in linear light");
            effect.compile().unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
}
