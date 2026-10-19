// Name: Iris
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: The incoming picture grows from the center in a shape
// Category: Wipes
// Tags: wipe, iris, shape

// @choice(circle: Circle, ellipse: Ellipse, diamond: Diamond, rectangle: Rectangle, star: Star) Shape
uniform int shape; // = 0
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

// Distance of d (from the center, in picture heights) in units of the
// shape.
float size(vec2 d) {
  if (shape == 1) {
    return length(d / vec2(ratio, 1.0));
  } else if (shape == 2) {
    return abs(d.x) + abs(d.y);
  } else if (shape == 3) {
    return max(abs(d.x) / ratio, abs(d.y));
  } else if (shape == 4) {
    // Five points: radius 1 at a point, 0.45 between them.
    float a = atan(d.x, -d.y) / (2.0 * 3.14159265359) * 5.0;
    return length(d) / mix(0.45, 1.0, abs(fract(a) - 0.5) * 2.0);
  }
  return length(d);
}

vec4 transition(vec2 uv) {
  vec2 center = vec2(center_x, center_y) / 100.0;
  vec2 d = (vec2(uv.x, 1.0 - uv.y) - center) * vec2(ratio, 1.0);
  // The farthest corner is reached last.
  float far = 0.0;
  for (int i = 0; i < 4; i++) {
    vec2 corner = vec2(i & 1, i >> 1);
    far = max(far, size((corner - center) * vec2(ratio, 1.0)));
  }
  return reveal(uv, size(d) / max(far, 1e-6));
}
