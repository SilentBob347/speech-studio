import i18n from "./i18n";
import { api, JobCancelledError, type JobKind } from "./api";
import { useStore } from "../store";
import { playSfx } from "./sfx";

/** Follows a job of the queue: its progress in the status bar, its end in the activity log; resolves with its result. */
export async function followJob(jobId: string, kind: JobKind): Promise<unknown> {
  const label = i18n.t(`jobs.kind.${kind}`);
  const store = useStore.getState();
  store.setProgress(kind, "", null);
  try {
    const result = await api.watchJob(jobId, (e) => {
      if (e.type === "progress" || e.type === "running" || e.type === "queued") useStore.getState().setProgress(e.stage || kind, e.msg || "", e.pct ?? null);
    });
    useStore.getState().setProgress("done", i18n.t("bridge.jobDone", { kind: label }), null);
    playSfx("success");
    return result;
  } catch (e) {
    if (e instanceof JobCancelledError) {
      useStore.getState().setProgress("done", i18n.t("jobs.cancelledKind", { kind: label }), null);
      throw e;
    }
    const text = e instanceof Error ? e.message : String(e);
    useStore.getState().setProgress("error", i18n.t("bridge.jobFailed", { kind: label, error: text }), null);
    playSfx("error");
    throw e;
  }
}
