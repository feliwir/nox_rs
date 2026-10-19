// Name: Dip to Color
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: The outgoing picture fades to a color, then the incoming one fades in from it
// Category: Dissolves
// Tags: fade, color

// @color Color
uniform vec3 color; // = vec3(0.0, 0.0, 0.0)

// a to b by t, with their alpha (premultiplied, then straight again).
vec4 mixColors(vec4 a, vec4 b, float t) {
  vec4 m = mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), t);
  return m.a > 0.0 ? vec4(m.rgb / m.a, m.a) : vec4(0.0);
}

vec4 transition(vec2 uv) {
  vec4 solid = vec4(color, 1.0);
  if (progress < 0.5) {
    return mixColors(getFromColor(uv), solid, 2.0 * progress);
  }
  return mixColors(solid, getToColor(uv), 2.0 * progress - 1.0);
}
