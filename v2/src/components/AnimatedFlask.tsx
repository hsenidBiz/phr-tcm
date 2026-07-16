import EqRing from "./EqRing";

/** The sign-in screen's animated flask mark: the outline draws itself on,
 * then floats with a breathing glow and bubbles rising out of the neck.
 * Pure CSS (see .login-flask in index.css), theme-accent tinted, stilled
 * under prefers-reduced-motion. When system audio is playing, the outer
 * ring doubles as a circular equalizer. */
export default function AnimatedFlask({ size = 112 }: { size?: number }) {
  return (
    <div
      className="login-flask"
      aria-hidden
      style={{ width: size * 1.9, height: size * 1.9 }}
    >
      <span className="glow" />
      <EqRing radius={size * 0.75} maxLen={size * 0.18} />
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path className="fl-top" d="M9 3h6" />
        <path className="fl-body" d="M10 3v6l-5 9a2 2 0 0 0 1.8 3h10.4a2 2 0 0 0 1.8-3l-5-9V3" />
        <path className="fl-line" d="M7.5 14h9" />
      </svg>
      <span className="bubble b1" />
      <span className="bubble b2" />
      <span className="bubble b3" />
      <span className="bubble b4" />
    </div>
  );
}
