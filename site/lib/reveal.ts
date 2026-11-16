/** How much of an element must be in view to reveal it, and how little to hide it again. */
export const SHOW_AT = 0.12;
export const HIDE_AT = 0.04;

/**
 * Whether a scroll reveal should show, hide or stay. Visibility is measured against the
 * element or the window, whichever is shorter, so tall elements reveal too. An element hides
 * only as it leaves through the bottom (the reader scrolling back up); one leaving through the
 * top stays shown. The gap between the two thresholds keeps it from flickering.
 */
export function revealChange({
  shown,
  visible,
  height,
  top,
  viewport,
}: {
  shown: boolean;
  /** Pixels of it in view. */
  visible: number;
  height: number;
  /** Its top, relative to the window. */
  top: number;
  viewport: number;
}): 'show' | 'hide' | null {
  const fraction = visible / Math.max(1, Math.min(height, viewport));
  if (!shown && fraction >= SHOW_AT) return 'show';
  if (shown && fraction <= HIDE_AT && top > viewport / 2) return 'hide';
  return null;
}
