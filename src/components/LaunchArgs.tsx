import { useEffect, useRef, useState } from "react";

interface Props {
  value: string;
  onSave: (next: string) => void;
}

/**
 * The grey strip above the play button, plus its popover editor.
 * Escape and click-outside discard; only Save commits.
 */
export function LaunchArgs({ value, onSave }: Props) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(value);
  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  // Keep the draft in step when the value changes elsewhere (Settings tab),
  // but never yank the text out from under an open editor.
  useEffect(() => {
    if (!open) setDraft(value);
  }, [value, open]);

  useEffect(() => {
    if (!open) return;

    field.current?.focus();
    field.current?.setSelectionRange(draft.length, draft.length);

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setDraft(value);
        setOpen(false);
      }
    };
    const onClick = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) {
        setDraft(value);
        setOpen(false);
      }
    };

    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onClick);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onClick);
    };
    // `draft` is deliberately absent: re-running on every keystroke would
    // re-focus and reset the caret.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, value]);

  return (
    <div className="argbar" ref={root}>
      <button
        className={`argbar__btn${open ? " is-open" : ""}`}
        aria-expanded={open}
        type="button"
        onClick={() => setOpen((o) => !o)}
      >
        <span className="argbar__gear">
          {/* A proper gear/cog, not the sun-like rays-on-a-circle the old
              icon here read as. */}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
               strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </span>
        <span className="argbar__label">Launch arguments</span>
        <span className="argbar__chev">
          <svg viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.6"
               strokeLinecap="round" strokeLinejoin="round">
            <path d="M1.5 6.5L5 3l3.5 3.5" />
          </svg>
        </span>
      </button>

      <div className={`argpop${open ? " is-open" : ""}`}>
        <div className="argpop__title">Launch arguments</div>
        <textarea
          className="input"
          ref={field}
          spellCheck={false}
          placeholder="-windowed -ResX=1280"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
        <div className="argpop__row">
          <span className="argpop__hint">Extra arguments only. The ones the game needs are always added for you.</span>
          <button className="btn" type="button" onClick={() => { setDraft(value); setOpen(false); }}>
            Cancel
          </button>
          <button className="btn btn--primary" type="button" onClick={() => { onSave(draft); setOpen(false); }}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
