'use client';

import Link from 'next/link';
import { useEffect, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { asset } from '@/lib/site';

/** Whether the page is in dark mode: the `dark` class Fumadocs' theme sets on `<html>`. */
function useDarkClass() {
  const [dark, setDark] = useState(false);
  useEffect(() => {
    const root = document.documentElement;
    const read = () => setDark(root.classList.contains('dark'));
    read();
    const observer = new MutationObserver(read);
    observer.observe(root, { attributes: true, attributeFilter: ['class'] });
    return () => observer.disconnect();
  }, []);
  return dark;
}

/**
 * A recording of the site in its window frame, light and dark (`public/shots/<name>-*.{mp4,webm}`
 * and a poster, written by `scripts/video.mjs`). Only the theme's video loads, and only once it
 * is near the screen; it plays while on screen, never on its own with Reduce Motion on, and a
 * button pauses it (moving content longer than five seconds can be stopped, WCAG 2.2.2).
 */
export function Film({ name, alt, href, width = 1280, height = 750 }: { name: string; alt: string; href: string; width?: number; height?: number }) {
  const light = useRef<HTMLVideoElement>(null);
  const dark = useRef<HTMLVideoElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const resolvedTheme = useDarkClass() ? 'dark' : 'light';
  const [stopped, setStopped] = useState(false);
  const [playing, setPlaying] = useState(false);

  useEffect(() => {
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) setStopped(true);
  }, []);

  useEffect(() => {
    const active = (resolvedTheme === 'dark' ? dark : light).current;
    const other = (resolvedTheme === 'dark' ? light : dark).current;
    other?.pause();
    if (!active || !box.current) return;
    const sync = (visible: boolean) => {
      if (visible && !stopped) void active.play().catch(() => setPlaying(false));
      else active.pause();
    };
    const observer = new IntersectionObserver(([entry]) => sync(!!entry?.isIntersecting), { rootMargin: '10% 0px' });
    observer.observe(box.current);
    return () => observer.disconnect();
  }, [resolvedTheme, stopped]);

  const video = (theme: 'light' | 'dark') => (
    <video
      ref={theme === 'light' ? light : dark}
      width={width}
      height={height}
      muted
      loop
      playsInline
      preload="none"
      poster={asset(`/shots/${name}-${theme}-poster.webp`)}
      aria-label={alt}
      onPlay={() => setPlaying(true)}
      onPause={() => setPlaying(false)}
      className={`h-auto w-full ${theme === 'light' ? 'block dark:hidden' : 'hidden dark:block'}`}
    >
      <source src={asset(`/shots/${name}-${theme}.webm`)} type="video/webm" />
      <source src={asset(`/shots/${name}-${theme}.mp4`)} type="video/mp4" />
    </video>
  );

  return (
    <div ref={box}>
      <div className="tg-window overflow-hidden rounded-[10px] border bg-fd-card md:rounded-2xl">
        <Link href={href} aria-label="Open the playground" className="block">
          {video('light')}
          {video('dark')}
        </Link>
      </div>
      <div className="mt-3 flex items-center justify-between gap-3 px-1 text-xs text-fd-muted-foreground">
        <span className="inline-flex items-center gap-2">
          <span className="relative flex size-1.5">
            <span className={`absolute inline-flex size-full rounded-full bg-red-500 opacity-60 ${playing ? 'animate-ping' : ''}`} />
            <span className="relative inline-flex size-1.5 rounded-full bg-red-500" />
          </span>
          A real recording of the playground: every answer ran in the browser.
        </span>
        <button
          type="button"
          onClick={() => setStopped((s) => !s)}
          aria-label={playing ? 'Pause the recording' : 'Play the recording'}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-3 font-medium transition-colors hover:text-fd-foreground"
        >
          {playing ? <Pause className="size-3.5" aria-hidden="true" /> : <Play className="size-3.5" aria-hidden="true" />}
          {playing ? 'Pause' : 'Play'}
        </button>
      </div>
    </div>
  );
}
