// Name: Spin
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: The incoming picture spins in, growing from the center
// Category: Motion
// Tags: motion, spin, rotate

// @range(0, 5) Turns
uniform float turns; // = 1.0
// @param Clockwise
uniform bool clockwise; // = true

// Picture point s (0-1 from the top left) of the incoming or outgoing
// picture; transparent outside it.
vec4 at(bool incoming, vec2 s) {
  if (any(lessThan(s, vec2(0.0))) || any(greaterThan(s, vec2(1.0)))) {
    return vec4(0.0);
  }
  vec2 uv = vec2(s.x, 1.0 - s.y);
  return incoming ? getToColor(uv) : getFromColor(uv);
}

// top over bottom.
vec4 over(vec4 top, vec4 bottom) {
  float a = top.a + bottom.a * (1.0 - top.a);
  if (a <= 0.0) {
    return vec4(0.0);
  }
  return vec4((top.rgb * top.a + bottom.rgb * bottom.a * (1.0 - top.a)) / a, a);
}

vec4 transition(vec2 uv) {
  vec2 s = vec2(uv.x, 1.0 - uv.y);
  float scale = max(progress, 1e-4);
  // Turned this far (degrees, clockwise), back to no turn at the end.
  float turned = (clockwise ? -1.0 : 1.0) * (1.0 - progress) * 360.0 * turns;
  float c = cos(radians(turned));
  float n = sin(radians(turned));
  vec2 d = (s - 0.5) * vec2(ratio, 1.0);
  // Undo the turn and the scale.
  vec2 q = vec2(c * d.x + n * d.y, -n * d.x + c * d.y) / scale;
  return over(at(true, q / vec2(ratio, 1.0) + 0.5), at(false, s));
}
