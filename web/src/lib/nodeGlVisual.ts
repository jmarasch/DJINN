// The node visualizer's shader styles (WebGL), the same pictures as DJINN GO
// for Android's Warp, Tunnel, Plasma, Kaleidoscope, Nebula and Stars
// (`GlVisuals.kt`): change both. The node sends levels, not the waveform, so
// Warp's ring follows the bands where Android's follows the wave.

import type { GlStyle, Smoother } from './nodeVisual'

const VERTEX = `
attribute vec2 aPos;
varying vec2 vUv;
void main() {
    vUv = aPos * 0.5 + 0.5;
    gl_Position = vec4(aPos, 0.0, 1.0);
}
`

/** Shared by every picture: the sound as uniforms, and a few tools. */
const HEADER = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying vec2 vUv;
uniform float uTime;
uniform vec2 uRes;
uniform float uBass;
uniform float uMid;
uniform float uTreble;
uniform float uBeat;
uniform float uSlowBass;
uniform float uSlowMid;
uniform float uSlowTreble;
uniform float uTravel;
uniform float uHue;
uniform sampler2D uBands;
uniform sampler2D uPrev;
const float TAU = 6.2831853;

vec3 palette(float t) {
    return 0.5 + 0.5 * cos(TAU * (t + uHue + vec3(0.72, 0.52, 0.20)));
}
float band(float x) { return texture2D(uBands, vec2(clamp(x, 0.0, 1.0), 0.5)).r; }
float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x),
               mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
