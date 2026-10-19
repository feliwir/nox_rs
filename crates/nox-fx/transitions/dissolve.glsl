// Name: Cross Dissolve
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: The outgoing picture fades into the incoming one
// Category: Dissolves
// Tags: fade, blend

// @choice(perceptual: Perceptual, linear: Linear Light) Blend
uniform int blend; // = 0

// a to b by t, with their alpha (premultiplied, then straight again).
vec4 mixColors(vec4 a, vec4 b, float t) {
  vec4 m = mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), t);
  return m.a > 0.0 ? vec4(m.rgb / m.a, m.a) : vec4(0.0);
}

vec3 toLinear(vec3 c) {
  return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c));
}

vec3 toEncoded(vec3 c) {
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

// Perceptually even (as editors' dissolves), or in linear light (as light
// adds up).
vec4 transition(vec2 uv) {
  vec4 from = getFromColor(uv);
  vec4 to = getToColor(uv);
  if (blend == 1) {
    vec4 m = mixColors(vec4(toLinear(max(from.rgb, 0.0)), from.a), vec4(toLinear(max(to.rgb, 0.0)), to.a), progress);
    return vec4(toEncoded(m.rgb), m.a);
  }
  return mixColors(from, to, progress);
}
