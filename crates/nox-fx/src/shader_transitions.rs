//! Transitions that need both pictures at once, written as gl-transitions
//! GLSL: a function `vec4 transition(vec2 uv)` that reads the two pictures
//! with `getFromColor(uv)` and `getToColor(uv)` (`uv` from the bottom left,
//! as in GL), with `progress` (0–1), `ratio` (width / height) and uniforms.
//! Nox's own come from `transitions/*.glsl`; users add theirs as packages,
//! `.glsl` files in `nox/transitions` in the data folder
//! (`NOX_TRANSITIONS_DIR` overrides).
//!
//! The header and uniforms are as in `glsl` (`Category`: Dissolves,
//! Wipes, Motion or Stylize).
//!
//! The body is put into a compute shader (`template`): the pictures are
//! the compositor's transition pictures (linear working space,
//! premultiplied), handed to the body as straight sRGB-encoded BT.709, the
//! colors gl-transitions are written for; the result is drawn over the
//! output. Uniforms become push constants. It is compiled with glslang when
//! first used.

use nox_gpu::compute::{BindingKind, Kernel};
use nox_gpu::{GpuDevice, TransitionPass};
use nox_project::Transition;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub use crate::glsl::Problem;
use crate::glsl::{GlslSource, MAX_PARAMS};
use crate::registry::ParamDef;

/// Push constants of the template.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Push {
    size: [i32; 2],
    progress: f32,
    ratio: f32,
    params: [f32; MAX_PARAMS],
}

/// A transition in GLSL, parsed.
#[derive(Debug)]
pub struct ShaderTransition {
    pub name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    /// From the header's `Category`, if any.
    pub category: Option<String>,
    /// The parameters, as the registry has them.
    pub params: Vec<ParamDef>,
    source: GlslSource,
}

impl ShaderTransition {
    /// Parses `source` (see `glsl` for the format).
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = GlslSource::parse(source)?;
        if !source.body.contains("transition") {
            return Err("no transition function".into());
        }
        Ok(Self {
            name: source.get("Name"),
            description: source.get("Description"),
            author: source.get("Author"),
            license: source.get("License"),
            category: source.header("Category").map(str::to_owned),
            params: source.params.clone(),
            source,
        })
    }

    /// The compute shader around the body.
    pub fn template(&self) -> String {
        let (globals, assignments) = self.source.globals("nox_p");
        format!(
            r#"#version 450
layout(local_size_x = 16, local_size_y = 16) in;
layout(set = 0, binding = 0, rgba32f) readonly uniform image2D nox_from;
layout(set = 0, binding = 1, rgba32f) readonly uniform image2D nox_to;
layout(set = 0, binding = 2, rgba32f) uniform image2D nox_out;
layout(push_constant) uniform NoxPush {{
    ivec2 nox_size;
    float nox_progress;
    float nox_ratio;
    float nox_params[{MAX_PARAMS}];
}} nox_p;

{colors}
vec4 nox_texel(bool to, ivec2 at) {{
    at = clamp(at, ivec2(0), nox_p.nox_size - 1);
    return to ? imageLoad(nox_to, at) : imageLoad(nox_from, at);
}}
// Bilinear, at GL's uv (from the bottom left), straight and encoded.
vec4 nox_sample(bool to, vec2 uv) {{
    vec2 texel = vec2(uv.x, 1.0 - uv.y) * vec2(nox_p.nox_size) - 0.5;
    ivec2 base = ivec2(floor(texel));
    vec2 f = texel - vec2(base);
    vec4 c = mix(
        mix(nox_texel(to, base), nox_texel(to, base + ivec2(1, 0)), f.x),
        mix(nox_texel(to, base + ivec2(0, 1)), nox_texel(to, base + ivec2(1, 1)), f.x),
        f.y);
    return c.a > 0.0 ? vec4(nox_encode(c.rgb / c.a), c.a) : vec4(0.0);
}}
vec4 getFromColor(vec2 uv) {{ return nox_sample(false, uv); }}
vec4 getToColor(vec2 uv) {{ return nox_sample(true, uv); }}
float progress;
float ratio;
{globals}#line 1
{body}
void main() {{
    ivec2 nox_pos = ivec2(gl_GlobalInvocationID.xy);
    if (any(greaterThanEqual(nox_pos, nox_p.nox_size))) {{
        return;
    }}
    progress = nox_p.nox_progress;
    ratio = nox_p.nox_ratio;
{assignments}
    vec2 nox_uv = vec2((vec2(nox_pos) + 0.5) / vec2(nox_p.nox_size));
    vec4 nox_c = transition(vec2(nox_uv.x, 1.0 - nox_uv.y));
    float nox_a = clamp(nox_c.a, 0.0, 1.0);
    vec4 nox_dst = imageLoad(nox_out, nox_pos);
    imageStore(nox_out, nox_pos, vec4(mix(nox_dst.rgb, nox_decode(nox_c.rgb), nox_a), 1.0));
}}
"#,
            colors = crate::glsl::color_functions(),
            body = self.source.body,
        )
    }

    /// The template compiled to SPIR-V (glslang).
    pub fn compile(&self) -> Result<Vec<u32>, String> {
        crate::glsl::compile(self.template())
    }

    /// The push constants for `transition` (one of this kind) at eased
    /// `progress`, over an output of `size`.
    fn push(&self, transition: &Transition, progress: f64, size: (u32, u32)) -> Vec<u8> {
        let values = crate::registry::param_values(transition, &self.params, 0.0);
        bytemuck::bytes_of(&Push {
            size: [size.0 as i32, size.1 as i32],
            progress: progress as f32,
            ratio: size.0 as f32 / size.1.max(1) as f32,
            params: self.source.pack(&values, size.1),
        })
        .to_vec()
    }
}

