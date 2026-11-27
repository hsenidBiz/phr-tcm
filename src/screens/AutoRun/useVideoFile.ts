// What a kept video file offers: its facts, Play (an object URL the page can
// show) and Save (copied where the person chooses). Shared by the video under
// a failed case and by the Recordings card, which has the same file kind.

import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";
import { pickSavePath } from "../../lib/runnerFiles";
import { toast } from "../../lib/toast";

/** The query one video's facts are read through. */
export const videoInfoKey = (name: string) => ["autorun-video-info", name] as const;

/** The bytes of a base64 string, for a Blob. */
function bytesOf(b64: string): Uint8Array<ArrayBuffer> {
  const raw = atob(b64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

/** `name` is the file's name in the run or recordings folder; `saveAs` is the
 * file name the save dialog starts from (the file's own name by default). */
export function useVideoFile(name: string | null | undefined, saveAs?: string) {
  const info = useQuery({
    queryKey: videoInfoKey(name ?? ""),
    queryFn: () => commands.autoRunVideoInfo(name as string),
    enabled: Boolean(name),
    // Read again whenever a list mounts: a file pruned since the last look
    // must show as gone.
    staleTime: 0,
    refetchOnMount: "always",
    retry: false,
  });
  /** The object URL of the video being watched, if it is. */
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => () => {
    if (src) URL.revokeObjectURL(src);
  }, [src]);

  const playing = useMutation({
    mutationFn: async () => {
      const b64 = await unwrapStr(commands.autoRunVideo(name as string));
      return URL.createObjectURL(new Blob([bytesOf(b64)], { type: "video/mp4" }));
    },
    onSuccess: setSrc,
    onError: (e) => toast.error(`Could not play the video: ${e.message}`),
  });
  const saving = useMutation({
    mutationFn: async () => {
      const path = await pickSavePath(saveAs ?? (name as string));
      if (!path) return false;
      await unwrapStr(commands.autoRunSaveVideo(name as string, path));
      return true;
    },
    onSuccess: (saved) => {
      if (saved) toast.success("Saved the video");
    },
    onError: (e) => toast.error(`Could not save the video: ${e.message}`),
  });

  return {
    info,
    src,
    close: () => setSrc(null),
    play: playing,
    save: saving,
  };
}
