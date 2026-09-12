import type { FieldShader } from "./types";

/**
 * "Liquid Ink" — a barrage of ink blobs is pushed in from beyond the right
 * edge along a curl-noise flow, swirls across the black water, pools into the
 * left panel and cross-fades into the exact coral → violet → black gradient.
 *
 * Three rules hold the look together:
 *
 * 1. Colour is never mixed in RGB. Every blob carries a position `u` on a 1-D
 *    ink ramp (0 = coral, ~0.28 = magenta, ~0.45 = violet, 1 = black) and it is
 *    the `u` values that merge, so coral meeting violet always passes through
 *    magenta. The ramp itself interpolates in OKLCh (lightness, chroma, hue)
 *    rather than RGB, so chroma climbs monotonically from coral to violet
 *    instead of notching out halfway.
 * 2. A wavefront gates the ink. Nothing is drawn left of the front, which
 *    sweeps in from beyond the right edge — that is what makes the barrage read
 *    as arriving *from the right* rather than fading up in place.
 * 3. Nothing is ever a hole. Where ink thins or parts, a trough of the local
 *    hue pushed deeper down the ramp sits underneath, and an abyss floor of
 *    near-black violet sits under everything, so a gap reads as water rather
 *    than as an unrendered frame. Both vanish as the field settles, which is
 *    what keeps the final gradient exact.
 */
