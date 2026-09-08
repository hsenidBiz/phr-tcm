/** Whether this machine can create a WebGL context.
 *
 * The sign-in screen's Aurora backdrop is WebGL (via ogl), and ogl's Renderer
 * throws outright when no context is available - which over RDP or on a
 * software-rendered VDI would take out the app's only entry point. Callers
 * gate the decoration on this so sign-in always renders. */
export function hasWebGL(): boolean {
  try {
    const canvas = document.createElement("canvas");
    return Boolean(
      canvas.getContext("webgl2") ||
        canvas.getContext("webgl") ||
        canvas.getContext("experimental-webgl"),
    );
  } catch {
    return false;
  }
}
