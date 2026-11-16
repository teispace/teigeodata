'use client';

import { usePathname } from 'next/navigation';
import { useEffect } from 'react';

/**
 * Scroll scenes (teitunnel's, cut to the two kinds this site uses): every `[data-scene]`
 * element gets `--p`, its progress from 0 to 1, which CSS turns into movement (transforms
 * only).
 *
 * - `view`: 0 when its top reaches the bottom of the window, 1 when its bottom leaves the top.
 * - `enter`: 0 when its top reaches the bottom, 1 when its top reaches `data-end` of the window
 *   (a fraction, 0.35 by default): things that settle into place as they arrive.
 *
 * Only scenes near the window are computed, once per frame at most. Without JavaScript, or with
 * Reduce Motion (no `html.tg-motion`), nothing moves: the CSS shows every scene at rest.
 */

interface Scene {
  element: HTMLElement;
  enter: boolean;
  end: number;
  progress: number;
}

const clamp = (value: number) => Math.min(Math.max(value, 0), 1);

function measure(scene: Scene, height: number): number {
  const rect = scene.element.getBoundingClientRect();
  if (scene.enter) return clamp((height - rect.top) / (height * (1 - scene.end)));
  return clamp((height - rect.top) / (height + rect.height));
}

function apply(scene: Scene, progress: number) {
  if (scene.progress !== -1 && Math.abs(progress - scene.progress) < 0.0005) return;
  scene.progress = progress;
  scene.element.style.setProperty('--p', progress.toFixed(4));
}

export function Scenes() {
  const pathname = usePathname();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new page brings new scenes
  useEffect(() => {
    const root = document.documentElement;
    const scenes = [...document.querySelectorAll<HTMLElement>('[data-scene]')].map(
      (element): Scene => ({
        element,
        enter: element.dataset.scene === 'enter',
        end: Number(element.dataset.end ?? 0.35),
        progress: -1,
      }),
    );
    if (!root.classList.contains('tg-motion') || !('IntersectionObserver' in window)) {
      for (const scene of scenes) apply(scene, 1);
      return;
    }

    const near = new Set<Scene>();
    const byElement = new Map(scenes.map((scene) => [scene.element, scene]));
    let frame = 0;
    const update = () => {
      frame = 0;
      const height = window.innerHeight;
      for (const scene of near) apply(scene, measure(scene, height));
    };
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(update);
    };
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const scene = byElement.get(entry.target as HTMLElement);
          if (!scene) continue;
          if (entry.isIntersecting) near.add(scene);
          else {
            // Leaving: settle at the end it left by, so nothing is stuck halfway.
            apply(scene, measure(scene, window.innerHeight));
            near.delete(scene);
          }
        }
        schedule();
      },
      { rootMargin: '25% 0px 25% 0px' },
    );
    for (const scene of scenes) {
      apply(scene, measure(scene, window.innerHeight));
      observer.observe(scene.element);
    }

    // Reduce Motion turned on while reading: everything to rest.
    const reduce = matchMedia('(prefers-reduced-motion: reduce)');
    const onReduce = () => {
      if (!reduce.matches) return;
      root.classList.remove('tg-motion');
      for (const scene of scenes) apply(scene, 1);
    };
    window.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule, { passive: true });
    reduce.addEventListener('change', onReduce);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      window.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
      reduce.removeEventListener('change', onReduce);
    };
  }, [pathname]);
  return null;
}