/// The transitions in `transitions/` (id, source): Nox's own (`nox.`),
/// then the bundled gl-transitions (`gl.`).
pub(crate) const BUILT_IN: &[(&str, &str)] = &[
    ("nox.dissolve", include_str!("../transitions/dissolve.glsl")),
    ("nox.dip", include_str!("../transitions/dip.glsl")),
    (
        "nox.edge_wipe",
        include_str!("../transitions/edge_wipe.glsl"),
    ),
    ("nox.split", include_str!("../transitions/split.glsl")),
    (
        "nox.clock_wipe",
        include_str!("../transitions/clock_wipe.glsl"),
    ),
    ("nox.iris", include_str!("../transitions/iris.glsl")),
    ("nox.blinds", include_str!("../transitions/blinds.glsl")),
    (
        "nox.checkerboard",
        include_str!("../transitions/checkerboard.glsl"),
    ),
    ("nox.blocks", include_str!("../transitions/blocks.glsl")),
    ("nox.push", include_str!("../transitions/push.glsl")),
    ("nox.slide", include_str!("../transitions/slide.glsl")),
    ("nox.reveal", include_str!("../transitions/reveal.glsl")),
    ("nox.zoom", include_str!("../transitions/zoom.glsl")),
    ("nox.spin", include_str!("../transitions/spin.glsl")),
    (
        "gl.linear_blur",
        include_str!("../transitions/linear_blur.glsl"),
    ),
    (
        "gl.cross_zoom",
        include_str!("../transitions/cross_zoom.glsl"),
    ),
    ("gl.dreamy", include_str!("../transitions/dreamy.glsl")),
    ("gl.burn", include_str!("../transitions/burn.glsl")),
    (
        "gl.film_burn",
        include_str!("../transitions/film_burn.glsl"),
    ),
    ("gl.morph", include_str!("../transitions/morph.glsl")),
    ("gl.ripple", include_str!("../transitions/ripple.glsl")),
    ("gl.swirl", include_str!("../transitions/swirl.glsl")),
    (
        "gl.kaleidoscope",
        include_str!("../transitions/kaleidoscope.glsl"),
    ),
    ("gl.pixelize", include_str!("../transitions/pixelize.glsl")),
    (
        "gl.glitch_memories",
        include_str!("../transitions/glitch_memories.glsl"),
    ),
    (
        "gl.directional_warp",
        include_str!("../transitions/directional_warp.glsl"),
    ),
    (
        "gl.cross_warp",
        include_str!("../transitions/cross_warp.glsl"),
    ),
    ("gl.wind", include_str!("../transitions/wind.glsl")),
    (
        "gl.window_slice",
        include_str!("../transitions/window_slice.glsl"),
    ),
    ("gl.mosaic", include_str!("../transitions/mosaic.glsl")),
    ("gl.cube", include_str!("../transitions/cube.glsl")),
    ("gl.doorway", include_str!("../transitions/doorway.glsl")),
    (
        "gl.page_curl",
        include_str!("../transitions/page_curl.glsl"),
    ),
];

