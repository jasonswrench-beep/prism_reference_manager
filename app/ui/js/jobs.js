// Background jobs in the window: each long task reports progress, checks in between items so it
// can be paused, resumed or cancelled, keeps a short log, and can offer Retry when it finishes
// with failures or is cancelled. The job panel (Tools › Background jobs) lists them.

export const JOBS = [];
const listeners = new Set();
let seq = 0;

export class Cancelled extends Error {
  constructor() { super("Cancelled"); this.cancelled = true; }
}

export function onJobs(f) { listeners.add(f); return () => listeners.delete(f); }
function notify() { for (const f of listeners) { try { f(JOBS); } catch (_) {} } }

/**
 * Start a job. `run(job)` does the work and should call `await job.step()` between items, and
 * `job.progress(done, total)`. Options: `pausable` (default true), `onCancel` for work that
 * must be told to stop (such as a running import), and `retry` to start the job again.
 */
export function startJob({ label, source = "", pausable = true, onCancel, retry, run }) {
  let wake = null;
  const job = {
    id: ++seq, label, source, pausable, state: "running", done: 0, total: 0, failed: 0, log: [], started: new Date(), ended: null, retry,
    progress(done, total, note) { this.done = done; if (total !== undefined) this.total = total; if (note) this.note = note; notify(); },
    fail(what) { this.failed++; this.log.push("Failed: " + what); notify(); },
    note_(text) { this.log.push(text); notify(); },
    async step() {
      if (this.state === "cancelling") throw new Cancelled();
      while (this.state === "paused") await new Promise((r) => (wake = r));
      if (this.state === "cancelling") throw new Cancelled();
    },
    pause() { if (this.pausable && this.state === "running") { this.state = "paused"; this.log.push("Paused"); notify(); } },
    resume() { if (this.state === "paused") { this.state = "running"; this.log.push("Resumed"); notify(); wake?.(); } },
    cancel() {
      if (this.state !== "running" && this.state !== "paused") return;
      this.state = "cancelling"; this.log.push("Cancel requested"); notify(); wake?.();
      try { onCancel?.(); } catch (_) {}
    },
    get active() { return this.state === "running" || this.state === "paused" || this.state === "cancelling"; },
  };
  JOBS.push(job);
  if (JOBS.length > 50) JOBS.splice(0, JOBS.findIndex((j) => !j.active) + 1 || 0);
  notify();
  job.promise = (async () => {
    try {
      const r = await run(job);
      job.state = job.state === "cancelling" ? "cancelled" : "done";
      return r;
    } catch (e) {
      if (e?.cancelled || job.state === "cancelling") { job.state = "cancelled"; return null; }
      job.state = "failed"; job.log.push(e?.message || String(e));
      throw e;
    } finally {
      job.ended = new Date();
      job.log.push(job.state === "done" ? `Finished: ${job.done}${job.total ? " of " + job.total : ""} completed${job.failed ? `, ${job.failed} failed` : ""}` : job.state === "cancelled" ? `Cancelled after ${job.done}${job.total ? " of " + job.total : ""}` : "Stopped by an error");
      notify();
    }
  })();
  return job;
}

export function activeJobs() { return JOBS.filter((j) => j.active); }
