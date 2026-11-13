// Bridge to the program core. Every action goes through one validated dispatcher.

const T = () => window.__TAURI__;

export async function call(method, args = {}) {
  try {
    return await T().core.invoke("call", { method, args });
  } catch (e) {
    const err = new Error(e && e.message ? e.message : String(e));
    err.detail = e && e.detail;
    throw err;
  }
}

export async function readAttachment(id) {
  const buf = await T().core.invoke("read_attachment", { id });
  return new Uint8Array(buf);
}

export async function readPicked(path) {
  const buf = await T().core.invoke("read_picked", { path });
  return new Uint8Array(buf);
}

export function on(event, fn) {
  return T().event.listen(event, (e) => fn(e.payload));
}