/// Where users put transition packages.
pub fn packages_dir() -> PathBuf {
    match std::env::var_os("NOX_TRANSITIONS_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => nox_ai::models::data_dir().join("transitions"),
    }
}

/// The transition packages in `dir` (parsed and compiled) and problems.
pub(crate) fn load_packages(
    dir: &std::path::Path,
) -> (Vec<(String, ShaderTransition)>, Vec<Problem>) {
    crate::glsl::load_packages(dir, "transition", |source| {
        let parsed = ShaderTransition::parse(source)?;
        parsed.compile()?;
        Ok(parsed)
    })
}

/// Transitions packages failed to load as (`transitions()` loads them).
pub fn package_problems() -> &'static [Problem] {
    crate::transitions::transitions();
    PROBLEMS.get().map_or(&[], Vec::as_slice)
}

pub(crate) static PROBLEMS: OnceLock<Vec<Problem>> = OnceLock::new();

/// Compiled shader transitions for one device, made when first used.
pub struct ShaderKernels {
    gpu: Arc<GpuDevice>,
    kernels: HashMap<&'static str, Option<Arc<Kernel>>>,
}

impl ShaderKernels {
    pub fn new(gpu: Arc<GpuDevice>) -> Self {
        Self {
            gpu,
            kernels: HashMap::new(),
        }
    }

    /// The pass drawing `transition` (a shader transition, with id `id`)
    /// at `progress` over an output of `size`; `None` if it doesn't
    /// compile (logged once).
    pub fn pass(
        &mut self,
        id: &'static str,
        shader: &ShaderTransition,
        transition: &Transition,
        progress: f64,
        size: (u32, u32),
    ) -> Option<TransitionPass> {
        let gpu = &self.gpu;
        let kernel = self
            .kernels
            .entry(id)
            .or_insert_with(|| {
                let made = shader.compile().and_then(|words| {
                    Kernel::new(
                        gpu,
                        bytemuck::cast_slice(&words),
                        &[BindingKind::Image; 3],
                        size_of::<Push>(),
                    )
                    .map_err(|e| e.to_string())
                });
                match made {
                    Ok(kernel) => Some(Arc::new(kernel)),
                    Err(err) => {
                        tracing::warn!("transition {id}: {err}");
                        None
                    }
                }
            })
            .clone()?;
        Some(TransitionPass {
            kernel,
            push: shader.push(transition, progress, size),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_a_transition_function() {
        let none = "vec4 effect(vec2 uv) { return vec4(0.0); }";
        assert!(
            ShaderTransition::parse(none)
                .unwrap_err()
                .contains("transition")
        );
    }

    /// Every bundled transition parses, follows the file layout and
    /// compiles.
    #[test]
    fn built_in_transitions_compile() {
        for (id, source) in BUILT_IN {
            let t = ShaderTransition::parse(source).unwrap_or_else(|e| panic!("{id}: {e}"));
            // gl-transitions name the file they came from.
            assert_eq!(id.starts_with("gl."), source.contains("// Source:"), "{id}");
            for key in ["// Name:", "// Author:", "// License:", "// Description:"] {
                assert!(source.contains(key), "{id}: {key}");
            }
            assert!(!t.name.is_empty() && !t.description.is_empty(), "{id}");
            t.compile().unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
}
