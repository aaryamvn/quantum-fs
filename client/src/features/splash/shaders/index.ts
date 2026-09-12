import { field } from "./field";
import type { FieldShader } from "./types";

export type { FieldShader } from "./types";
export { FRAG_HEADER } from "./header";

export type FieldId = "default";

export const FIELD_SHADERS: Record<FieldId, FieldShader> = { default: field };

export const DEFAULT_FIELD: FieldId = "default";

/** `?field=` → shader; there is only one, so anything resolves to it. */
export function resolveField(id: string | null): FieldShader {
  void id;
  return FIELD_SHADERS[DEFAULT_FIELD];
}
