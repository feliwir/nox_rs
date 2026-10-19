// Name: Clock Wipe
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: A hand sweeps around the center, revealing the incoming picture
// Category: Wipes
// Tags: wipe, clock, radial

// @range(-180, 180) Start Angle
uniform float angle; // = -90.0
// @param Clockwise
uniform bool clockwise; // = true
// @range(0, 100, %) Center X
uniform float center_x; // = 50.0
// @range(0, 100, %) Center Y
uniform float center_y; // = 50.0
// @range(0, 100, %) Softness
uniform float softness; // = 2.0
// @range(0, 25, %) Border
uniform float border; // = 0.0
// @color Border Color
uniform vec3 border_color; // = vec3(1.0, 1.0, 1.0)
// @param Reverse
uniform bool reverse; // = false

// a to b by t, with their alpha (premultiplied, then straight again).
vec4 mixColors(vec4 a, vec4 b, float t) {
  vec4 m = mix(vec4(a.rgb * a.a, a.a), vec4(b.rgb * b.a, b.a), t);
  return m.a > 0.0 ? vec4(m.rgb / m.a, m.a) : vec4(0.0);
}

// The incoming picture where the pixel's reveal time r (0-1) has
// passed, over softness, with a border running ahead of the edge.
vec4 reveal(vec2 uv, float r) {
  vec4 from = getFromColor(uv);
  vec4 to = getToColor(uv);
  if (progress >= 1.0) {
    return to;
  }
  r = clamp(r, 0.0, 1.0);
  if (reverse) {
    r = 1.0 - r;
  }
  float soft = max(softness / 100.0, 0.002);
  float width = border / 100.0;
  float edge = progress * (1.0 + soft + width);
  float shown = clamp((edge - r - width) / soft, 0.0, 1.0);
  float ahead = width > 0.0 ? clamp((edge - r) / 0.002, 0.0, 1.0) * (1.0 - shown) : 0.0;
  return mixColors(mixColors(from, vec4(border_color, 1.0), ahead), to, shown);
}

// The pixel from the picture's center, in picture heights, y down.
vec2 fromCenter(vec2 uv) {
  return (vec2(uv.x, 1.0 - uv.y) - 0.5) * vec2(ratio, 1.0);
}

// A direction at angle degrees, clockwise from the right.
vec2 towards(float degrees) {
  return vec2(cos(radians(degrees)), sin(radians(degrees)));
}

// How far the picture reaches along dir from its center.
float reach(vec2 dir) {
  return 0.5 * (abs(dir.x) * ratio + abs(dir.y));
}

vec4 transition(vec2 uv) {
  vec2 c = (vec2(uv.x, 1.0 - uv.y) - vec2(center_x, center_y) / 100.0) * vec2(ratio, 1.0);
  float turned = (atan(c.y, c.x) - radians(angle)) / (2.0 * 3.14159265359);
  return reveal(uv, fract(clockwise ? turned : -turned));
}
