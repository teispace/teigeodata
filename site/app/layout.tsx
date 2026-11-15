import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { Provider } from '@/components/provider';
import './global.css';

export const metadata: Metadata = {
  title: {
    template: '%s | teigeodata',
    default: 'teigeodata: accurate, offline places',
  },
  description:
    'Accurate, offline place data with one API in every major language: search, reverse geocoding, admin hierarchy, timezones, elevation and boundaries, in small signed packs.',
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
