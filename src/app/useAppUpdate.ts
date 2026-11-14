import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { commands, events } from "../bindings";
import { toast } from "../lib/toast";
import { detach } from "../lib/detach";

/** How often to look for a new release, in the background. */
const UPDATE_CHECK_MS = 60 * 60 * 1000;

/** The release check and the download-and-restart action behind the banner. */
export function useAppUpdate() {
  // Checked on launch and then quietly once an hour, because this app is
  // left open for days at a time - a launch-only check means a release
  // lands and nobody sees it until they next restart, which for some
  // people is next week.
  //
  // Silent by design: no toast, no spinner, nothing moves. React Query
  // holds the previous answer while a background refetch is in flight, so
  // the only visible effect is the banner below appearing the first time
  // there is genuinely something to say. `refetchIntervalInBackground`
  // keeps it running while the window is minimised, which is exactly when
  // an app left open all week is sitting.
  const update = useQuery({
    queryKey: ["update"],
    queryFn: () => commands.checkUpdate(),
    staleTime: UPDATE_CHECK_MS,
    refetchInterval: UPDATE_CHECK_MS,
    refetchIntervalInBackground: true,
    // A check that fails is not news. It reports itself as `blocked`, the
    // banner stays away, and the next hour tries again.
    retry: false,
  });

  // How far the update package has got. Null until the first event, which
  // is also the first moment the size is known - the backend has to re-ask
  // the feed before it can say how big the download is.
  const [dl, setDl] = useState<{ percent: number; downloaded: number; total: number } | null>(null);

  const applyUpdate = useMutation({
    mutationFn: async () => {
      setDl(null);
      // The byte fields cross as f64, which the bindings widen to
      // `number | null` because a double can be NaN. A byte count cast from
      // a u64 never is - the fallback is here so the display cannot be, not
      // because it is expected to fire.
      const un = await events.updateProgress.listen((e) =>
        setDl({
          percent: e.payload.percent,
          downloaded: e.payload.downloaded ?? 0,
          total: e.payload.total ?? 0,
        }),
      );
      try {
        const r = await commands.applyUpdate();
        if (r.status === "error") throw new Error(r.error);
      } finally {
        // On success the app restarts into the new build, so this only
        // really matters on the failure path - but leaking a listener per
        // failed attempt is how a retry ends up updating state twice.
        detach(un);
      }
    },
    onError: (e) => {
      setDl(null);
      toast.error(`Update failed: ${e.message}`);
    },
  });

  return { update, dl, applyUpdate };
}