vec2 centered() {
    vec2 p = vUv * 2.0 - 1.0;
    p.x *= uRes.x / uRes.y;
    return p;
}
`

/** The last frame, turned and pulled in a little, with the sound drawn on
 * top: the trails and swirls come from feeding each frame into the next. */
const WARP = `
void main() {
    vec2 p = vUv - 0.5;
    float aspect = uRes.x / uRes.y;
    p.x *= aspect;
    float r = length(p);
    float a = atan(p.y, p.x);
    float spin = 0.012 * sin(uTime * 0.21) + 0.018 * uMid * sin(uTime * 0.53 + r * 5.0);
    a += spin;
    float zoom = 0.985 - 0.035 * uBass - 0.02 * uBeat + 0.006 * sin(a * 5.0 + uTime * 0.7) * uTreble;
    r *= zoom;
    vec2 q = vec2(cos(a), sin(a)) * r;
    q += 0.004 * vec2(sin(uTime * 0.31 + q.y * 9.0), cos(uTime * 0.37 + q.x * 9.0));
    q.x /= aspect;
    vec3 c = texture2D(uPrev, q + 0.5).rgb * (0.955 + 0.03 * uBass);

    vec2 s = centered();
    float angle = atan(s.y, s.x) / TAU + 0.5;
    float radius = length(s);
    float level = band(abs(angle - 0.5) * 2.0);
    float ring = 0.34 + 0.12 * uBass + 0.16 * (level * level - 0.3);
    float line = smoothstep(0.022, 0.0, abs(radius - ring));
    c += line * palette(uTime * 0.05 + angle) * (0.55 + uBeat);
    float flash = smoothstep(0.4, 0.0, radius) * uBeat * 0.16;
    c += flash * palette(uTime * 0.09 + 0.5);
    gl_FragColor = vec4(min(c, vec3(1.0)), 1.0);
}
`

const TUNNEL = `
void main() {
    vec2 p = centered();
    float r = length(p) + 0.001;
    float a = atan(p.y, p.x) / TAU;
    float depth = 0.45 / r + uTravel;
    float twist = a * 6.0 + depth * 0.5 + uSlowMid * 0.8;
    float rings = 0.5 + 0.5 * sin(depth * 8.0 + uSlowBass * 3.0);
    float slats = 0.5 + 0.5 * sin(twist * TAU * 1.0);
    float level = band(abs(a) * 2.0);
    vec3 c = palette(depth * 0.09 + 0.15 * sin(a * TAU)) * (rings * 0.55 + slats * 0.35 + level * 0.8);
    c *= smoothstep(0.06, 0.5, r);
    c += palette(uTime * 0.11) * smoothstep(0.32, 0.0, r) * (0.25 + uBeat * 0.8);
    gl_FragColor = vec4(c, 1.0);
}
`

const PLASMA = `
void main() {
    vec2 p = centered() * (1.6 + uSlowBass * 0.7);
    float t = uTime * 0.45;
    float v = sin(p.x * 2.1 + t) + sin(p.y * 2.7 - t * 1.3)
        + sin((p.x + p.y) * 1.7 + t * 0.7) + sin(length(p) * 3.3 - t * 1.9 + uSlowMid * 3.0);
    v += 0.8 * sin(p.x * 5.0 * uSlowTreble + p.y * 3.0 + t);
    vec3 c = palette(v * 0.17 + uTime * 0.03);
    c *= 0.55 + 0.45 * sin(v * 2.2 + uBeat * 2.0);
    c += 0.28 * uBeat * palette(v * 0.3);
    gl_FragColor = vec4(c, 1.0);
}
`

const KALEIDOSCOPE = `
void main() {
    vec2 p = centered();
    float r = length(p);
    float a = atan(p.y, p.x);
    float slices = 8.0;
    float side = TAU / slices;
    a = mod(a + uTime * 0.08, side);
    a = abs(a - side * 0.5);
    vec2 q = r * vec2(cos(a), sin(a));
    q *= 1.4 + 0.6 * uSlowBass;
    q += vec2(uTime * 0.11, sin(uTime * 0.13));
    float n = noise(q * 3.0) * 0.6 + noise(q * 7.0 + uTime * 0.2) * 0.3 + noise(q * 15.0) * 0.1;
    float level = band(r * 0.7);
    float edge = smoothstep(0.02, 0.0, abs(fract(n * 4.0 + uSlowMid) - 0.5) - 0.2);
    vec3 c = palette(n + r * 0.3 + uTime * 0.04) * (0.25 + edge * 0.7 + level * 0.75);
    c *= smoothstep(1.7, 0.2, r);
    c += uBeat * 0.2 * palette(r + uTime * 0.1);
    gl_FragColor = vec4(c, 1.0);
}
`

const NEBULA = `
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 4; i++) {
        v += a * noise(p);
        p = p * 2.03 + vec2(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}
void main() {
    vec2 p = centered() * 1.3;
    float t = uTime * 0.07;
    vec2 w = vec2(fbm(p + vec2(t, -t)), fbm(p + vec2(5.2 - t, 1.3 + t)));
    float n = fbm(p + 2.2 * w + uSlowBass * 0.5);
    vec3 c = palette(n * 0.9 + w.x * 0.4 + uTime * 0.015) * smoothstep(0.15, 0.65, n);
    c *= 0.9 + uSlowMid * 1.1;
    float glow = smoothstep(1.1, 0.0, length(p));
    c += glow * glow * 0.3 * uSlowBass * palette(uTime * 0.04);
    c += uBeat * 0.14 * palette(n);
    gl_FragColor = vec4(c, 1.0);
}
`

const STARS = `
vec3 layer(vec2 p, float z, float pixel) {
    vec2 cell = floor(p);
    vec2 f = fract(p) - 0.5;
    float id = hash(cell + z);
    vec2 at = vec2(hash(cell + 1.3), hash(cell + 7.7)) - 0.5;
    float d = length(f - at * 0.7);
    float size = max(0.02 + 0.05 * id * (0.5 + uSlowTreble), pixel * 2.5);
    return palette(id + uTime * 0.02) * smoothstep(size, 0.0, d);
}
void main() {
    vec2 p = centered();
    vec3 c = vec3(0.0);
    for (int i = 0; i < 5; i++) {
        float z = fract(float(i) / 5.0 + uTravel);
        float scale = mix(9.0, 0.6, z);
        float fade = smoothstep(0.0, 0.35, z) * smoothstep(1.0, 0.8, z);
        float pixel = scale * 2.0 / min(uRes.x, uRes.y);
        c += layer(p * scale + float(i) * 3.7, float(i), pixel) * fade * 1.6;
    }
    float r = length(p);
    c += palette(uTime * 0.03 + r) * smoothstep(0.9, 0.0, r) * (0.07 + 0.3 * uSlowBass);
    c += uBeat * 0.12 * palette(uTime * 0.08);
    gl_FragColor = vec4(c, 1.0);
}
`

/** Shows the smaller picture on screen. */
const SHOW = `
precision mediump float;
varying vec2 vUv;
uniform sampler2D uPrev;
void main() { gl_FragColor = texture2D(uPrev, vUv); }
`

const SOURCES: Record<GlStyle, string> = {
  warp: WARP,
  tunnel: TUNNEL,
  plasma: PLASMA,
  kaleidoscope: KALEIDOSCOPE,
  nebula: NEBULA,
  stars: STARS,
}

/** The biggest side the picture is drawn at, then stretched; smaller if the
 * box can't keep up. */
const PICTURE_MAX = 720
const PICTURE_SMALL = 480
/** Frames a second below which a picture is too much for this box. */
const MIN_FPS = 20
const SETTLE_MS = 1500
const WINDOW_MS = 4000

const UNIFORMS = [
  'uTime',
  'uRes',
  'uBass',
  'uMid',
  'uTreble',
  'uBeat',
  'uSlowBass',
  'uSlowMid',
  'uSlowTreble',
  'uTravel',
  'uHue',
  'uBands',
  'uPrev',
] as const

interface Program {
  id: WebGLProgram
  pos: number
  at: Record<(typeof UNIFORMS)[number], WebGLUniformLocation | null>
}

/** Draws one shader style on a canvas. `draw` returns false once this box
 * has shown it can't keep up, or if the style won't build; the caller then
 * falls back to a shape style. */
export class GlVisual {
  /** Colors turn slowly (a look's "rainbow"). */
  rainbow = false
  private gl: WebGLRenderingContext
  private program: Program
  private show: Program
  private quad: WebGLBuffer
  private bands: WebGLTexture
  private frames: { buffer: WebGLFramebuffer; texture: WebGLTexture }[] = []
  private front = 0
  private size = { w: 0, h: 0 }
  private largest = PICTURE_MAX
  private started = performance.now()
  // Pace watching, as Android's FrameWatch.
  private watchFrom = 0
  private watchFrames = 0
  private lastFrame = 0
  private shrunk = false
  private gaveUp = false

  constructor(
    private canvas: HTMLCanvasElement,
    private style: GlStyle,
  ) {
    const gl = canvas.getContext('webgl', { alpha: false, antialias: false, preserveDrawingBuffer: false })
    if (!gl) throw new Error('no WebGL')
    this.gl = gl
    this.program = this.link(HEADER + SOURCES[style])
    this.show = this.link(SHOW)
    const quad = gl.createBuffer()
    const bands = gl.createTexture()
    if (!quad || !bands) throw new Error('WebGL ran out')
    this.quad = quad
    gl.bindBuffer(gl.ARRAY_BUFFER, quad)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW)
    this.bands = bands
    gl.bindTexture(gl.TEXTURE_2D, bands)
    this.textureRules()
    gl.disable(gl.DEPTH_TEST)
    gl.disable(gl.BLEND)
  }

  private textureRules() {
    const gl = this.gl
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  }

  private compile(type: number, source: string): WebGLShader {
    const gl = this.gl
    const shader = gl.createShader(type)
    if (!shader) throw new Error('WebGL ran out')
    gl.shaderSource(shader, source)
    gl.compileShader(shader)
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(`${this.style} won't build: ${gl.getShaderInfoLog(shader)}`)
    }
    return shader
  }

  private link(fragment: string): Program {
    const gl = this.gl
    const id = gl.createProgram()
    if (!id) throw new Error('WebGL ran out')
    gl.attachShader(id, this.compile(gl.VERTEX_SHADER, VERTEX))
    gl.attachShader(id, this.compile(gl.FRAGMENT_SHADER, fragment))
    gl.linkProgram(id)
    if (!gl.getProgramParameter(id, gl.LINK_STATUS)) throw new Error(`${this.style} won't link`)
    const at = Object.fromEntries(UNIFORMS.map((name) => [name, gl.getUniformLocation(id, name)])) as Program['at']
    return { id, pos: gl.getAttribLocation(id, 'aPos'), at }
  }

  /** The two pictures Warp takes turns drawing into and reading from; the
   * others draw into one, smaller than the screen, to stay smooth. */
  private buildFrames(width: number, height: number) {
    const gl = this.gl
    for (const frame of this.frames) {
      gl.deleteFramebuffer(frame.buffer)
      gl.deleteTexture(frame.texture)
    }
    const scale = Math.min(1, this.largest / Math.max(width, height))
    const w = Math.max(1, Math.round(width * scale))
    const h = Math.max(1, Math.round(height * scale))
    this.frames = [0, 1].map(() => {
      const texture = gl.createTexture()
      const buffer = gl.createFramebuffer()
      if (!texture || !buffer) throw new Error('WebGL ran out')
      gl.bindTexture(gl.TEXTURE_2D, texture)
      this.textureRules()
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null)
      gl.bindFramebuffer(gl.FRAMEBUFFER, buffer)
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0)
      gl.clearColor(0, 0, 0, 1)
      gl.clear(gl.COLOR_BUFFER_BIT)
      return { buffer, texture }
    })
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    this.size = { w, h }
  }

  private quadWith(program: Program, w: number, h: number, sound: Smoother, seconds: number, previous: WebGLTexture) {
    const gl = this.gl
    const u = program.at
    gl.useProgram(program.id)
    gl.viewport(0, 0, w, h)
    gl.uniform1f(u.uTime, seconds)
    gl.uniform2f(u.uRes, w, h)
    gl.uniform1f(u.uBass, sound.bass)
    gl.uniform1f(u.uMid, sound.mid)
    gl.uniform1f(u.uTreble, sound.treble)
    gl.uniform1f(u.uBeat, sound.beat)
    gl.uniform1f(u.uSlowBass, sound.slowBass)
    gl.uniform1f(u.uSlowMid, sound.slowMid)
    gl.uniform1f(u.uSlowTreble, sound.slowTreble)
    gl.uniform1f(u.uHue, this.rainbow ? ((performance.now() - this.started) / 30_000) % 1 : 0)
    gl.uniform1f(u.uTravel, this.style === 'tunnel' ? sound.tunnel : this.style === 'stars' ? sound.stars : 0)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, previous)
    gl.uniform1i(u.uPrev, 0)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.bands)
    gl.uniform1i(u.uBands, 1)
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quad)
    gl.enableVertexAttribArray(program.pos)
    gl.vertexAttribPointer(program.pos, 2, gl.FLOAT, false, 0, 0)
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
  }

  /** Whether this box keeps up: first a smaller picture, then giving up.
   * Gaps (the page hidden, a shader being built) don't count. */
  private keepingUp(now: number): boolean {
    if (!this.lastFrame || now - this.lastFrame > 1000) {
      this.watchFrom = now + SETTLE_MS
      this.watchFrames = 0
    }
    this.lastFrame = now
    if (now < this.watchFrom) return true
    this.watchFrames++
    const took = now - this.watchFrom
    if (took < WINDOW_MS) return true
    const fps = (this.watchFrames * 1000) / took
    this.watchFrom = now + SETTLE_MS
    this.watchFrames = 0
    if (fps >= MIN_FPS) return true
    if (this.shrunk) return false
    this.shrunk = true
    this.largest = PICTURE_SMALL
    this.size = { w: 0, h: 0 }
    return true
  }

  /** Frees what it put on the graphics chip, keeping the canvas usable for
   * the next style. */
  dispose() {
    const gl = this.gl
    for (const frame of this.frames) {
      gl.deleteFramebuffer(frame.buffer)
      gl.deleteTexture(frame.texture)
    }
    this.frames = []
    gl.deleteTexture(this.bands)
    gl.deleteBuffer(this.quad)
    gl.deleteProgram(this.program.id)
    gl.deleteProgram(this.show.id)
  }

  /** Lets go of the graphics chip altogether (a page only gets a few at
   * once); the canvas can't draw again after. */
  close() {
    this.dispose()
    this.gl.getExtension('WEBGL_lose_context')?.loseContext()
  }

  /** Draws a frame. False when it's too much for this box. */
  draw(sound: Smoother, now: number): boolean {
    if (this.gaveUp) return false
    if (!this.keepingUp(now)) {
      this.gaveUp = true
      return false
    }
    const gl = this.gl
    const scale = window.devicePixelRatio || 1
    // On screen it fills its box; off screen (thumbnails) it keeps its size.
    const shown = this.canvas.isConnected && this.canvas.clientWidth > 0
    const width = shown ? Math.max(1, Math.round(this.canvas.clientWidth * scale)) : this.canvas.width
    const height = shown ? Math.max(1, Math.round(this.canvas.clientHeight * scale)) : this.canvas.height
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width
      this.canvas.height = height
      this.size = { w: 0, h: 0 }
    }
    if (!this.size.w) this.buildFrames(width, height)

    const bytes = new Uint8Array(sound.bands.length || 1)
    sound.bands.forEach((b, i) => (bytes[i] = Math.round(Math.min(1, Math.max(0, b)) * 255)))
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.bands)
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, bytes.length, 1, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, bytes)

    // Kept small so precision holds on chips with little of it.
    const seconds = ((now - this.started) % 3_600_000) / 1000
    const back = 1 - this.front
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.frames[back].buffer)
    this.quadWith(this.program, this.size.w, this.size.h, sound, seconds, this.frames[this.front].texture)
    this.front = back
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    this.quadWith(this.show, width, height, sound, seconds, this.frames[this.front].texture)
    return true
  }
}
