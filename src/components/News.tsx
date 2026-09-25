import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { NewsItem } from "../types";

const ROTATE_MS = 6000;

interface Props {
  items: NewsItem[];
  onError: (message: string) => void;
}

/** "Sat 20:00" style, or empty if the bound isn't set/parsable. */
function fmt(iso: string): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString(undefined, {
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function dateRange(item: NewsItem): string {
  const start = fmt(item.starts_at);
  const end = fmt(item.ends_at);
  if (start && end) return `${start} – ${end}`;
  return start || end;
}

/** CSS `url(...)` needs its own quoting, not JS's. */
function cssUrl(src: string): string {
  return `url("${src.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}")`;
}

// Tags are now free text from the feed rather than a fixed category, so the
// color can't be a lookup table anymore — hash the tag string instead. Same
// tag always lands on the same color, so "Patch Notes" stays consistent
// across items without anyone having to configure it.
const TAG_PALETTE = [
  { bg: "var(--amber)", fg: "#0b0b0c" },
  { bg: "var(--signal)", fg: "#fff" },
  { bg: "var(--paper)", fg: "#0b0b0c" },
  { bg: "#4aa3d8", fg: "#0b0b0c" },
];

function tagColor(tag: string): { bg: string; fg: string } {
  let hash = 0;
  for (let i = 0; i < tag.length; i++) {
    hash = (hash * 31 + tag.charCodeAt(i)) | 0;
  }
  return TAG_PALETTE[Math.abs(hash) % TAG_PALETTE.length];
}

export function News({ items, onError }: Props) {
  const [current, setCurrent] = useState(0);
  const timer = useRef<number | null>(null);

  const go = useCallback(
    (next: number) => {
      if (items.length === 0) return;
      setCurrent(((next % items.length) + items.length) % items.length);
    },
    [items.length],
  );

  // Restart the rotation on every manual interaction, so clicking a dot does
  // not leave you two seconds away from an automatic jump.
  useEffect(() => {
    if (items.length < 2) return;
    if (timer.current) window.clearInterval(timer.current);
    timer.current = window.setInterval(() => {
      setCurrent((c) => (c + 1) % items.length);
    }, ROTATE_MS);
    return () => {
      if (timer.current) window.clearInterval(timer.current);
    };
  }, [items.length, current]);

  if (items.length === 0) {
    return (
      <div className="news">
        <div className="news__viewport">
          <article className="slide is-active">
            <div className="slide__body">
              <h2 className="slide__title">No news</h2>
              <p className="slide__text">Nothing to report right now.</p>
            </div>
          </article>
        </div>
      </div>
    );
  }

  return (
    <div className="news">
      <button className="news__arrow" title="Previous" type="button" onClick={() => go(current - 1)}>
        <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"
             strokeLinecap="round" strokeLinejoin="round">
          <path d="M7.5 1.5L3 6l4.5 4.5" />
        </svg>
      </button>

      <div className="news__viewport">
        {items.map((item, i) => {
          const meta = dateRange(item);
          const clickable = item.clickable && !!item.url;
          return (
            <article
              key={item.id}
              className={`slide${i === current ? " is-active" : ""}${clickable ? " is-clickable" : ""}`}
              style={item.image ? { backgroundImage: cssUrl(item.image) } : undefined}
              role={clickable ? "button" : undefined}
              tabIndex={clickable ? 0 : undefined}
              onClick={
                clickable
                  ? () => openUrl(item.url as string).catch((err) => onError(`Could not open link: ${String(err)}`))
                  : undefined
              }
              onKeyDown={
                clickable
                  ? (e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        openUrl(item.url as string).catch((err) => onError(`Could not open link: ${String(err)}`));
                      }
                    }
                  : undefined
              }
            >
              <span className="slide__scrim" />
              <span className="slide__art">
                <span /><span /><span />
              </span>
              <span className="slide__tag" style={{ background: tagColor(item.tag).bg, color: tagColor(item.tag).fg }}>
                {item.tag}
              </span>
              <span className="slide__index">
                <b>{String(i + 1).padStart(2, "0")}</b> / {String(items.length).padStart(2, "0")}
              </span>
              <div className="slide__body">
                <h2 className="slide__title">{item.title}</h2>
                {meta && <p className="slide__meta">{meta}</p>}
                <p className="slide__text">{item.description}</p>
              </div>
            </article>
          );
        })}

        <span className="brackets"><i /><i /></span>

        <div className="news__dots">
          {items.map((item, i) => (
            <button
              key={item.id}
              className={`dot${i === current ? " is-active" : ""}`}
              type="button"
              aria-label={`Slide ${i + 1}`}
              onClick={() => go(i)}
            />
          ))}
        </div>
      </div>

      <button className="news__arrow" title="Next" type="button" onClick={() => go(current + 1)}>
        <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"
             strokeLinecap="round" strokeLinejoin="round">
          <path d="M4.5 1.5L9 6l-4.5 4.5" />
        </svg>
      </button>
    </div>
  );
}
