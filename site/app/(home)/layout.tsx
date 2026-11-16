import type { ReactNode } from 'react';
import { HomeLayout } from 'fumadocs-ui/layouts/home';
import { SiteFooter } from '@/components/landing';
import { Motion } from '@/components/motion';
import { Scenes } from '@/components/scenes';
import { baseOptions } from '@/lib/layout.shared';

export default function Layout({ children }: { children: ReactNode }) {
  const options = baseOptions();
  return (
    <HomeLayout {...options} nav={{ ...options.nav, transparentMode: 'top' }}>
      {children}
      <SiteFooter />
      <Motion />
      <Scenes />
    </HomeLayout>
  );
}
