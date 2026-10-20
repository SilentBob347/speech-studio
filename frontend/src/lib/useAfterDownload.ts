import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, type DownloadJob } from "./api";
import { useSetupStatus } from "./useSetupStatus";

/** A start the studio refused with `asr_downloading` runs again once the background download of what it lacks is
 * done; `waiting` is the studio's word about it meanwhile, the header shows the download's progress. */
export function useAfterDownload(onError: (e: string) => void) {
  const again = useRef<(() => void) | null>(null);
  const failed = useRef(onError);
  useEffect(() => { failed.current = onError; }, [onError]);
  const [waiting, setWaiting] = useState<string | null>(null);
  const settle = useCallback((job: DownloadJob | null) => {
    const go = again.current;
    if (!go) return;
    again.current = null;
    setWaiting(null);
    if (job?.status === "completed") go();
    else failed.current(job?.error || job?.status || "");
  }, []);
  const { refresh } = useSetupStatus(settle);
  const caught = useCallback((e: unknown, retry: () => void) => {
    if (!(e instanceof ApiError) || e.code !== "asr_downloading") return false;
    again.current = retry;
    setWaiting(e.detail);
    refresh().then((s) => { if (s.active?.status !== "downloading") settle(s.active); }, () => undefined);
    return true;
  }, [refresh, settle]);
  return { waiting, caught };
}
