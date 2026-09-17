/** Whether this machine can draw WebGL on a GPU.
 *
 * The sign-in screen's Threads backdrop is WebGL (via ogl), and ogl's
 * Renderer throws outright when no context is available - which over RDP
 * or on a software-rendered VDI would take out the app's only entry point.
 * Callers gate the decoration on this so sign-in always renders.
 *
 * A context that exists but is drawn in software counts as "no": WebView2
 * 153 put WebGL on WARP (Microsoft's CPU rasteriser) on a machine with two
 * real GPUs, and the backdrop's per-pixel shader then held every core at
 * 100% until the user signed in. A decoration is never worth that. */
export function hasWebGL(): boolean {
  try {
    const canvas = document.createElement("canvas");
    const gl =
      canvas.getContext("webgl2") ||
      canvas.getContext("webgl") ||
      canvas.getContext("experimental-webgl");
    if (!gl) return false;
    return !isSoftwareRenderer(rendererName(gl as WebGLRenderingContext));
  } catch {
    return false;
  }
}

/** The GPU (or software rasteriser) behind a context, as the browser names
 * it; "" when the debug extension is withheld. */
function rendererName(gl: WebGLRenderingContext): string {
  try {
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    const name = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    return typeof name === "string" ? name : "";
  } catch {
    return "";
  }
}

/** Whether a renderer name is one of the software rasterisers Chromium and
 * WebView2 fall back to: WARP ("Microsoft Basic Render Driver"),
 * SwiftShader, Mesa llvmpipe, or anything calling itself software. */
export function isSoftwareRenderer(name: string): boolean {
  return /swiftshader|warp|basic render driver|llvmpipe|softpipe|software/i.test(name);
}
