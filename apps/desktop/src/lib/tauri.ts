// Unwrap a tauri-specta typedError result. Re-throws on error so call sites
// can keep using try/catch exactly as they did with raw invoke<T>().
export async function unwrap<T>(
  p: Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>
): Promise<T> {
  const r = await p;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}
