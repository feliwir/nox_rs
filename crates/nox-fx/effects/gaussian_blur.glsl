// Name: Gaussian Blur
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: Blurs the picture; transparent parts don't bleed into it
// Category: Blur and Sharpen
// Passes: horizontal, vertical
// Color: linear

// @range(0, 10, %h) Radius
uniform float radius; // = 1.0

// One direction of a separable Gaussian, out to 3σ; wide blurs take every
// few pixels, which a Gaussian that wide hardly notices. The first pass
// weights colors by alpha (premultiplied), the second divides again.
vec4 blurAlong(ivec2 direction, bool first) {
  float sigma = max(radius / 2.0, 0.3);
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
  if (radius < 0.5) {
    return getColorAt(pixel);
  }
  return blurAlong(ivec2(1, 0), true);
}

vec4 vertical(vec2 uv) {
  if (radius < 0.5) {
    return getColorAt(pixel);
  }
  vec4 c = blurAlong(ivec2(0, 1), false);
  c.rgb = c.a > 1e-6 ? c.rgb / c.a : vec3(0.0);
  return c;
}
