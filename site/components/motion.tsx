'use client';

import { usePathname } from 'next/navigation';
import { useEffect } from 'react';
import { revealChange } from '@/lib/reveal';

declare global {
  interface Window {
    __tgReveal?: boolean;
  }
}

/** Visibility steps the observer reports, fine enough for tall elements too. */
const STEPS = [0, 0.02, 0.04, 0.06, 0.08, 0.1, 0.12, 0.16, 0.2, 0.3, 0.5, 0.75, 1];

/**
 * Reveals `[data-reveal]` elements as they scroll into view and hides them as they leave
 * through the bottom, so scrolling back up and down again plays each reveal again.
 */
export function Motion() {
  const pathname = usePathname();
  useEffect(() => {
    window.__tgReveal = true;
    const root = document.documentElement;
    const elements = document.querySelectorAll<HTMLElement>('[data-reveal]');
    if (!root.classList.contains('tg-motion') || !('IntersectionObserver' in window)) {
      for (const element of elements) element.dataset.shown = '';
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const element = entry.target as HTMLElement;
          const change = revealChange({
            shown: element.dataset.shown !== undefined,
            visible: entry.intersectionRect.height,
            height: entry.boundingClientRect.height,
            top: entry.boundingClientRect.top,
            viewport: entry.rootBounds?.height ?? window.innerHeight,
          });
          if (change === 'show') element.dataset.shown = '';
          else if (change === 'hide') delete element.dataset.shown;
        }
      },
      { rootMargin: '0px 0px -8% 0px', threshold: STEPS },
    );
    for (const element of elements) observer.observe(element);
    return () => observer.disconnect();
  }, [pathname]);
  return null;
}
