'use client';

import { useEffect, useState } from 'react';

export interface Query {
  /** The tab's label. */
  label: string;
  /** The call, one line. */
  call: string;
  /** What it printed, from the example's checked-in output. */
  output: string;
}

/**
 * Real queries and what they returned on the released pack, one at a time. It steps through
 * them on its own until the reader picks one, and holds still for Reduce Motion.
 */
export function QueryCard({ queries }: { queries: Query[] }) {
  const [index, setIndex] = useState(0);
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (held || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const timer = window.setInterval(() => setIndex((i) => (i + 1) % queries.length), 4200);
    return () => window.clearInterval(timer);
  }, [held, queries.length]);
  const q = queries[index] ?? queries[0];
  if (!q) return null;
  return (
    <div className="tg-window overflow-hidden rounded-2xl border bg-fd-card/85 text-sm backdrop-blur">
      <div role="tablist" aria-label="Queries" className="flex gap-1 overflow-x-auto border-b px-2 py-2">
        {queries.map((item, i) => (
          <button
            key={item.label}
            type="button"
            role="tab"
            aria-selected={i === index}
            onClick={() => {
              setIndex(i);
              setHeld(true);
            }}
            className={`shrink-0 rounded-full px-3 py-1 text-xs font-medium transition-colors ${
              i === index
                ? 'bg-fd-primary text-fd-primary-foreground'
                : 'text-fd-muted-foreground hover:bg-fd-accent hover:text-fd-foreground'
            }`}
          >
            {item.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" aria-live="polite" className="font-mono text-[12.5px] leading-relaxed">
        <pre key={`c${index}`} className="tg-fade overflow-x-auto px-4 pt-4 text-fd-foreground">
          <span className="select-none text-fd-primary">› </span>
          {q.call}
        </pre>
        <pre key={`o${index}`} className="tg-fade min-h-[7.5rem] overflow-x-auto px-4 pt-2 pb-4 text-fd-muted-foreground">
          {q.output}
        </pre>
      </div>
      <p className="border-t px-4 py-2 text-[11px] text-fd-muted-foreground">
        Printed by the guides&apos; programs on the released world lite pack, offline.
      </p>
    </div>
  );
}
