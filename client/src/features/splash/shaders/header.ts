/**
 * Prepended to every `FieldShader.frag`. This is the contract a field shader
 * writes against: everything below is already declared for you.
 *
 * `#version` must be the very first character of the final source.
 */
export const FRAG_HEADER = `#version 300 es
precision highp float;
uniform vec2  u_resolution;  // canvas backing-store size in px
uniform vec2  u_viewport;    // viewport size in CSS px (aspect = u_viewport.x / u_viewport.y)
uniform float u_time;        // seconds since mount; frozen under ?at=
uniform float u_progress;    // 0..1 eased barrage progress (waves fly in from the right)
uniform float u_settle;      // 0..1 eased settle progress (field resolves into the left-panel gradient)
uniform float u_idle;        // seconds since the settle finished (0 during barrage/settle; frozen under ?at=)
uniform float u_panel;       // final panel width as a fraction of viewport width (0.45)
in vec2 v_uv;                // viewport-space uv: x 0→1 left→right, y 0→1 TOP→bottom; slightly outside [0,1] in the bleed margin
out vec4 fragColor;
const vec3 CORAL  = vec3(1.000, 0.482, 0.482); // #FF7B7B
const vec3 VIOLET = vec3(0.306, 0.055, 1.000); // #4E0EFF
const vec3 BLACK  = vec3(0.0039, 0.0196, 0.0745); // page background #010513
`;
