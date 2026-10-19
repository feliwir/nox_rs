//! What effect and transition files have in common: GLSL with a header and
//! annotated uniforms, compiled at runtime with glslang (`shader_effects`,
//! `shader_transitions` put the body into their own compute shaders).
//!
//! A file starts with a header and annotates each uniform:
//!
//! ```glsl
//! // Name: Cross Zoom
//! // Author: rectalogic
//! // License: MIT
//! // Description: A crossfade through a strong zoom blur
//! // Category: Stylize
//!
//! // @range(0, 1) Blur strength
//! uniform float strength; // = 0.4
//! // @choice(circle: Circle, star: Star) Shape   (an int: the index)
//! uniform int shape; // = 0
//! ```
//!
//! Other annotations: `@param Label`, `@color Label` (a `vec3`),
//! `@range(0, 100, %) Label` for percentages and `@range(0, 10, %h) Label`
//! for sizes in percent of the picture's height (the shader gets pixels).
//! Uniforms become globals set from push constants, so the body may shadow
//! them as in GL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::registry::{ParamDef, ParamKind, ParamValue, Unit};

/// Floats of uniforms a file can have.
pub(crate) const MAX_PARAMS: usize = 24;

/// Header keys files may have.
const HEADER_KEYS: [&str; 8] = [
    "Name",
    "Author",
    "License",
    "Description",
    "Category",
    "Passes",
    "Color",
    "Outputs",
];

/// A file, parsed.
#[derive(Debug)]
pub struct GlslSource {
    header: HashMap<String, String>,
    /// The parameters, as the registries have them.
    pub params: Vec<ParamDef>,
    uniforms: Vec<Uniform>,
    /// The source with the uniforms taken out (lines kept).
    pub body: String,
}

/// A uniform's `// @…` line.
#[derive(Debug, Default)]
struct Annotation {
    /// `@color`.
    color: bool,
    /// `@choice(value: Label, …)`: the options of an `int` (their index).
    choices: Vec<(String, String)>,
    /// `@range(min, max)`, with `%` or `%h` after them.
    range: Option<(f64, f64)>,
    unit: Unit,
    label: String,
}

