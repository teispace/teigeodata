/** The wordmark: a pin over a contour, then the name. */
export function Logo() {
  return (
    <span className="inline-flex items-center gap-2 font-semibold tracking-tight">
      <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true" className="text-fd-primary">
        <path
          d="M12 2.5c-3.6 0-6.5 2.8-6.5 6.4 0 4.6 5.1 10.5 6 11.5a.7.7 0 0 0 1 0c.9-1 6-6.9 6-11.5 0-3.6-2.9-6.4-6.5-6.4Z"
          fill="currentColor"
          opacity="0.18"
        />
        <path
          d="M12 2.5c-3.6 0-6.5 2.8-6.5 6.4 0 4.6 5.1 10.5 6 11.5a.7.7 0 0 0 1 0c.9-1 6-6.9 6-11.5 0-3.6-2.9-6.4-6.5-6.4Z"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
        />
        <circle cx="12" cy="9" r="2.4" fill="currentColor" />
      </svg>
      teigeodata
    </span>
  );
}
