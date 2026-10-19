// Name: Chroma Key
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: Makes a color (a green or blue screen) transparent, holding its spill down
// Category: Keying
// Color: linear
// Outputs: picture, matte

// @color Key Color
uniform vec3 color; // = vec3(0.0, 0.69, 0.25)
// @range(0, 100, %) Tolerance
uniform float tolerance; // = 20.0
// @range(0, 100, %) Softness
uniform float softness; // = 10.0
// @range(0, 100, %) Spill Suppression
uniform float spill; // = 50.0
// @param Show Matte
uniform bool show_matte; // = false

// Alpha falls off as a pixel's chromaticity (rgb / (r + g + b), so
// exposure doesn't matter) nears the key's; the key's dominant channel is
// held down to the others' (spill suppression).
vec4 effect(vec2 uv) {
  vec3 key = max(workingFromDisplay(color), vec3(0.0));
  key /= max(key.r + key.g + key.b, 1e-6);
  // The strongest channel (the last of equal ones).
  int dominant = key.b >= key.g && key.b >= key.r ? 2 : (key.g >= key.r ? 1 : 0);
  // Chromaticity distances: a saturated key is about 0.5 from gray.
  float near = tolerance / 100.0 * 0.4;
  float fade = softness / 100.0 * 0.4;
  vec4 c = getColorAt(pixel);
  vec3 rgb = max(c.rgb, vec3(0.0));
  float sum = rgb.r + rgb.g + rgb.b;
  // Black and near black have no chromaticity: kept.
  float d = sum > 1e-4 ? distance(rgb / sum, key) : 1.0;
  float alpha = smoothstep(near, near + fade + 1e-6, d);
  float limit = max(rgb[(dominant + 1) % 3], rgb[(dominant + 2) % 3]);
  rgb[dominant] = mix(rgb[dominant], min(rgb[dominant], limit), spill / 100.0);
  c.a *= alpha;
  c.rgb = rgb;
  if (show_matte) {
    c = vec4(vec3(c.a), 1.0);
  }
  return c;
}
