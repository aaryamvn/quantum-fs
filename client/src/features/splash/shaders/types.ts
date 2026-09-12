export interface FieldShader {
  /** Stable id, also the `?field=` value. */
  id: string;
  /** Human-readable name, for dev tooling. */
  label: string;
  /**
   * GLSL ES 3.00 fragment shader BODY: helper functions + `void main()`.
   * The host prepends `FRAG_HEADER` (version, precision, uniforms, varyings,
   * `fragColor`, brand constants) — do not repeat any of it here.
   */
  frag: string;
}