impl Annotation {
    /// `rest` is what follows `@` on the line.
    fn parse(rest: &str) -> Result<Self, String> {
        let (kind, rest) = rest.split_once([' ', '(']).unwrap_or((rest, ""));
        let mut annotation = Self::default();
        let label = match kind {
            "range" | "choice" => {
                let (inside, label) = rest.split_once(')').ok_or(format!("@{kind} without )"))?;
                let items: Vec<&str> = inside.split(',').map(str::trim).collect();
                if kind == "range" {
                    let number = |i: usize| items.get(i).and_then(|n| n.parse::<f64>().ok());
                    let (Some(min), Some(max)) = (number(0), number(1)) else {
                        return Err(format!("bad @range({inside})"));
                    };
                    annotation.range = Some((min, max));
                    annotation.unit = match items.get(2) {
                        Some(&"%") => Unit::Percent,
                        Some(&"%h") => Unit::PictureHeight,
                        None => Unit::Plain,
                        Some(other) => return Err(format!("bad unit {other} in @range")),
                    };
                } else {
                    annotation.choices = items
                        .iter()
                        .map(|item| {
                            let (value, label) = item.split_once(':')?;
                            Some((value.trim().to_owned(), label.trim().to_owned()))
                        })
                        .collect::<Option<_>>()
                        .ok_or(format!("bad @choice({inside}): value: Label, …"))?;
                }
                label
            }
            "color" => {
                annotation.color = true;
                rest
            }
            _ => rest,
        };
        annotation.label = label.trim().to_owned();
        Ok(annotation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UniformType {
    Float,
    Int,
    Bool,
    /// `vec2`…`vec4` (`int`: `ivec`), as numbers.
    Vector {
        size: usize,
        int: bool,
    },
    /// `vec3`/`vec4` as a color (alpha 1).
    Color {
        size: usize,
    },
}

#[derive(Debug)]
struct Uniform {
    name: String,
    ty: UniformType,
    /// Where its floats start in `params`.
    slot: usize,
}

impl UniformType {
    fn parse(ty: &str) -> Option<Self> {
        Some(match ty {
            "float" => Self::Float,
            "int" => Self::Int,
            "bool" => Self::Bool,
            "vec2" | "vec3" | "vec4" => Self::Vector {
                size: usize::from(ty.as_bytes()[3] - b'0'),
                int: false,
            },
            "ivec2" | "ivec3" | "ivec4" => Self::Vector {
                size: usize::from(ty.as_bytes()[4] - b'0'),
                int: true,
            },
            _ => return None,
        })
    }

    /// Floats in the push constants.
    fn floats(self) -> usize {
        match self {
            Self::Float | Self::Int | Self::Bool => 1,
            Self::Vector { size, .. } | Self::Color { size } => size,
        }
    }
}

/// `fooBar`, `foo_bar` → "Foo bar".
fn label_of(name: &str) -> String {
    let mut words = String::new();
    let mut previous = '_';
    for c in name.chars() {
        if c == '_' {
            words.push(' ');
        } else {
            if c.is_uppercase() && previous.is_lowercase() {
                words.push(' ');
            }
            words.extend(c.to_lowercase());
        }
        previous = c;
    }
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Numbers in a default: `0.4`, `vec2(-1.0, 1.0)`, `ivec2(20)` (one
/// value for all), `true`.
fn parse_default(text: &str, floats: usize) -> Option<Vec<f64>> {
    let text = text.trim().trim_end_matches(';').trim();
    let inner = match text.find('(') {
        Some(open) => text[open + 1..].trim_end().strip_suffix(')')?,
        None => text,
    };
    let values: Vec<f64> = inner
        .split(',')
        .map(|v| match v.trim() {
            "true" => Some(1.0),
            "false" => Some(0.0),
            v => v.parse().ok(),
        })
        .collect::<Option<_>>()?;
    match values.len() {
        n if n == floats => Some(values),
        1 => Some(vec![values[0]; floats]),
        _ => None,
    }
}

/// A range for a number without `@range`: from 0 (or below a negative
/// default) to a few times the default.
fn range_for(default: f64) -> (f64, f64) {
    if default < 0.0 {
        (default * 3.0, -default * 3.0)
    } else {
        (0.0, (default * 3.0).max(1.0))
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

impl GlslSource {
    /// Parses `source` (see the module's description).
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut header = HashMap::new();
        let mut uniforms = Vec::new();
        let mut params = Vec::new();
        let mut body = String::new();
        // The annotation for the next uniform.
        let mut annotation: Option<Annotation> = None;
        let mut slot = 0;
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(comment) = trimmed.strip_prefix("//") {
                let comment = comment.trim();
                if let Some(rest) = comment.strip_prefix('@') {
                    annotation = Some(Annotation::parse(rest)?);
                } else if let Some((key, value)) = comment.split_once(':')
                    && HEADER_KEYS.contains(&key)
                {
                    header.insert(key.to_owned(), value.trim().to_owned());
                }
                body.push_str(line);
                body.push('\n');
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("uniform ") {
                let (declaration, default) = rest.split_once(';').ok_or("uniform without ;")?;
                let mut words = declaration.split_whitespace();
                let (Some(ty), Some(name)) = (words.next(), words.next()) else {
                    return Err(format!("bad uniform: {trimmed}"));
                };
                if ty.starts_with("sampler") {
                    return Err(format!("uniform {name}: extra images aren't supported"));
                }
                let mut ty = UniformType::parse(ty).ok_or(format!("uniform {name}: type {ty}"))?;
                let annotation = annotation.take().unwrap_or_default();
                let (range, unit) = (annotation.range, annotation.unit);
                let label = if annotation.label.is_empty() {
                    label_of(name)
                } else {
                    annotation.label.clone()
                };
                let is_color = annotation.color
                    || name.to_lowercase().contains("color")
                    || name.to_lowercase().contains("colour");
                if let UniformType::Vector {
                    size: 3 | 4,
                    int: false,
                } = ty
                    && is_color
                {
                    ty = UniformType::Color { size: ty.floats() };
                }
                let default = default
                    .trim()
                    .strip_prefix("//")
                    .and_then(|d| d.trim().strip_prefix('='))
                    .ok_or(format!("uniform {name} has no default (// = …)"))?;
                let values = parse_default(default, ty.floats())
                    .ok_or(format!("uniform {name}: bad default {default}"))?;
                let name_static = leak(name.to_owned());
                match ty {
                    UniformType::Int if !annotation.choices.is_empty() => {
                        let options: Vec<(&'static str, &'static str)> = annotation
                            .choices
                            .iter()
                            .map(|(value, label)| (leak(value.clone()), leak(label.clone())))
                            .collect();
                        let default = options
                            .get(values[0] as usize)
                            .ok_or(format!("uniform {name}: no choice {}", values[0]))?
                            .0;
                        params.push(ParamDef {
                            name: name_static,
                            label: leak(label),
                            kind: ParamKind::Choice {
                                options: Box::leak(options.into_boxed_slice()),
                                default,
                            },
                        });
                    }
                    UniformType::Bool => params.push(ParamDef {
                        name: name_static,
                        label: leak(label),
                        kind: ParamKind::Bool {
                            default: values[0] != 0.0,
                        },
                    }),
                    UniformType::Color { .. } => params.push(ParamDef {
                        name: name_static,
                        label: leak(label),
                        kind: ParamKind::Color {
                            default: [values[0], values[1], values[2]],
                        },
                    }),
                    UniformType::Float | UniformType::Int | UniformType::Vector { .. } => {
                        let int =
                            matches!(ty, UniformType::Int | UniformType::Vector { int: true, .. });
                        let axes = ["X", "Y", "Z", "W"];
                        for (i, &value) in values.iter().enumerate() {
                            let (name, label) = if values.len() == 1 {
                                (name_static, leak(label.clone()))
                            } else {
                                (
                                    leak(format!("{name}_{}", axes[i].to_lowercase())),
                                    leak(format!("{label} {}", axes[i])),
                                )
                            };
                            let (min, max) = range.unwrap_or_else(|| range_for(value));
                            // Fewer decimals the wider the range.
                            let digits = match max - min {
                                _ if int => 0,
                                span if span > 50.0 => 0,
                                span if span > 10.0 => 1,
                                _ => 2,
                            };
                            params.push(ParamDef {
                                name,
                                label,
                                kind: ParamKind::Number {
                                    min: min.min(value),
                                    max: max.max(value),
                                    default: value,
                                    digits,
                                    unit,
                                },
                            });
                        }
                    }
                }
                uniforms.push(Uniform {
                    name: name.to_owned(),
                    ty,
                    slot,
                });
                slot += ty.floats();
                if slot > MAX_PARAMS {
                    return Err(format!("more than {MAX_PARAMS} numbers of uniforms"));
                }
                // Keep the line count (compile errors point at the file).
                body.push('\n');
                continue;
            }
            annotation = None;
            body.push_str(line);
            body.push('\n');
        }
        Ok(Self {
            header,
            params,
            uniforms,
            body,
        })
    }

    /// Header value `key`, if given.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.header.get(key).map(String::as_str)
    }

    /// Header value `key`, or empty.
    pub fn get(&self, key: &str) -> String {
        self.header(key).unwrap_or_default().to_owned()
    }

    /// The uniforms as globals (declarations), and their assignments from
    /// the push constants' `params` (`push` is the push block's name), for
    /// `main` to run first.
    pub(crate) fn globals(&self, push: &str) -> (String, String) {
        let p = |slot: usize| format!("{push}.nox_params[{slot}]");
        self.uniforms
            .iter()
            .map(|u| {
                let s = u.slot;
                let (ty, value) = match u.ty {
                    UniformType::Float => ("float".to_owned(), p(s)),
                    UniformType::Int => ("int".to_owned(), format!("int({})", p(s))),
                    UniformType::Bool => ("bool".to_owned(), format!("({} != 0.0)", p(s))),
                    UniformType::Vector { size, int } => {
                        let ty = format!("{}vec{size}", if int { "i" } else { "" });
                        let parts: Vec<_> = (s..s + size).map(p).collect();
                        let value = format!("{ty}({})", parts.join(", "));
                        (ty, value)
                    }
                    UniformType::Color { size: 3 } => (
                        "vec3".to_owned(),
                        format!("vec3({}, {}, {})", p(s), p(s + 1), p(s + 2)),
                    ),
                    UniformType::Color { .. } => (
                        "vec4".to_owned(),
                        format!("vec4({}, {}, {}, 1.0)", p(s), p(s + 1), p(s + 2)),
                    ),
                };
                (
                    format!("{ty} {};\n", u.name),
                    format!("    {} = {value};\n", u.name),
                )
            })
            .unzip()
    }

    /// `values` (of `params`, in order) as the push constants' `params`;
    /// sizes in percent of the picture's height as pixels of one
    /// `height` pixels high.
    pub(crate) fn pack(&self, values: &[ParamValue], height: u32) -> [f32; MAX_PARAMS] {
        let mut params = [0f32; MAX_PARAMS];
        let mut floats =
            self.params
                .iter()
                .zip(values)
                .flat_map(|(param, v)| match (*v, param.kind) {
                    (ParamValue::Number(n), ParamKind::Number { unit, .. }) => {
                        vec![match unit {
                            Unit::PictureHeight => (n / 100.0 * f64::from(height)) as f32,
                            _ => n as f32,
                        }]
                    }
                    (ParamValue::Number(n), _) => vec![n as f32],
                    (ParamValue::Bool(b), _) => vec![f32::from(u8::from(b))],
                    (ParamValue::Color(rgb), _) => rgb.map(|c| c as f32).to_vec(),
                    // An `int` of the option's index.
                    (ParamValue::Choice(choice), ParamKind::Choice { options, .. }) => {
                        vec![options.iter().position(|(o, _)| *o == choice).unwrap_or(0) as f32]
                    }
                    (ParamValue::Choice(_), _) => vec![0.0],
                });
        for u in &self.uniforms {
            let n = match u.ty {
                // Colors' alpha is the template's.
                UniformType::Color { .. } => 3,
                ty => ty.floats(),
            };
            for param in &mut params[u.slot..u.slot + n] {
                *param = floats.next().unwrap_or_default();
            }
        }
        params
    }
}

/// GLSL converting between the linear working space and sRGB-encoded
/// BT.709 (`nox_encode`, `nox_decode`).
pub(crate) fn color_functions() -> String {
    let rows =
        |m: nox_color::Mat3| m.map(|r| format!("vec3({:.9}, {:.9}, {:.9})", r[0], r[1], r[2]));
    let to = rows(nox_color::WORKING_PRIMARIES.conversion_to(nox_color::Primaries::Bt709));
    let from = rows(nox_color::Primaries::Bt709.conversion_to(nox_color::WORKING_PRIMARIES));
    format!(
        r#"float nox_oetf(float v) {{
    float a = abs(v);
    return sign(v) * (a <= 0.0031308 ? 12.92 * a : 1.055 * pow(a, 1.0 / 2.4) - 0.055);
}}
float nox_eotf(float v) {{
    float a = abs(v);
    return sign(v) * (a <= 0.04045 ? a / 12.92 : pow((a + 0.055) / 1.055, 2.4));
}}
// Linear working space ↔ sRGB-encoded BT.709.
vec3 nox_encode(vec3 c) {{
    vec3 l = vec3(dot({t0}, c), dot({t1}, c), dot({t2}, c));
    return vec3(nox_oetf(l.r), nox_oetf(l.g), nox_oetf(l.b));
}}
vec3 nox_decode(vec3 e) {{
    vec3 l = vec3(nox_eotf(e.r), nox_eotf(e.g), nox_eotf(e.b));
    return vec3(dot({f0}, l), dot({f1}, l), dot({f2}, l));
}}
"#,
        t0 = to[0],
        t1 = to[1],
        t2 = to[2],
        f0 = from[0],
        f1 = from[1],
        f2 = from[2],
    )
}

/// `source` (a compute shader) compiled to SPIR-V with glslang.
pub(crate) fn compile(source: String) -> Result<Vec<u32>, String> {
    use glslang::{
        Compiler, CompilerOptions, ShaderInput, ShaderSource, ShaderStage, SpirvVersion, Target,
        VulkanVersion,
    };
    let compiler = Compiler::acquire().ok_or("glslang didn't start")?;
    let source = ShaderSource::from(source);
    let options = CompilerOptions {
        target: Target::Vulkan {
            version: VulkanVersion::Vulkan1_3,
            spirv_version: SpirvVersion::SPIRV1_6,
        },
        ..CompilerOptions::default()
    };
    let input = ShaderInput::new(
        &source,
        ShaderStage::Compute,
        &options,
        None::<&[(&str, Option<&str>)]>,
        None,
    )
    .map_err(|e| e.to_string())?;
    compiler
        .create_shader(input)
        .and_then(|shader| shader.compile())
        .map_err(|e| compile_errors(&e.to_string()))
}

/// glslang's log as the errors in it, one per line, at their lines in
/// the file (`ERROR: 0:12: 'x' : undeclared identifier` → `line 12: 'x' :
/// undeclared identifier`), without its summary lines.
fn compile_errors(log: &str) -> String {
    let errors: Vec<String> = log
        .lines()
        .filter_map(|line| line.trim().strip_prefix("ERROR: "))
        .filter(|e| !e.contains("compilation terminated") && !e.contains("compilation errors"))
        .map(
            |e| match e.strip_prefix("0:").and_then(|rest| rest.split_once(": ")) {
                Some((line, message)) => format!("line {line}: {}", message.trim()),
                None => e.trim().to_owned(),
            },
        )
        .collect();
    if errors.is_empty() {
        log.trim().to_owned()
    } else {
        errors.join("\n")
    }
}

/// A package that couldn't be loaded, and why.
#[derive(Debug, Clone)]
pub struct Problem {
    pub path: PathBuf,
    pub message: String,
}

/// The `.glsl` packages in `dir`, sorted by name: (file stem, `load`ed)
/// and the ones `load` refused (logged as `kind` packages).
pub(crate) fn load_packages<T>(
    dir: &Path,
    kind: &str,
    load: impl Fn(&str) -> Result<T, String>,
) -> (Vec<(String, T)>, Vec<Problem>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "glsl"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    let mut loaded = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|source| load(&source));
        match result {
            Ok(parsed) => loaded.push((stem, parsed)),
            Err(message) => {
                tracing::warn!("{kind} package {}: {message}", path.display());
                problems.push(Problem { path, message });
            }
        }
    }
    (loaded, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "// Name: Test
// Author: Nox
// License: MIT
// Description: The top half first
// Category: Wipes

// @range(0, 2) Edge
uniform float edge; // = 0.5
uniform vec2 direction; // = vec2(-1.0, 1.0)
// @color Tint
uniform vec3 tint /* = vec3(1.0) */; // = vec3(1.0, 0.5, 0.0)
uniform ivec2 squares; // = ivec2(20)
uniform bool flip; // = true
// @range(0, 10, %h) Radius
uniform float radius; // = 1.0

vec4 transition(vec2 uv) {
  return vec4(edge);
}
";

    #[test]
    fn parses_headers_and_uniforms() {
        let t = GlslSource::parse(SOURCE).unwrap();
        assert_eq!(
            (t.get("Name"), t.get("License"), t.header("Category")),
            ("Test".to_owned(), "MIT".to_owned(), Some("Wipes"))
        );
        assert_eq!(t.header("Passes"), None);
        let names: Vec<_> = t.params.iter().map(|p| (p.name, p.label)).collect();
        assert_eq!(
            names,
            [
                ("edge", "Edge"),
                ("direction_x", "Direction X"),
                ("direction_y", "Direction Y"),
                ("tint", "Tint"),
                ("squares_x", "Squares X"),
                ("squares_y", "Squares Y"),
                ("flip", "Flip"),
                ("radius", "Radius"),
            ]
        );
        assert_eq!(
            t.params[0].kind,
            ParamKind::Number {
                min: 0.0,
                max: 2.0,
                default: 0.5,
                digits: 2,
                unit: Unit::Plain
            }
        );
        assert!(matches!(
            t.params[1].kind,
            ParamKind::Number {
                min: -3.0,
                max: 3.0,
                default: -1.0,
                ..
            }
        ));
        assert_eq!(
            t.params[3].kind,
            ParamKind::Color {
                default: [1.0, 0.5, 0.0]
            }
        );
        assert!(matches!(
            t.params[4].kind,
            ParamKind::Number {
                default: 20.0,
                digits: 0,
                ..
            }
        ));
        assert_eq!(t.params[6].kind, ParamKind::Bool { default: true });
        assert!(matches!(
            t.params[7].kind,
            ParamKind::Number {
                unit: Unit::PictureHeight,
                ..
            }
        ));
        // The uniforms are out of the body, lines kept.
        assert!(!t.body.contains("uniform"));
        assert_eq!(t.body.lines().count(), SOURCE.lines().count());
        let (globals, assignments) = t.globals("p");
        assert!(globals.contains("float edge;"));
        assert!(assignments.contains("edge = p.nox_params[0];"));
        assert!(assignments.contains("direction = vec2(p.nox_params[1], p.nox_params[2]);"));
        assert!(assignments.contains("tint = vec3("));
        assert!(globals.contains("ivec2 squares;"));
        assert!(assignments.contains("flip = (p.nox_params[8] != 0.0);"));
        // Sizes in percent of the picture's height reach it as pixels.
        let values = crate::registry::param_values(
            &nox_project::Effect::rendering("x", "x"),
            &t.params,
            0.0,
        );
        let packed = t.pack(&values, 1080);
        assert_eq!(packed[0], 0.5);
        assert_eq!(packed[9], 10.8);
    }

    #[test]
    fn refuses_what_it_cannot_run() {
        let sampler = "uniform sampler2D map;\nvec4 effect(vec2 uv) { return vec4(0.0); }";
        assert!(GlslSource::parse(sampler).unwrap_err().contains("images"));
        let no_default = "uniform float x;\nvec4 effect(vec2 uv) { return vec4(0.0); }";
        assert!(
            GlslSource::parse(no_default)
                .unwrap_err()
                .contains("default")
        );
        let unit = "// @range(0, 1, px) X\nuniform float x; // = 0.5\n";
        assert!(GlslSource::parse(unit).unwrap_err().contains("unit"));
        let broken = "#version 450\nlayout(local_size_x = 1) in;\nvoid main() { nope; }";
        assert_eq!(
            compile(broken.into()).unwrap_err().lines().next(),
            Some("line 3: 'nope' : undeclared identifier")
        );
    }

    #[test]
    fn labels_from_names() {
        assert_eq!(label_of("squaresMin"), "Squares min");
        assert_eq!(label_of("border_width"), "Border width");
        assert_eq!(label_of("Seed"), "Seed");
    }
}
