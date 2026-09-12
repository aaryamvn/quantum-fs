import { useEffect, useRef, useState } from "react";
import type { MotionValue } from "motion/react";

import { FRAG_HEADER } from "./shaders/header";
import type { FieldShader } from "./shaders/types";

declare global {
  interface Window {
    __qfsField?: { ok: boolean; shader: string; backing: [number, number] };
  }
}

export interface ColorFieldProps {
  progress: MotionValue<number>;
  settle: MotionValue<number>;
  time: MotionValue<number>;
  /** Seconds since the settle finished; 0 until then. Drives the ambient drift. */
  idle: MotionValue<number>;
  shader: FieldShader;
  /**
   * Final panel width as a fraction of the viewport width.
   *
   * A motion value is read per frame inside the render loop, so the vault dive
   * can bloom and drain the whole field without re-rendering this component.
   */
  panel?: number | MotionValue<number>;
  /** Backing-store scale: the canvas is rendered tiny and blurred up. */
  scale?: number;
  /** CSS blur radius in px. */
  blur?: number;
}

const VERT = `#version 300 es
precision highp float;
in vec2 a_pos;
uniform vec2 u_canvasCss;
uniform vec2 u_bleed;
uniform vec2 u_viewport;
out vec2 v_uv;
void main() {
  vec2 css = (a_pos * 0.5 + 0.5) * u_canvasCss;
  vec2 vp = (css - u_bleed) / u_viewport;
  v_uv = vec2(vp.x, 1.0 - vp.y);
  gl_Position = vec4(a_pos, 0.0, 1.0);
}
`;

const CONTEXT_ATTRS: WebGLContextAttributes = {
  alpha: false,
  antialias: false,
  depth: false,
  stencil: false,
  premultipliedAlpha: false,
  powerPreference: "high-performance",
  preserveDrawingBuffer: false,
};

/** Static fallback used when WebGL2 is unavailable. */
const FALLBACK_GRADIENT = "linear-gradient(90deg, #FF7B7B 0%, #4E0EFF 18%, #010513 45%)";

let warnedNoWebGL2 = false;

function compile(gl: WebGL2RenderingContext, type: number, src: string, id: string): WebGLShader | null {
  const sh = gl.createShader(type);
  if (!sh) return null;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    const kind = type === gl.VERTEX_SHADER ? "vertex" : "fragment";
    console.error(`ColorField[${id}]: ${kind} shader failed\n${gl.getShaderInfoLog(sh) ?? ""}`);
    gl.deleteShader(sh);
    return null;
  }
  return sh;
}

function link(gl: WebGL2RenderingContext, fragBody: string, id: string): WebGLProgram | null {
  const vs = compile(gl, gl.VERTEX_SHADER, VERT, id);
  if (!vs) return null;
  const fs = compile(gl, gl.FRAGMENT_SHADER, `${FRAG_HEADER}\n${fragBody}`, id);
  if (!fs) {
    gl.deleteShader(vs);
    return null;
  }

  const prog = gl.createProgram();
  if (!prog) {
    gl.deleteShader(vs);
    gl.deleteShader(fs);
    return null;
  }

  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.bindAttribLocation(prog, 0, "a_pos");
  gl.linkProgram(prog);
  gl.deleteShader(vs);
  gl.deleteShader(fs);

  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    console.error(`ColorField[${id}]: program link failed\n${gl.getProgramInfoLog(prog) ?? ""}`);
    gl.deleteProgram(prog);
    return null;
  }
  return prog;
}

