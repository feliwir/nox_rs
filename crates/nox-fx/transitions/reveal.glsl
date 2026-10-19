// Name: Reveal
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: The outgoing picture slides away, showing the incoming one
// Category: Motion
// Tags: motion, slide, reveal
// @range(-180, 180) Angle
uniform float angle; // = 0.0

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

// A picture's move towards angle (clockwise from the right), far enough
// to leave the frame, as a part of its width and height.
vec2 away() {
  vec2 dir = vec2(cos(radians(angle)), sin(radians(angle)));
  return dir / max(abs(dir.x), abs(dir.y));
}

vec4 transition(vec2 uv) {
  vec2 s = vec2(uv.x, 1.0 - uv.y);
  return over(at(false, s - away() * progress), at(true, s));
}