export const field: FieldShader = {
  id: "default",
  label: "Liquid Ink",
  frag: `
// ── noise ───────────────────────────────────────────────────────────────
float hash21(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float valueNoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = hash21(i);
  float b = hash21(i + vec2(1.0, 0.0));
  float c = hash21(i + vec2(0.0, 1.0));
  float d = hash21(i + vec2(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// 3 octaves, roughly zero-centred.
float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 3; i++) {
    v += a * valueNoise(p);
    p = p * 2.03 + vec2(11.7, 4.3);
    a *= 0.5;
  }
  return v - 0.4375;
}

// Divergence-free flow field: curl of the fbm scalar potential.
// 4 fbm evaluations — with the idle drift that is 5 per fragment.
vec2 curlNoise(vec2 p) {
  float e = 0.11;
  float n0 = fbm(p + vec2(0.0, e));
  float n1 = fbm(p - vec2(0.0, e));
  float n2 = fbm(p + vec2(e, 0.0));
  float n3 = fbm(p - vec2(e, 0.0));
  return vec2(n0 - n1, n3 - n2) * (0.5 / e);
}

// Analytic curl of a sinusoidal stream function. Evaluated once per blob per
// fragment, so it must stay away from fbm; it is divergence-free all the same,
// which is what makes the blobs wander instead of drifting in a straight line.
vec2 swirl(vec2 p, float t) {
  float ax = 1.45 * p.x + 0.62 * t;
  float ay = 1.10 * p.y - 0.47 * t;
  float bx = 1.90 * p.x + 0.53 * t;
  float by = 2.30 * p.y + 0.38 * t;
  float dPdy = -0.605 * sin(ax) * sin(ay) + 0.805 * cos(by) * cos(bx);
  float dPdx =  0.798 * cos(ax) * cos(ay) - 0.665 * sin(by) * sin(bx);
  return vec2(dPdy, -dPdx);
}

// ── ink ramp ────────────────────────────────────────────────────────────
// OKLCh stops, measured from the brand sRGB constants to 8 decimals. Hues are
// unwrapped (21.8° → -10.2° → -82.4°) so the path sweeps monotonically the
// short way round and never crosses the grey axis.
const vec3 LCH_CORAL   = vec3(0.73610919, 0.16149472,  0.38062433);
const vec3 LCH_MAGENTA = vec3(0.68787434, 0.24354720, -0.17797847);
const vec3 LCH_VIOLET  = vec3(0.48755479, 0.29436330, -1.43757377);

vec3 oklchToSrgb(vec3 lch) {
  float a = lch.y * cos(lch.z);
  float b = lch.y * sin(lch.z);

  float l_ = lch.x + 0.39633778 * a + 0.21580376 * b;
  float m_ = lch.x - 0.10556135 * a - 0.06385417 * b;
  float s_ = lch.x - 0.08948418 * a - 1.29148555 * b;

  vec3 lms = vec3(l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);

  vec3 lin = vec3(
     4.07674166 * lms.x - 3.30771159 * lms.y + 0.23096993 * lms.z,
    -1.26843800 * lms.x + 2.60975740 * lms.y - 0.34131940 * lms.z,
    -0.00419609 * lms.x - 0.70341861 * lms.y + 1.70761470 * lms.z
  );
  lin = clamp(lin, 0.0, 1.0);

  return mix(
    lin * 12.92,
    1.055 * pow(lin, vec3(1.0 / 2.4)) - 0.055,
    step(vec3(0.0031308), lin)
  );
}

// 0 = coral · 0.28 = magenta · 0.45 = violet · 1 = black. Lightness, chroma and
// hue are each interpolated on their own, so chroma rises 0.161 → 0.294 across
// the whole ramp with no notch where two hues meet — the failure mode of an RGB
// lerp between magenta and violet.
vec3 inkRamp(float u) {
  u = clamp(u, 0.0, 1.0);
  vec3 lch = mix(LCH_CORAL, LCH_MAGENTA, smoothstep(0.08, 0.28, u));
  lch = mix(lch, LCH_VIOLET, smoothstep(0.26, 0.45, u));
  vec3 c = oklchToSrgb(lch);
  // Scaling toward BLACK keeps the sRGB chromaticity exactly, so the tail of
  // the gradient stays violet instead of going grey. pow() holds the hue a
  // little longer on the way down. u == 1 is exactly BLACK (the page bg).
  return mix(BLACK, c, 1.0 - pow(smoothstep(0.50, 1.0, u), 1.3));
}

// ── blob table ──────────────────────────────────────────────────────────
// a = (startX, startY, restX, restY)  · aspect-corrected uv, startX is beyond
//     the right edge (1.6 at 16:10) so nothing is on screen when the barrage
//     starts; born guarantees literal black at u_progress == 0 regardless.
// b = (settleY, radius, delay, u)
//
// The start column is deliberately tight (2.12 … 2.88, i.e. under half a screen
// of stagger). A long train looks impressive on paper but with the barrage
// clock slowed enough to hold the left edge, the tail blobs simply never arrive
// — 250 ms in you get one lonely blob instead of a wave. Rest positions and the
// ink-ramp column are untouched: those are the composition.
//
// The column was pulled in by 0.234 q (0.146 uv) when the barrage clock was
// slowed: with the old run-up, the whole eased ramp was spent dragging the
// crest across empty off-screen space and 250 ms produced a black frame.
// 0.234 is the run-up that puts the crest at ~x 0.80 uv at t250 — a wave
// breaking into the right quarter — without ever showing ink at t = 0.
void blobSpec(int i, out vec4 a, out vec4 b) {
  if      (i == 0) { a = vec4(2.116, 0.10, 0.18, 0.30); b = vec4(0.24, 0.28, 0.000, 0.02); }
  else if (i == 1) { a = vec4(2.286, 0.92, 0.50, 0.78); b = vec4(0.70, 0.26, 0.036, 0.09); }
  else if (i == 2) { a = vec4(2.176, 0.44, 0.24, 0.06); b = vec4(0.06, 0.24, 0.096, 0.16); }
  else if (i == 3) { a = vec4(2.436, 0.18, 0.80, 0.42); b = vec4(0.50, 0.28, 0.060, 0.26); }
  else if (i == 4) { a = vec4(2.346, 0.70, 0.46, 0.98); b = vec4(0.90, 0.25, 0.144, 0.34); }
  else if (i == 5) { a = vec4(2.576, 0.86, 1.02, 0.18); b = vec4(0.32, 0.27, 0.120, 0.44); }
  else if (i == 6) { a = vec4(2.496, 0.34, 0.68, 0.70); b = vec4(0.60, 0.29, 0.192, 0.53); }
  else if (i == 7) { a = vec4(2.746, 0.62, 1.32, 0.50); b = vec4(0.16, 0.26, 0.168, 0.68); }
  else if (i == 8) { a = vec4(2.666, 0.06, 1.16, 0.94); b = vec4(0.82, 0.28, 0.252, 0.82); }
  else             { a = vec4(2.876, 0.50, 1.54, 0.24); b = vec4(0.44, 0.30, 0.300, 0.94); }
}

void main() {
  float p = u_progress;
  float s = u_settle;
  float t = u_time;
  vec2 uv = v_uv;

  float aspect = u_viewport.x / u_viewport.y;
  vec2 q = vec2(uv.x * aspect, uv.y);

  // ── barrage clock ─────────────────────────────────────────────────────
  // u_progress carries the anticipation-beat / surge / decelerate shape.
  // u_time is the same elapsed time un-eased. The clock leans on the un-eased
  // one because the *visible* deceleration is already in the travel curve —
  // the crest sits at 1.21·(1-g)^1.9, so a near-linear g still sweeps the
  // screen at 1.5 uv/s at the start and 0.3 uv/s at the end. Leaning on the
  // eased one instead spends the whole surge in one 300 ms window and the
  // frames on either side of it go static, which is the bug this replaced.
  // The u_progress term guarantees the clock is exactly 1 when the barrage is
  // over, whatever the barrage duration is set to.
  float tb = clamp(t * 0.5882, 0.0, 1.0);          // 1 / 1.70 s
  float g = clamp(mix(p, tb, 0.78), 0.0, 1.0);
  g = max(g, smoothstep(0.985, 1.0, p));

  // Late barrage: the warp goes finer and shallower. A deep low-frequency warp
  // is what turns circles into ink early on, but held that deep it degenerates
  // into smoke once the blobs overlap — so trade amplitude for frequency and
  // the edges stay sculpted all the way to the settle. Kicking in from g 0.34
  // (rather than 0.45) is what keeps the mid-barrage voids convex.
  float late = smoothstep(0.34, 0.86, g);

  // Curl-noise domain warp — this is what turns circles into ink.
  float warpAmp = mix(0.205, 0.092, late) * mix(1.0, 0.45, s);
  float warpFreq = mix(1.78, 3.15, late);
  vec2 warp = curlNoise(q * warpFreq + vec2(t * 0.11, -t * 0.085)) * warpAmp;
  warp += vec2(sin(q.y * 7.3 + t * 0.55), cos(q.x * 6.1 - t * 0.46)) * mix(0.026, 0.016, late);
  warp += vec2(sin(q.y * 14.9 - t * 0.81), cos(q.x * 12.7 + t * 0.69)) * mix(0.011, 0.015, late);
  vec2 wq = q + warp;
  float wx = wq.x / aspect;  // warped position back in uv-x space

  // ── wavefront ─────────────────────────────────────────────────────────
  // Analytic leading edge of blob 0 (the coral one, delay 0), set back 0.06 uv.
  // Ink never appears to the left of it, so the barrage always reads as
  // arriving from the right instead of fading up in place — and because it sits
  // just inside the leading blob rather than out in the empty black, the front
  // shapes the crest instead of merely clipping it.
  float front = 1.210 * pow(1.0 - g, 1.9) - 0.123;
  // Tilted, then undulated: a diagonal front is far stronger than a vertical
  // one, but a *straight* diagonal reads as a seam, so bend it. 0.21 uv of lean
  // over the height is ~18° at 16:10 — enough to read as a raking wave.
  float crest = 0.21 * (uv.y - 0.5);
  crest += 0.055 * sin(uv.y * 4.7 + 1.1) + 0.032 * sin(uv.y * 9.3 - 0.6);
  // …and fbm on top of the sines, so no two crests repeat down the edge.
  crest += 0.16 * fbm(vec2(uv.y * 2.6, 3.7));
  front += crest;

  float gate = smoothstep(front, front + 0.17, wx);
  // The trough field leads the ink by ~0.2 uv: a dim bow wave ahead of the
  // front, so the black the front is holding back never looks unrendered.
  float halo = smoothstep(front - 0.24, front + 0.30, wx);

  float swirlAmp = mix(0.16, 0.02, s);
  float sumU = 0.0;    // Σ w·u   — merge happens in ramp space, never in RGB
  float sumW = 0.0;    // Σ w
  float clear = 1.0;   // Π (1 - f) — metaball-style union coverage
  float sumUH = 0.0;   // the same three, at a much wider radius: the trough
  float sumWH = 0.0;   // field that fills the gaps between blobs
  float clearH = 1.0;

  for (int i = 0; i < 10; i++) {
    vec4 A, B;
    blobSpec(i, A, B);

    float delay = B.z;
    float rad = B.y;
    float uval = B.w;

    float qi = clamp((g - delay) / (1.0 - delay), 0.0, 1.0);
    // Later blobs catch up harder, so the whole barrage lands together even
    // though u_progress is already expo-eased.
    float e = 1.0 - pow(1.0 - qi, 1.9 + 2.2 * delay);

    vec2 flying = mix(A.xy, A.zw, e);
    flying += swirl(flying * 1.3 + vec2(float(i) * 2.7, 0.0), t) * swirlAmp;

    // Pooling target: x follows the blob's own place on the ink ramp, so the
    // pooled blobs already form the final gradient before the cross-fade.
    vec2 pooled = vec2(uval * u_panel * aspect, B.x);

    vec2 c = mix(flying, pooled, s);

    // Comet stretch: fast blobs smear backwards (to the right) into a wave.
    float streak = pow(1.0 - e, 0.42);
    float dx = wq.x - c.x;
    float sx = 1.0 + streak * (1.15 + 3.40 * smoothstep(-0.15, 0.25, dx));
    float sy = 1.0 - 0.38 * streak;
    vec2 sc = mix(vec2(sx, sy), vec2(0.85, 3.40), s);

    vec2 d = (wq - c) / sc;
    float dist = length(d);
    // The feather widens as the barrage lands: once blobs overlap, a tight
    // edge turns every gap between them into a hard-cut void. ≥ 0.20 uv of
    // total softness at every stage, which is what keeps the gaps convex.
    float f = 1.0 - smoothstep(rad - mix(0.15, 0.23, late), rad + mix(0.17, 0.26, late), dist);
    float fh = 1.0 - smoothstep(rad * 0.5, rad + 0.85, dist);

    // Every wave carries its own crest. A blob still in flight is clipped left
    // of its own leading edge — which is exactly where its comet smear would
    // otherwise leak ahead of it — so the barrage arrives as a train of tilted
    // fronts rather than one. The clip dissolves as the blob lands (streak → 0)
    // and during the settle, so nothing is ever cut out of a resting blob.
    float cfront = (c.x - rad) / aspect - 0.055 + crest;
    float clip = streak * (1.0 - s);
    f *= mix(1.0, smoothstep(cfront, cfront + 0.16, wx), clip);
    fh *= mix(1.0, smoothstep(cfront - 0.22, cfront + 0.28, wx), clip);

    float w = f * f;
    sumU += w * uval;
    sumW += w;
    clear *= 1.0 - f;

    sumUH += fh * uval;
    sumWH += fh;
    clearH *= 1.0 - 0.72 * fh;
  }

  // Where no ink reaches, the ramp position defaults to 1.0 (black) — that is
  // what keeps the emptying right-hand side from flashing a coloured veil
  // while the field cross-fades.
  float uBlob = (sumU + 0.02) / (sumW + 0.02);
  float uHalo = (sumUH + 0.03) / (sumWH + 0.03);
  float born = smoothstep(0.0, 0.02, p);
  float cov = clamp(1.0 - clear, 0.0, 1.0) * born * gate;
  float glow = clamp(1.0 - clearH, 0.0, 1.0) * born * halo;

  // Thin ink reads cooler and deeper, exactly like a drop diffusing into dark
  // water. Without this, a half-covered coral edge is simply a dimmed coral —
  // i.e. brown. Pushing it up the ramp instead keeps every fringe saturated.
  float thin = 1.0 - cov;
  // The hue wobble is scaled by coverage so it can never cancel the cooling
  // push on a thin fringe — that is what would put a dimmed coral (i.e. brown)
  // on screen.
  float uInk = uBlob + 0.78 * thin * thin + 0.075 * cov * warp.y / warpAmp;
  uInk = clamp(uInk, 0.0, 1.0);

  // Ambient. Every term below that is gated on amb exists only once the
  // settle is over: u_idle is exactly 0 through the barrage and the settle, so
  // amb is exactly 0 there and every idle term adds a literal 0.0. It then eases
  // in over 2.5 s with zero slope at u_idle == 0, so the last settle frame and
  // the first idle frame are the same frame — the life arrives without a seam.
  float amb = smoothstep(0.0, 2.5, u_idle);

  // Resting gradient. The drift is the bulk of the idle motion budget: < 0.03 uv
  // across any 10 s, and every term is continuous in t.
  float breathe =
      0.0090 * sin(uv.y * 2.30 - t * 0.15)
    + 0.0050 * sin(uv.y * 4.10 + t * 0.10 + 1.7)
    + 0.0060 * fbm(vec2(uv.y * 1.60, t * 0.05));
  // Biased strictly positive so the panel only ever breathes *left*: the black
  // point sits at (u_panel - drift), which therefore never slides past u_panel
  // of the width. Range ≈ 0.001 … 0.014 uv, i.e. the black point lives within
  // ~18 px left of the panel edge at 1280 wide (558–575 px at u_panel = 0.45).
  float drift = max(0.0075 + 0.40 * breathe, 0.0);
  float uTarget = (uv.x + drift) / u_panel;
  // Idle: the colour boundary undulates down the panel on a ~18 s period, and
  // the standing wave itself leans and slides on a much slower ~57 s drift — so
  // the edge breathes and tilts instead of ticking through one fixed shape.
  //
  // Offset strictly negative for the same reason the drift above is strictly
  // positive: lowering uTarget moves the black point *right*, so the wave can
  // only ever push the boundary outward, and at 0.009 of ramp (2.3× the old
  // amplitude) a wave centred on zero would otherwise walk the edge past the
  // 530 px floor the settled gradient is held to. Range ≈ 0.5 … 11 px right of
  // where the drift alone puts it: measured over 3 s … 75 s of idle the black
  // point stays inside 534–553 px at 1280 wide.
  float lean = uv.y * (2.4 + 0.55 * sin(u_idle * 0.11 + 0.4));
  uTarget -= amb * 0.009
           * (1.1 - sin(lean + u_idle * 0.35 + 0.8 * sin(u_idle * 0.09) + 1.3));
  // Hue shimmer. Weighted onto the magenta→violet transition (~0.36 of the
  // panel) by a Gaussian that is ≈ 0 at both endpoints, so the steepest part of
  // the ramp shimmers while the coral anchor and the black point do not move:
  // 0.02 of ramp there is 0.009 uv, and under 0.0005 out at the black point.
  float mid = exp(-pow((uv.x / u_panel - 0.36) * 3.4, 2.0));
  uTarget += amb * mid * (
      0.013 * sin(uv.y * 1.70 - u_idle * 0.95 + 0.6)
    + 0.007 * sin(uv.y * 3.10 + u_idle * 1.60 + 2.4)
  );
  uTarget = clamp(uTarget, 0.0, 1.0);

  // The panel resolves before the open field does, and the cross-fade starts
  // far earlier than the ink finishes pooling, so the morph is a drain rather
  // than a cut. The head start is shaped s·(1-s) so it is gone by s == 1: the
  // left edge still runs ~8 points ahead through the middle of the settle, and
  // yet every column reaches fade == 1 at the same instant. Together with a
  // smoothstep that ends at 1.0 rather than 0.95, no settle-driven term
  // saturates before s ≈ 0.97 and all of them arrive with zero slope — which is
  // what stops the last moment of the settle reading as a landing.
  float sLead = s + 0.34 * (1.0 - uv.x) * s * (1.0 - s);
  float fade = smoothstep(0.15, 1.0, sLead);

  // Black crops back in from the right while the ink pools, mirroring the
  // wavefront that opened the barrage. The ramp ends at s == 1 for the same
  // reason the cross-fade does: it used to finish at s == 0.70, i.e. mid-flight.
  float cropX = mix(1.42, u_panel + 0.02, smoothstep(0.04, 1.0, s));
  float crop = 1.0 - smoothstep(cropX - 0.13, cropX + 0.13, wx);

  float uu = mix(uInk, uTarget, fade);
  float cc = mix(cov * crop, 1.0, fade);

  vec3 ink = inkRamp(uu);
  // Troughs. A gap between blobs is the local hue pushed a third of the way
  // further down the ramp and dimmed — a wave trough, not a hole punched
  // through to #000. Dies with the cross-fade, so the settled field is clean.
  vec3 deep = inkRamp(clamp(uHalo + 0.34, 0.0, 1.0));
  float trough = (0.075 + 0.300 * glow) * born * (1.0 - fade) * crop;

  vec3 col = BLACK * (1.0 - cc) + ink * cc + deep * trough * (1.0 - cc);
  // Abyss floor, in two depths. Inside the entered region it is a 5.6% violet
  // so a gap between waves reads as a trough rather than as a hole punched
  // through to #000; ahead of the front it drops to 2.0%, which is black to
  // the eye and is what lets the left third genuinely hold. Both are gone by
  // the settle, which is what keeps the resting right-hand side pure black.
  float abyss = (0.020 + 0.036 * halo) * born * (1.0 - cc) * (1.0 - fade);
  col += VIOLET * abyss;

  // Mottle, not grain: the canvas renders at quarter scale under a 22 px CSS
  // blur, so per-pixel grain is filtered away to nothing. This sits at ~50 px
  // on screen, which survives, and gives the flats a printed-ink tooth.
  float mottle = valueNoise(q * 17.0 + vec2(t * 0.05, -t * 0.04)) - 0.5;
  col *= 1.0 + 0.085 * mottle * (1.0 - fade);

  // Idle: three bands of light travelling along the panel, and they disagree —
  // 12% outward at ~12 s per pass, 4% back the other way at ~33 s, and 4.5%
  // outward at ~7.9 s, each on its own spatial frequency and its own vertical
  // lean. The two slow ones are the pass you watch cross the panel; the third
  // is what makes the motion legible second to second rather than only over a
  // whole pass — with a single ~12 s band the field changes by well under one
  // level from one second to the next, which is why the old 3.5% band read as
  // still. The periods are incommensurate, so the field never repeats a frame.
  //
  // Every band scales the colour *above* BLACK, never the frame — so it can
  // only ever dim or lift what is already lit and can never raise the dark side
  // above the page background.
  //
  // Two windows. The near one keeps the bands off the first tenth of the panel
  // because that is the coral anchor: #FF7B7B is a brand endpoint and has to
  // measure the same at t = 3 s and t = 75 s. The far one fades them out across
  // the last third, where the violet→black tail is only a few levels above
  // BLACK: the bands move nothing the eye can see there, but those few levels
  // are enough to wobble the measured edge of the gradient, and the boundary
  // undulation above is what is supposed to move that edge.
  float bx = (uv.x / u_panel) * 6.2832;
  float band = smoothstep(0.0, 0.10, uv.x)
             * (1.0 - smoothstep(u_panel * 0.68, u_panel * 0.96, uv.x))
             * amb * (
      0.119 * sin(bx - u_idle * 0.52 + uv.y * 0.8)
    + 0.040 * sin(bx * 0.55 + u_idle * 0.19 - uv.y * 1.4 + 2.1)
    + 0.045 * sin(bx * 1.70 - u_idle * 0.80 + uv.y * 2.2 + 0.7)
  );
  col += max(col - BLACK, vec3(0.0)) * band;

  float dither = (hash21(gl_FragCoord.xy + u_time) - 0.5) * (2.0 / 255.0);
  fragColor = vec4(col + dither, 1.0);
}
`,
};
