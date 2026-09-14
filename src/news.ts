import type { NewsItem } from "./types";
import playtestsImage from "./assets/news-playtests.jpg";

// Hardcoded for now instead of coming from `news_url` — swap back to the
// fetched feed (drop this and use `news` from `fetch_news` directly) once
// there's a real feed to point at.
export const HARDCODED_NEWS: NewsItem[] = [
  {
    id: "playtests",
    tag: "PLAYTESTS",
    title: "Join our testing sessions",
    description: "More info in Discord.",
    image: playtestsImage,
    starts_at: "",
    ends_at: "",
    clickable: false,
    url: null,
  },
];

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
