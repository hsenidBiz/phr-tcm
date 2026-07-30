// The signed-out screen: animated flask mark, drawn-in title, subtle
// Threads backdrop, and the Microsoft sign-in action. `children` is the
// slot App uses for dev-only extras (kept there so the compile-time
// DEV_TOOLS gate still dead-code-eliminates them from releases).

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import AnimatedFlask from "../components/AnimatedFlask";
import ShinyText from "../components/ShinyText";
import SplitText from "../components/SplitText";
import Threads from "../components/Threads";
import { Button } from "../components/ui/button";
import { hasWebGL } from "../lib/webgl";
import { IconSignIn } from "../lib/actionIcons";

export default function SignIn({
  signingIn,
  onSignIn,
  children,
}: {
  signingIn: boolean;
  onSignIn: () => void;
  children?: ReactNode;
}) {
  // Threads backdrop, tinted to the accent the theme resolved at startup.
  // Stays null without WebGL, which is the signal not to render it at all
  // (RDP, software-rendered VDI).
  const [threadsColor, setThreadsColor] = useState<[number, number, number] | null>(null);
  useEffect(() => {
    if (!hasWebGL()) return;
    const accent = getComputedStyle(document.documentElement)
      .getPropertyValue("--color-accent")
      .trim();
    const m = /^#([0-9a-f]{6})$/i.exec(accent);
    if (!m) return;
    const int = parseInt(m[1], 16);
    setThreadsColor([((int >> 16) & 255) / 255, ((int >> 8) & 255) / 255, (int & 255) / 255]);
  }, []);

  return (
    <div className="relative flex h-full flex-col items-center justify-center">
      {/* -inset-6 cancels <main>'s p-6 so the lines run edge to edge
          instead of stopping at the content padding. */}
      {threadsColor && (
        <div
          aria-hidden
          className="pointer-events-none absolute -inset-6 overflow-hidden opacity-40"
        >
          <Threads color={threadsColor} amplitude={0.8} distance={0} />
        </div>
      )}
      <div className="relative flex flex-col items-center gap-4">
        <AnimatedFlask />
        <SplitText
          text="Test Case Manager"
          tag="h1"
          className="text-xl font-semibold"
          delay={40}
          duration={0.8}
        />
        <p className="max-w-sm text-center text-sm text-muted">
          Sign in with your Microsoft account to manage Azure DevOps test
          cases, runs, and work items.
        </p>
        {/* White on the button, not the on-accent token: ShinyText paints
            itself white so it can shine, and anything following the token
            sat dark beside it. The colour belongs on the button rather than
            the icon - as an icon-only rule it left the plain-text "Waiting
            for browser" label on the token, so the two halves of the same
            button disagreed. This screen is fixed branding either way: the
            flask, the threads and the wordmark are all light on dark
            whatever theme is on. */}
        <Button className="text-white" disabled={signingIn} onClick={onSignIn}>
          <IconSignIn aria-hidden />
          {signingIn ? (
            "Waiting for browser"
          ) : (
            <ShinyText
              text="Sign in with Microsoft"
              speed={3}
              color="rgba(255, 255, 255, 0.85)"
              shineColor="#ffffff"
            />
          )}
        </Button>
        {children}
      </div>
    </div>
  );
}
