/** Display helpers. Kept apart so they can be unit-tested without a DOM. */

export function bytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  const value = n / 1024 ** i;
  return `${value.toFixed(value >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

export function rate(bytesPerSec: number): string {
  return `${bytes(bytesPerSec)}/s`;
}

/** "6 min left", "45 s left", or "" when there is nothing sensible to say. */
export function eta(received: number, total: number, bytesPerSec: number): string {
  if (bytesPerSec <= 0 || total <= received) return "";
  const seconds = (total - received) / bytesPerSec;
  if (!Number.isFinite(seconds)) return "";
  if (seconds < 90) return `${Math.ceil(seconds)} s left`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 90) return `${minutes} min left`;
  return `${Math.ceil(minutes / 60)} h left`;
}
