/** Mock for @tauri-apps/api/core — used in vitest when the real Tauri API is unavailable. */
// eslint-disable-next-line @typescript-eslint/no-unused-vars
export async function invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  return undefined;
}
