import type { Metadata } from 'next';
import { Playground } from '@/components/playground/playground';

export const metadata: Metadata = {
  title: 'Playground',
  description:
    'Every query of teigeodata in your browser: search, reverse, admin units, timezones, elevation, nearby places and outlines, run by the WebAssembly engine on a released pack. Nothing leaves the page.',
};

export default function PlaygroundPage() {
  return (
    <main className="flex h-[calc(100dvh-var(--fd-nav-height,3.5rem))] min-h-[560px] flex-col">
      <Playground />
    </main>
  );
}
