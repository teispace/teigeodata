import type { Metadata } from 'next';
import { ogImage, pageMetadata } from '@/lib/seo';
import { Playground } from '@/components/playground/playground';

export const metadata: Metadata = pageMetadata({
  title: 'Playground',
  description:
    'Every query of teigeodata : search, reverse, admin units, timezones, elevation, nearby places, outlines, distances and radii on an OpenStreetMap map, run by the WebAssembly engine on a released pack in your browser.',
  path: '/playground/',
  image: ogImage(['playground']),
});

export default function PlaygroundPage() {
  return (
    <main className="flex h-[calc(100dvh-var(--fd-nav-height,3.5rem))] min-h-[560px] flex-col">
      <Playground />
    </main>
  );
}
