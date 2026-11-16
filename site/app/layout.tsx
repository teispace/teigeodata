import type { Metadata, Viewport } from 'next';
import type { ReactNode } from 'react';
import { Provider } from '@/components/provider';
import { motionBootScript } from '@/lib/motion-boot';
import { ogImage, pageMetadata } from '@/lib/seo';
import { asset, site } from '@/lib/site';
import './global.css';

export const metadata: Metadata = {
  // Social cards default to the home page's; each page sets its own canonical URL.
  ...pageMetadata({ description: site.description, path: '/', image: ogImage(['home']) }),
  alternates: {},
  title: { default: site.title, template: `%s · ${site.name}` },
  metadataBase: new URL(site.url),
  applicationName: site.name,
  keywords: site.keywords,
  authors: [{ name: site.org.name, url: site.org.url }],
  creator: site.org.name,
  publisher: site.org.name,
  category: 'technology',
  robots: { index: true, follow: true, 'max-image-preview': 'large', 'max-snippet': -1 },
  icons: { icon: [{ url: asset('/icon.svg'), type: 'image/svg+xml' }, { url: asset('/brand/icon.png') }], apple: asset('/brand/icon.png') },
  formatDetection: { telephone: false, email: false, address: false },
};

export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#0a0a0a' },
  ],
  colorScheme: 'light dark',
};

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        {/* A constant script that must run before the first paint. */}
        <script dangerouslySetInnerHTML={{ __html: motionBootScript }} />
      </head>
      <body className="flex min-h-screen flex-col antialiased">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
