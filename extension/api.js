// Client for Prism Reference Manager's local connection on this computer (127.0.0.1 only).

export const DEFAULT_PORT = 23119;

export class ApiError extends Error {
  constructor(message, kind) {
    super(message);
    this.kind = kind; // offline, unauthorized, refused, failed
  }
}

export async function settings() {
  return chrome.storage.local.get({ port: DEFAULT_PORT, token: "", pairId: "" });
}

async function request(path, init = {}) {
  const { port } = await settings();
  let res;
  try {
    res = await fetch(`http://127.0.0.1:${port}${path}`, { ...init, headers: { "Content-Type": "application/json", ...(init.headers || {}) } });
  } catch (_) {
    throw new ApiError("Prism Reference Manager is not running, or local connections are off. Open the program, then turn on Settings › Local connections › Allow local connections.", "offline");
  }
  let body = {};
  try { body = await res.json(); } catch (_) {}
  if (res.status === 401) {
    await chrome.storage.local.set({ token: "" });
    throw new ApiError("This browser is no longer connected (the connection was revoked). Connect again.", "unauthorized");
  }
  if (res.status === 403) throw new ApiError(body.error || "The program refused this request.", "refused");
  if (!res.ok) throw new ApiError(body.error || `The program answered with an error (${res.status}).`, "failed");
  return body;
}

/** Ask the program for a connection; the person approves it in the program's window. */
export async function startPairing() {
  const r = await request("/v1/pair", { method: "POST", body: JSON.stringify({ name: "Browser capture extension", scopes: ["read", "import"] }) });
  await chrome.storage.local.set({ pairId: r.id, token: "" });
  return r.id;
}

/** Returns "pending", "approved" or "denied"; stores the token once approved. */
export async function pairingState() {
  const { pairId } = await settings();
  if (!pairId) return "none";
  const r = await request(`/v1/pair/${encodeURIComponent(pairId)}`);
  if (r.state === "approved" && r.token) {
    await chrome.storage.local.set({ token: r.token, pairId: "" });
    return "approved";
  }
  if (r.state === "denied") await chrome.storage.local.set({ pairId: "" });
  return r.state;
}

export async function call(method, args = {}) {
  const { token } = await settings();
  if (!token) throw new ApiError("Connect this browser to Prism Reference Manager first.", "unauthorized");
  const r = await request("/v1/call", { method: "POST", headers: { Authorization: `Bearer ${token}` }, body: JSON.stringify({ method, args }) });
  return r.result;
}
