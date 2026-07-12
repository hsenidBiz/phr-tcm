import { useMutation, useQuery } from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { useState } from "react";
import { toast } from "sonner";
import { commands } from "../bindings";
import { Button } from "../components/ui/button";
import { getTheme, setTheme, type Theme } from "../lib/theme";

export default function Settings() {
  const [theme, setThemeState] = useState<Theme>(getTheme());

  const version = useQuery({
    queryKey: ["app-version"],
    queryFn: () => getVersion().catch(() => "dev"),
    staleTime: Infinity,
  });

  const check = useMutation({
    mutationFn: () => commands.checkUpdate(),
    onSuccess: (v) =>
      v
        ? toast.info(`Version ${v} is available - use the banner to update.`)
        : toast.success("You are on the latest version."),
  });

  const pick = (t: Theme) => {
    setThemeState(t);
    setTheme(t);
  };

  return (
    <div className="max-w-lg space-y-8">
      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Appearance</h2>
        <div className="flex gap-2">
          {(["light", "dark", "system"] as Theme[]).map((t) => (
            <Button
              key={t}
              size="sm"
              variant={theme === t ? "default" : "outline"}
              onClick={() => pick(t)}
            >
              {t[0].toUpperCase() + t.slice(1)}
            </Button>
          ))}
        </div>
      </section>

      <section className="space-y-3">
        <h2 className="text-sm font-semibold text-text">Updates</h2>
        <p className="text-sm text-muted">
          Version {version.data ?? "..."} - updates install automatically from
          the releases feed.
        </p>
        <Button size="sm" variant="outline" disabled={check.isPending} onClick={() => check.mutate()}>
          {check.isPending ? "Checking..." : "Check for updates"}
        </Button>
      </section>
    </div>
  );
}
