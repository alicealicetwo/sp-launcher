import type { NewsItem } from "./types";

/**
 * Keeps only items whose `[starts_at, ends_at)` window covers `now`. An
 * empty or unparsable bound is treated as "no limit" on that side, so a
 * half-filled-in item still shows instead of silently vanishing.
 */
export function activeNews(items: NewsItem[], now: Date = new Date()): NewsItem[] {
  const t = now.getTime();
  return items.filter((item) => {
    const start = item.starts_at ? Date.parse(item.starts_at) : NaN;
    const end = item.ends_at ? Date.parse(item.ends_at) : NaN;
    if (!Number.isNaN(start) && t < start) return false;
    if (!Number.isNaN(end) && t > end) return false;
    return true;
  });
}
