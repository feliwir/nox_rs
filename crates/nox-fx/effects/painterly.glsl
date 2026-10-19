// Name: Painterly
// Author: Nox contributors
// License: GPL-3.0-or-later
// Description: Smooths regions while keeping their edges, like brush strokes (a generalized Kuwahara filter)
// Category: Stylize
// Color: linear

// @range(0, 3, %h) Radius
uniform float radius; // = 0.5
// @range(1, 16) Sharpness
uniform float sharpness; // = 8.0

const int SECTORS = 8;
const float PI = 3.14159265;

// Around each pixel, the mean and variance of eight sectors of a disc;
// the result is their means weighted towards the flattest sectors
// (1 / (1 + σ^sharpness)). Large radii take every few pixels.
vec4 effect(vec2 uv) {
  vec4 center = getColorAt(pixel);
  int r = int(round(radius));
  if (r < 1) {
    return center;
  }
  int stride = max(r / 6, 1);
  vec3 mean[SECTORS];
  vec3 square[SECTORS];
  float count[SECTORS];
  for (int s = 0; s < SECTORS; s++) {
    mean[s] = vec3(0.0);
    square[s] = vec3(0.0);
    count[s] = 0.0;
  }
  float r2 = float(r * r);
  for (int y = -r; y <= r; y += stride) {
    for (int x = -r; x <= r; x += stride) {
      if (float(x * x + y * y) > r2) {
        continue;
      }
      vec3 c = getColorAt(pixel + ivec2(x, y)).rgb;
      // The center belongs to every sector.
      if (x == 0 && y == 0) {
        for (int s = 0; s < SECTORS; s++) {
          mean[s] += c;
          square[s] += c * c;
          count[s] += 1.0;
        }
        continue;
      }
      float angle = atan(float(y), float(x));
      int s = int(floor((angle + PI) / (2.0 * PI) * float(SECTORS))) % SECTORS;
      mean[s] += c;
      square[s] += c * c;
      count[s] += 1.0;
    }
  }
  vec3 sum = vec3(0.0);
  float weights = 0.0;
  for (int s = 0; s < SECTORS; s++) {
    vec3 m = mean[s] / count[s];
    vec3 v = abs(square[s] / count[s] - m * m);
    // Perceptual: compare variances of roughly perceptual values.
    float sigma = sqrt(v.r + v.g + v.b) / (dot(m, vec3(0.333)) + 0.05);
    float w = 1.0 / (1.0 + pow(sigma * 10.0, sharpness));
    sum += m * w;
    weights += w;
  }
  return vec4(sum / weights, center.a);
}
