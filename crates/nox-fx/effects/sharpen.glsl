// Name: Sharpen
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: Brings out detail (an unsharp mask); detail below the threshold, such as noise, is left alone
// Category: Blur and Sharpen
// Passes: horizontal, vertical, sharpen
// Color: linear

// @range(0, 500, %) Amount
uniform float amount; // = 100.0
// @range(0, 2, %h) Radius
uniform float radius; // = 0.15
// @range(0, 50, %) Threshold
uniform float threshold; // = 0.0

// One direction of a separable Gaussian (as Gaussian Blur), premultiplied
// in the first pass and straight again after the second.
vec4 blurAlong(ivec2 direction, bool first) {
  float sigma = max(max(radius, 0.5) / 2.0, 0.3);
  int stride = int(max(floor(sigma / 3.0), 1.0));
  int taps = int(ceil(3.0 * sigma / float(stride)));
  float falloff = 1.0 / (2.0 * sigma * sigma);
  vec4 sum = vec4(0.0);
  float weights = 0.0;
  for (int i = -taps; i <= taps; i++) {
    int d = i * stride;
    float w = exp(-float(d * d) * falloff);
    vec4 c = first ? getColorAt(pixel + direction * d) : getPassAt(pixel + direction * d);
    if (first) {
      c.rgb *= c.a;
    }
    sum += c * w;
    weights += w;
  }
  return sum / weights;
}

vec4 horizontal(vec2 uv) {
  return blurAlong(ivec2(1, 0), true);
}

vec4 vertical(vec2 uv) {
  vec4 c = blurAlong(ivec2(0, 1), false);
  c.rgb = c.a > 1e-6 ? c.rgb / c.a : vec3(0.0);
  return c;
}

// The picture plus amount × (picture − blurred) where the difference is
// above the threshold (softly), color only.
vec4 sharpen(vec2 uv) {
  vec4 c = getColorAt(pixel);
  if (amount <= 0.0) {
    return c;
  }
  vec3 detail = c.rgb - getPassAt(pixel).rgb;
  float size = max(abs(detail.r), max(abs(detail.g), abs(detail.b)));
  float t = threshold / 100.0;
  float gate = smoothstep(t, t * 2.0 + 1e-6, size);
  c.rgb = max(c.rgb + amount / 100.0 * gate * detail, vec3(0.0));
  return c;
}
