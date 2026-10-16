import { useEffect, useRef } from "react";
import { fitCanvas, fullscreenProgram, readTheme, type VisualProps } from "./visual";

const VERTEX = `#version 300 es
in vec2 p;
out vec2 vUv;
void main() { vUv = p / RADIUS; gl_Position = vec4(p, 0.0, 1.0); }`;

const FRAGMENT = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 fragColor;
uniform float uTime, uAnimation, uInverted, uInputVolume, uOutputVolume, uEdge;
uniform float uOffsets[7];
uniform vec3 uColor1, uColor2;
uniform sampler2D uPerlinTexture;
const float PI = 3.14159265358979323846;

bool drawOval(vec2 polarUv, float a, float b, bool reverseGradient, float softness, out vec4 color) {
  vec2 p = polarUv;
  float oval = (p.x * p.x) / (a * a) + (p.y * p.y) / (b * b);
  float edge = smoothstep(1.0, 1.0 - softness, oval);
  if (edge > 0.0) {
    float gradient = reverseGradient ? (1.0 - (p.x / a + 1.0) / 2.0) : ((p.x / a + 1.0) / 2.0);
    gradient = mix(0.5, gradient, 0.1);
    color = vec4(vec3(gradient), 0.85 * edge);
    return true;
  }
  return false;
}
vec3 colorRamp(float g, vec3 c1, vec3 c2, vec3 c3, vec3 c4) {
  if (g < 0.33) return mix(c1, c2, g * 3.0);
  if (g < 0.66) return mix(c2, c3, (g - 0.33) * 3.0);
  return mix(c3, c4, (g - 0.66) * 3.0);
}
vec2 hash2(vec2 p) { return fract(sin(vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)))) * 43758.5453); }
float noise2D(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float n = mix(mix(dot(hash2(i), f), dot(hash2(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0)), u.x),
                mix(dot(hash2(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0)), dot(hash2(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0)), u.x), u.y);
  return 0.5 + 0.5 * n;
}
float sharpRing(vec3 d, float time) {
  float noise = mix(noise2D(vec2(d.x, time) * 5.0), noise2D(vec2(d.y, time) * 5.0), d.z);
  return 1.0 + (noise - 0.5) * 2.5 * 0.3 * 1.5;
}
float smoothRing(vec3 d, float time) {
  float noise = mix(noise2D(vec2(d.x, time) * 6.0), noise2D(vec2(d.y, time) * 6.0), d.z);
  return 0.9 + (noise - 0.5) * 5.0 * 0.2;
}
float flow(vec3 d, float time) {
  return mix(texture(uPerlinTexture, vec2(time, d.x / 2.0)).r, texture(uPerlinTexture, vec2(time, d.y / 2.0)).r, d.z);
}
void main() {
  vec2 uv = vUv;
  float radius = length(uv);
  float theta = atan(uv.y, uv.x);
  if (theta < 0.0) theta += 2.0 * PI;
  vec3 decomposed = vec3(theta / (2.0 * PI), mod(theta / (2.0 * PI) + 0.5, 1.0) + 1.0, abs(theta / PI - 1.0));
  float n = flow(decomposed, radius * 0.03 - uAnimation * 0.2) - 0.5;
  theta += n * mix(0.08, 0.25, uOutputVolume);
  vec4 color = vec4(1.0);
  float originalCenters[7] = float[7](0.0, 0.5 * PI, 1.0 * PI, 1.5 * PI, 2.0 * PI, 2.5 * PI, 3.0 * PI);
  vec4 ovalColor;
  for (int i = 0; i < 7; i++) {
    float center = originalCenters[i] + 0.5 * sin(uTime / 20.0 + uOffsets[i]);
    float noise = texture(uPerlinTexture, vec2(mod(center + uTime * 0.05, 1.0), 0.5)).r;
    float a = 0.5 + noise * 0.3;
    float b = noise * mix(3.5, 2.5, uInputVolume);
    float distTheta = min(abs(theta - center), min(abs(theta + 2.0 * PI - center), abs(theta - 2.0 * PI - center)));
    if (drawOval(vec2(distTheta, radius), a, b, i % 2 == 1, 0.6, ovalColor)) {
      color.rgb = mix(color.rgb, ovalColor.rgb, ovalColor.a);
      color.a = max(color.a, ovalColor.a);
    }
  }
  float ringRadius1 = sharpRing(decomposed, uTime * 0.1);
  float ringRadius2 = smoothRing(decomposed, uTime * 0.1);
  float inputRadius1 = radius + uInputVolume * 0.2;
  float inputRadius2 = radius + uInputVolume * 0.15;
  float ringAlpha1 = (inputRadius2 >= ringRadius1) ? mix(0.2, 0.6, uInputVolume) : 0.0;
  float ringAlpha2 = smoothstep(ringRadius2 - 0.05, ringRadius2 + 0.05, inputRadius1) * mix(0.15, 0.45, uInputVolume);
  float ring = max(ringAlpha1, ringAlpha2);
  color.rgb = 1.0 - (1.0 - color.rgb) * (1.0 - vec3(ring));
  float luminance = mix(color.r, 1.0 - color.r, uInverted);
  color.rgb = colorRamp(luminance, uColor1 * 0.35, uColor1, uColor2, mix(uColor2, vec3(1.0), 0.6));
  float inside = 1.0 - smoothstep(1.0 - uEdge, 1.0, radius);
  fragColor = vec4(color.rgb * inside, inside);
}`;

/** A tileable smooth noise, the grey ramp the orb's ovals and flow are sampled from. */
function noiseTexture(gl: WebGL2RenderingContext) {
  const size = 256;
  const cells = 8;
  const rand = (x: number, y: number) => {
    const s = Math.sin(((x % cells) + cells) % cells * 127.1 + (((y % cells) + cells) % cells) * 311.7) * 43758.5453;
    return s - Math.floor(s);
  };
  const smooth = (t: number) => t * t * (3 - 2 * t);
  const values = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let v = 0;
      let amp = 0.5;
      for (let octave = 0; octave < 4; octave++) {
        const scale = cells * 2 ** octave;
        const fx = (x / size) * scale;
        const fy = (y / size) * scale;
        const ix = Math.floor(fx);
        const iy = Math.floor(fy);
        const r = (a: number, b: number) => rand(a % scale, b % scale);
        const u = smooth(fx - ix);
        const w = smooth(fy - iy);
        const top = r(ix, iy) * (1 - u) + r(ix + 1, iy) * u;
        const bottom = r(ix, iy + 1) * (1 - u) + r(ix + 1, iy + 1) * u;
        v += amp * (top * (1 - w) + bottom * w);
        amp /= 2;
      }
      values[y * size + x] = v;
    }
  }
  // the shader was tuned on a soft noise: mean 0.5, spread 0.1; a stronger one cuts the ovals into hard petals
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const spread = Math.sqrt(values.reduce((a, b) => a + (b - mean) ** 2, 0) / values.length);
  const data = new Uint8Array(size * size * 4);
  values.forEach((v, i) => {
    data[i * 4] = data[i * 4 + 1] = data[i * 4 + 2] = Math.round(Math.min(1, Math.max(0, 0.5 + ((v - mean) / spread) * 0.104)) * 255);
    data[i * 4 + 3] = 255;
  });
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, size, size, 0, gl.RGBA, gl.UNSIGNED_BYTE, data);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  return tex;
}

const clamp01 = (n: number) => Math.min(1, Math.max(0, Number.isFinite(n) ? n : 0));

/** The talk screen's orb: soft coloured ovals turning inside a ring that swells with the person's voice and flows
 * faster with the agent's. The shader is ElevenLabs UI's Orb (MIT), drawn here without three.js. */
export default function Orb({ state, level, token, className }: VisualProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const stateRef = useRef(state);
  const levelRef = useRef(level);
  const tokenRef = useRef(token);
  stateRef.current = state;
  levelRef.current = level;
  tokenRef.current = token;

  useEffect(() => {
    const el = canvas.current;
    const gl = el?.getContext("webgl2", { premultipliedAlpha: true, alpha: true, antialias: true });
    const probe = document.createElement("canvas").getContext("2d");
    if (!el || !gl || !probe) return;
    const radius = 0.912;
    const program = fullscreenProgram(gl, VERTEX.replace("RADIUS", radius.toFixed(3)), FRAGMENT);
    gl.activeTexture(gl.TEXTURE0);
    noiseTexture(gl);
    const u = (name: string) => gl.getUniformLocation(program, name);
    gl.uniform1i(u("uPerlinTexture"), 0);
    gl.uniform1fv(u("uOffsets"), new Float32Array(Array.from({ length: 7 }, (_, i) => ((i * 0.6180339 + 0.31) % 1) * Math.PI * 2)));
    const loc = { time: u("uTime"), anim: u("uAnimation"), inv: u("uInverted"), input: u("uInputVolume"), output: u("uOutputVolume"), edge: u("uEdge"), c1: u("uColor1"), c2: u("uColor2") };
    gl.clearColor(0, 0, 0, 0);

    const calm = window.matchMedia("(prefers-reduced-motion: reduce)");
    let theme = readTheme(tokenRef.current, probe);
    let soft = readTheme("--color-accent-2", probe);
    let looked = performance.now();
    let time = 0;
    let animation = 0.1;
    let speed = 0.1;
    let curIn = 0;
    let curOut = 0;
    let last = performance.now();
    let frame = 0;
    const draw = (now: number) => {
      const dt = Math.min(0.1, (now - last) / 1000) * (calm.matches ? 0.3 : 1);
      last = now;
      if (fitCanvas(el, gl)) gl.uniform1f(loc.edge, 2 / (el.height * radius / 2));
      if (now - looked > 500) { theme = readTheme(tokenRef.current, probe); soft = readTheme("--color-accent-2", probe); looked = now; }
      time += dt * 0.5;
      const t = time * 2;
      const loud = clamp01(levelRef.current());
      let targetIn = 0;
      let targetOut = 0.3;
      switch (stateRef.current) {
        case "listening": targetIn = clamp01(0.55 + Math.sin(t * 3.2) * 0.35) * 0.6; targetOut = 0.45; break;
        case "hearing": targetIn = clamp01(0.35 + loud * 0.65); targetOut = 0.45; break;
        case "speaking": targetIn = clamp01(0.55 + Math.sin(t * 4.8) * 0.15); targetOut = clamp01(0.45 + loud * 0.55); break;
        case "thinking": targetIn = clamp01(0.38 + 0.07 * Math.sin(t * 0.7) + 0.05 * Math.sin(t * 2.1) * Math.sin(t * 0.37 + 1.2)); targetOut = clamp01(0.48 + 0.12 * Math.sin(t * 1.05 + 0.6)); break;
        case "off": break;
      }
      curIn += (targetIn - curIn) * 0.2;
      curOut += (targetOut - curOut) * 0.2;
      speed += (0.1 + (1 - (curOut - 1) ** 2) * 0.9 - speed) * 0.12;
      animation += dt * speed;
      gl.uniform1f(loc.time, time);
      gl.uniform1f(loc.anim, animation);
      gl.uniform1f(loc.inv, 0);
      gl.uniform1f(loc.input, curIn);
      gl.uniform1f(loc.output, curOut);
      gl.uniform3fv(loc.c1, soft.color);
      gl.uniform3fv(loc.c2, theme.color);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => { cancelAnimationFrame(frame); gl.getExtension("WEBGL_lose_context")?.loseContext(); };
  }, []);

  return <canvas ref={canvas} className={className} aria-hidden />;
}
