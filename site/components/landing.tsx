import Link from 'next/link';
import { type CSSProperties, Fragment, type ReactNode } from 'react';
import { Logo } from './logo';
import { site } from '@/lib/site';

/** Animation delay (ms) for `data-reveal` and `data-hero` elements. */
export function delay(ms: number): CSSProperties {
  return { '--tg-delay': ms } as CSSProperties;
}

/** A section's label, typed out as it is revealed. */
export function Eyebrow({ text }: { text: string }) {
  return (
    <p className="mb-3 font-mono text-xs uppercase tracking-[0.18em] text-fd-primary">
      <span className="tg-type inline-block" style={{ '--len': text.length } as CSSProperties}>
        {text}
      </span>
    </p>
  );
}

/** A title whose words rise in one after another (other content as it is). */
export function Words({ children }: { children: ReactNode }) {
  if (typeof children !== 'string') return <>{children}</>;
  return (
    <>
      {children.split(' ').map((word, index) => (
        // A fixed title: the index is the word's identity.
        // eslint-disable-next-line react/no-array-index-key
        <Fragment key={index}>
          {index > 0 ? ' ' : null}
          <span className="tg-rise-word" style={{ '--i': index } as CSSProperties}>
            {word}
          </span>
        </Fragment>
      ))}
    </>
  );
}

export function Section({
  id,
  eyebrow,
  title,
  lead,
  children,
  className = '',
}: {
  id?: string;
  eyebrow?: string;
  title: ReactNode;
  lead?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section id={id} className={`mx-auto w-full max-w-6xl px-6 py-20 md:py-28 ${className}`}>
      <div data-reveal="" className="mb-12 max-w-3xl">
        {eyebrow ? <Eyebrow text={eyebrow} /> : null}
        <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
          <Words>{title}</Words>
        </h2>
        {lead ? <p className="mt-5 text-lg text-fd-muted-foreground text-pretty">{lead}</p> : null}
      </div>
      {children}
    </section>
  );
}

/** The site's pill buttons. */
export function buttonClass(primary: boolean): string {
  return primary
    ? 'inline-flex h-11 items-center gap-2 rounded-full bg-fd-foreground px-6 text-sm font-medium text-fd-background transition-[opacity,transform] duration-200 hover:opacity-85 active:scale-[0.98]'
    : 'inline-flex h-11 items-center gap-2 rounded-full border bg-fd-background/60 px-6 text-sm font-medium backdrop-blur transition-[background-color,transform] duration-200 hover:bg-fd-accent active:scale-[0.98]';
}

export function ButtonLink({
  href,
  children,
  primary = false,
}: {
  href: string;
  children: ReactNode;
  primary?: boolean;
}) {
  const className = buttonClass(primary);
  if (href.startsWith('http')) {
    return (
      <a href={href} className={className} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    );
  }
  return (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}

const footerColumns: { title: string; links: [string, string][] }[] = [
  {
    title: 'Library',
    links: [
      ['Getting started', '/docs/getting-started/'],
      ['Install', '/docs/getting-started/install/'],
      ['Reference', '/docs/reference/'],
      ['Command line', '/docs/cli/'],
      ['Playground', '/playground/'],
    ],
  },
  {
    title: 'Data',
    links: [
      ['Coverage', '/docs/data/coverage/'],
      ['Accuracy', '/docs/concepts/accuracy/'],
      ['Measured', '/docs/data/measured/'],
      ['Licences', '/docs/concepts/licences/'],
      ['Data releases', site.releases],
    ],
  },
  {
    title: 'Project',
    links: [
      ['GitHub', site.github],
      ['Changelog', site.changelog],
      ['Report an issue', site.issues],
      ['llms.txt', '/llms.txt'],
    ],
  },
];

function FooterLink({ href, children }: { href: string; children: ReactNode }) {
  const className = 'transition-colors hover:text-fd-foreground';
  return href.startsWith('http') ? (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className}>
      {children}
    </a>
  ) : (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}

/** The footer of the landing page and the playground. */
export function SiteFooter() {
  return (
    <footer className="border-t bg-fd-card/40">
      <div className="mx-auto grid w-full max-w-6xl gap-12 px-6 py-16 text-sm md:grid-cols-[1.4fr_repeat(3,1fr)]">
        <div className="flex max-w-xs flex-col gap-4">
          <Link href="/" className="text-base">
            <Logo />
          </Link>
          <p className="text-fd-muted-foreground">
            Crafted by{' '}
            <a
              href={site.org.url}
              target="_blank"
              rel="noopener noreferrer"
              className="font-medium text-fd-foreground underline-offset-4 hover:underline"
            >
              {site.org.name}
            </a>{' '}
            in Nepal, and open source for everyone who needs to know where a place is.
          </p>
        </div>
        {footerColumns.map((column) => (
          <nav key={column.title} aria-label={column.title} className="flex flex-col gap-3">
            <p className="font-medium">{column.title}</p>
            <ul className="flex flex-col gap-2.5 text-fd-muted-foreground">
              {column.links.map(([label, href]) => (
                <li key={label}>
                  <FooterLink href={href}>{label}</FooterLink>
                </li>
              ))}
            </ul>
          </nav>
        ))}
      </div>
      <div className="border-t">
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-3 px-6 py-6 text-xs text-fd-muted-foreground md:flex-row md:items-center md:justify-between">
          <p>
            © {new Date().getFullYear()} {site.org.name}. Code under Apache-2.0.
          </p>
          <p className="max-w-xl md:text-right">
            Data: the main build under ODbL (© OpenStreetMap contributors and others), the lite build
            under CC BY 4.0. Every pack credits its sources.
          </p>
        </div>
      </div>
    </footer>
  );
}
