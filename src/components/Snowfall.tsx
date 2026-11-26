/** The falling flakes, shared by the Christmas scenery (ChristmasDecor) and
 * the plain snow layer (SnowLayer): one set of flakes, one set of keyframes
 * (christmas-fall in index.css). `className` names the clipped layer that
 * holds them, which decides where they sit and what colour they are. */

const FLAKES = Array.from({ length: 20 }, (_, i) => {
  const duration = 9 + ((i * 7) % 10);
  return {
    left: `${((i * 61.8) % 100).toFixed(1)}%`,
    size: 2 + (i % 3),
    duration: `${duration}s`,
    delay: `${-((i * 1.7) % duration).toFixed(1)}s`,
    drift: `${((i % 5) - 2) * 14}px`,
    opacity: (0.5 + (i % 4) * 0.12).toFixed(2),
  };
});

export default function Snowfall({ className }: { className: string }) {
  return (
    <div className={className} aria-hidden="true">
      {FLAKES.map((f, i) => (
        <span
          key={i}
          className="christmas-flake"
          style={
            {
              left: f.left,
              width: f.size,
              height: f.size,
              opacity: f.opacity,
              animationDuration: f.duration,
              animationDelay: f.delay,
              "--drift": f.drift,
            } as React.CSSProperties
          }
        />
      ))}
    </div>
  );
}
