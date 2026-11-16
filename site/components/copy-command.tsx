'use client';

import { Check, Copy } from 'lucide-react';
import { useState } from 'react';

/** A shell command with a copy button. */
export function CopyCommand({ command, label }: { command: string; label: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard?.writeText(command).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    });
  };
  return (
    <div className="inline-flex max-w-full items-center gap-2 rounded-full border bg-fd-background/70 py-1 ps-4 pe-1 font-mono text-[13px] backdrop-blur">
      <span className="select-none text-fd-muted-foreground" aria-hidden="true">
        $
      </span>
      <code className="min-w-0 truncate">{command}</code>
      <button
        type="button"
        onClick={copy}
        aria-label={copied ? 'Copied' : label}
        className="flex size-7 shrink-0 items-center justify-center rounded-full text-fd-muted-foreground transition-colors hover:bg-fd-accent hover:text-fd-foreground"
      >
        {copied ? (
          <Check className="size-3.5 text-fd-primary" aria-hidden="true" />
        ) : (
          <Copy className="size-3.5" aria-hidden="true" />
        )}
      </button>
      <span className="sr-only" aria-live="polite">
        {copied ? 'Copied to the clipboard' : ''}
      </span>
    </div>
  );
}