export function ColorField({
  progress,
  settle,
  time,
  idle,
  shader,
  panel = 0.45,
  scale = 0.25,
  blur = 22,
}: ColorFieldProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [supported, setSupported] = useState(true);

  // Latest per-frame inputs, read inside the RAF loop without re-running the effect.
  const liveRef = useRef({ progress, settle, time, idle, panel });
  liveRef.current = { progress, settle, time, idle, panel };

  const disposedRef = useRef(false);

  const bleed = Math.round(blur * 2);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const gl = canvas.getContext("webgl2", CONTEXT_ATTRS);
    if (!gl) {
      if (!warnedNoWebGL2) {
        warnedNoWebGL2 = true;
        console.warn("ColorField: WebGL2 unavailable — falling back to a static gradient.");
      }
      setSupported(false);
      return;
    }

    disposedRef.current = false;

    let program: WebGLProgram | null = null;
    let buffer: WebGLBuffer | null = null;
    let raf = 0;
    let reported = false;

    const size = { w: 1, h: 1, cssW: 1, cssH: 1, vpW: 1, vpH: 1 };

    let uResolution: WebGLUniformLocation | null = null;
    let uViewport: WebGLUniformLocation | null = null;
    let uTime: WebGLUniformLocation | null = null;
    let uProgress: WebGLUniformLocation | null = null;
    let uSettle: WebGLUniformLocation | null = null;
    let uIdle: WebGLUniformLocation | null = null;
    let uPanel: WebGLUniformLocation | null = null;
    let uCanvasCss: WebGLUniformLocation | null = null;
    let uBleed: WebGLUniformLocation | null = null;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const cssW = rect.width || canvas.clientWidth;
      const cssH = rect.height || canvas.clientHeight;
      if (cssW <= 0 || cssH <= 0) return;

      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const w = Math.max(1, Math.round(cssW * dpr * scale));
      const h = Math.max(1, Math.round(cssH * dpr * scale));

      if (canvas.width !== w) canvas.width = w;
      if (canvas.height !== h) canvas.height = h;

      size.w = w;
      size.h = h;
      size.cssW = cssW;
      size.cssH = cssH;
      size.vpW = Math.max(1, cssW - bleed * 2);
      size.vpH = Math.max(1, cssH - bleed * 2);

      gl.viewport(0, 0, w, h);
    };

    const init = () => {
      program = link(gl, shader.frag, shader.id);
      if (!program) return false;

      buffer = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      // Fullscreen triangle.
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
      gl.enableVertexAttribArray(0);
      gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

      gl.useProgram(program);
      uResolution = gl.getUniformLocation(program, "u_resolution");
      uViewport = gl.getUniformLocation(program, "u_viewport");
      uTime = gl.getUniformLocation(program, "u_time");
      uProgress = gl.getUniformLocation(program, "u_progress");
      uSettle = gl.getUniformLocation(program, "u_settle");
      uIdle = gl.getUniformLocation(program, "u_idle");
      uPanel = gl.getUniformLocation(program, "u_panel");
      uCanvasCss = gl.getUniformLocation(program, "u_canvasCss");
      uBleed = gl.getUniformLocation(program, "u_bleed");

      gl.disable(gl.DEPTH_TEST);
      gl.disable(gl.BLEND);
      resize();
      return true;
    };

    let ready = init();

    const frame = () => {
      raf = requestAnimationFrame(frame);
      if (!ready || !program) return;
      if (gl.isContextLost()) return;
      if (document.visibilityState !== "visible") return;

      const live = liveRef.current;

      gl.useProgram(program);
      gl.uniform2f(uResolution, size.w, size.h);
      gl.uniform2f(uViewport, size.vpW, size.vpH);
      gl.uniform2f(uCanvasCss, size.cssW, size.cssH);
      gl.uniform2f(uBleed, bleed, bleed);
      gl.uniform1f(uTime, live.time.get());
      gl.uniform1f(uProgress, live.progress.get());
      gl.uniform1f(uSettle, live.settle.get());
      gl.uniform1f(uIdle, live.idle.get());
      gl.uniform1f(uPanel, typeof live.panel === "number" ? live.panel : live.panel.get());

      gl.drawArrays(gl.TRIANGLES, 0, 3);

      if (import.meta.env.DEV && !reported) {
        reported = true;
        window.__qfsField = { ok: true, shader: shader.id, backing: [size.w, size.h] };
      }
    };
    raf = requestAnimationFrame(frame);

    let ro: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      ro = new ResizeObserver(resize);
      ro.observe(canvas);
    } else {
      window.addEventListener("resize", resize);
    }

    const onLost = (e: Event) => {
      e.preventDefault();
      ready = false;
    };
    const onRestored = () => {
      program = null;
      buffer = null;
      ready = init();
    };
    canvas.addEventListener("webglcontextlost", onLost);
    canvas.addEventListener("webglcontextrestored", onRestored);

    return () => {
      disposedRef.current = true;
      cancelAnimationFrame(raf);
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestored);
      if (ro) ro.disconnect();
      else window.removeEventListener("resize", resize);

      if (!gl.isContextLost()) {
        if (program) gl.deleteProgram(program);
        if (buffer) gl.deleteBuffer(buffer);
      }
      program = null;
      buffer = null;

      // StrictMode remounts synchronously right after this cleanup, and a lost
      // context cannot be handed back by getContext — so only really drop the
      // context when the component is still gone on the next tick.
      const lose = gl.getExtension("WEBGL_lose_context");
      setTimeout(() => {
        if (disposedRef.current) lose?.loseContext();
      }, 0);
    };
  }, [shader, scale, bleed]);

  if (!supported) {
    return (
      <div
        aria-hidden
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 0,
          pointerEvents: "none",
          background: FALLBACK_GRADIENT,
        }}
      />
    );
  }

  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      style={{
        position: "fixed",
        top: -bleed,
        left: -bleed,
        width: `calc(100% + ${bleed * 2}px)`,
        height: `calc(100% + ${bleed * 2}px)`,
        display: "block",
        zIndex: 0,
        pointerEvents: "none",
        filter: `blur(${blur}px)`,
        transform: "translateZ(0)",
      }}
    />
  );
}
