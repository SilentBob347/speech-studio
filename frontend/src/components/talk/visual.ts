export type TalkState = "off" | "muted" | "listening" | "hearing" | "thinking" | "speaking";
export type TalkStyle = "aura" | "wave";
export const TALK_STYLES: TalkStyle[] = ["aura", "wave"];

/** What every look of the talk screen is drawn from: the conversation's state, how loud whoever speaks is right now
 * (read each frame, never through React), and the theme colour it is painted in. */
export type VisualProps = { state: TalkState; level: () => number; token: string; className?: string };

/** A theme token's colour as a shader takes it, and whether the page is light. */
export function readTheme(token: string, probe: CanvasRenderingContext2D): { color: [number, number, number]; light: boolean } {
  const style = getComputedStyle(document.documentElement);
  const rgb = (name: string): [number, number, number] => {
    probe.fillStyle = style.getPropertyValue(name).trim();
    const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(probe.fillStyle);
    if (!m) throw new Error(`talk visual: ${name} is not a colour`);
    return [parseInt(m[1], 16) / 255, parseInt(m[2], 16) / 255, parseInt(m[3], 16) / 255];
  };
  // read from the theme's background, not color-scheme: the widget's page drops its scheme to stay see-through
  const [r, g, b] = rgb("--color-bg");
  return { color: rgb(token), light: 0.299 * r + 0.587 * g + 0.114 * b > 0.5 };
}

export function shader(gl: WebGLRenderingContext | WebGL2RenderingContext, type: number, source: string) {
  const s = gl.createShader(type);
  if (!s) throw new Error("WebGL: no shader");
  gl.shaderSource(s, source);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(`WebGL: ${gl.getShaderInfoLog(s) ?? "shader did not compile"}`);
  return s;
}

/** A program drawing one quad over the whole canvas, its vertex position in `p`. */
export function fullscreenProgram(gl: WebGLRenderingContext | WebGL2RenderingContext, vertex: string, fragment: string) {
  const program = gl.createProgram();
  if (!program) throw new Error("WebGL: no program");
  gl.attachShader(program, shader(gl, gl.VERTEX_SHADER, vertex));
  gl.attachShader(program, shader(gl, gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(`WebGL: ${gl.getProgramInfoLog(program) ?? "program did not link"}`);
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const at = gl.getAttribLocation(program, "p");
  gl.enableVertexAttribArray(at);
  gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);
  return program;
}

/** Keeps the canvas's backing store at its shown size, up to twice the CSS pixels; true when it changed. */
export function fitCanvas(el: HTMLCanvasElement, gl: WebGLRenderingContext | WebGL2RenderingContext) {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(el.clientWidth * ratio));
  const h = Math.max(1, Math.round(el.clientHeight * ratio));
  if (el.width === w && el.height === h) return false;
  el.width = w;
  el.height = h;
  gl.viewport(0, 0, w, h);
  return true;
}
