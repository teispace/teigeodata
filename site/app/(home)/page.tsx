import type { Metadata } from 'next';
import Link from 'next/link';
import {
  ArrowRight,
  Boxes,
  Clock,
  Database,
  FileCheck2,
  Globe2,
  Languages,
  Layers,
  Mountain,
  Network,
  Search,
  ShieldCheck,
  Sparkles,
  WifiOff,
} from 'lucide-react';
import { ServerCodeBlock } from 'fumadocs-ui/components/codeblock.rsc';
import { Tab, Tabs } from 'fumadocs-ui/components/tabs';
import { Contours } from '@/components/contours';
import { CopyCommand } from '@/components/copy-command';
import { JsonLd } from '@/components/json-ld';
import { ButtonLink, delay, Eyebrow, Section, Words } from '@/components/landing';
import { QueryCard, type Query } from '@/components/query-card';
import { output, region } from '@/lib/examples';
import { faqPage, ogImage, organization, pageMetadata, softwareLibrary, website } from '@/lib/seo';
import { site } from '@/lib/site';

export const metadata: Metadata = pageMetadata({
  description: site.description,
  path: '/',
  image: ogImage(['home']),
});

const lines = (name: string, n?: number) => output(name).split('\n').slice(0, n).join('\n');

// Calls as the Python guide's program makes them; outputs as it printed them.
const queries: Query[] = [
  { label: 'Search', call: 'engine.search("Pokhara", 3)', output: output('search') },
  { label: 'Reverse', call: 'engine.reverse(27.7172, 85.3240, 3)', output: output('reverse') },
  { label: 'Admin units', call: 'engine.admin_at(28.2096, 83.9856)', output: lines('admin', 4) },
  { label: 'Hierarchy', call: 'engine.ancestors("tgd:np/1xhb")', output: lines('hierarchy', 4) },
  {
    label: 'Timezone',
    call: 'engine.tz_at(27.7172, 85.3240)',
    output: `${lines('timezone', 1)}\n${output('timezone').split('\n').at(-1)}`,
  },
  { label: 'Elevation', call: 'engine.elevation_at(27.7172, 85.3240)', output: output('elevation') },
];

// Each figure is from a gate's record (site/data/measured) or the released pack (coverage).
const proof = [
  { value: '172,339', unit: 'places', body: 'in the 15.6 MB world lite pack' },
  { value: '0.9', unit: 'ms', body: 'to open a pack, estimated for a low-end phone' },
  { value: '1.3', unit: 'ms', body: 'per search keystroke at p99, same phone estimate' },
  { value: '12', unit: 'languages', body: 'one API, generated from one definition' },
];

const features = [
  {
    icon: Search,
    title: 'Search in every script',
    body: 'काठमाडौं, Kathmandu and Kathamandu find the same place. "Name, Parent" ranks the place in that parent first; a focus point ranks what is near.',
  },
  {
    icon: Globe2,
    title: 'Reverse geocoding',
    body: 'From a point to the nearest places with distance and bearing, and to every admin unit that holds it, country first.',
  },
  {
    icon: Layers,
    title: 'The whole hierarchy',
    body: 'Country, province, district, local level and ward: every unit with its ISO code, parents, children and outline.',
  },
  {
    icon: Clock,
    title: 'Exact timezones',
    body: 'The IANA zone at any point as its boundaries draw it; offsets and transitions from the tzdb rules in the pack, not the device.',
  },
  {
    icon: Mountain,
    title: 'Elevation',
    body: 'A height for every place and at any point: metres above EGM2008, interpolated between samples, with its source named.',
  },
  {
    icon: Languages,
    title: 'Local names',
    body: 'Names in each language and script, written ones kept apart from transliterations, spellings checked against each country’s lexicon.',
  },
];

const languages: { name: string; lang: string; file: string; guide: string }[] = [
  { name: 'Rust', lang: 'rust', file: 'rust/tour.rs', guide: 'rust' },
  { name: 'Python', lang: 'python', file: 'python/tour.py', guide: 'python' },
  { name: 'Node.js', lang: 'js', file: 'node/tour.mjs', guide: 'node' },
  { name: 'Swift', lang: 'swift', file: 'swift/Tour/Sources/Tour/main.swift', guide: 'swift' },
  { name: 'Kotlin', lang: 'kotlin', file: 'kotlin/Tour.kt', guide: 'kotlin-java' },
  { name: 'Dart', lang: 'dart', file: 'dart/bin/tour.dart', guide: 'dart-flutter' },
  { name: 'Go', lang: 'go', file: 'go/tour.go', guide: 'go' },
  { name: 'C++', lang: 'cpp', file: 'cpp/tour.cpp', guide: 'cpp' },
  { name: 'C#', lang: 'csharp', file: 'dotnet/Program.cs', guide: 'dotnet' },
  { name: 'Java', lang: 'java', file: 'java/Tour.java', guide: 'kotlin-java' },
];

const pipeline = [
  {
    icon: Database,
    title: 'Open sources',
    body: 'OpenStreetMap, GeoNames, Wikidata, official boundaries, terrain models and tzdb, each pinned by hash.',
  },
  {
    icon: Sparkles,
    title: 'The forge',
    body: 'Cleans, deduplicates and links them; checks spellings, heights and outlines against gates that fail the build.',
  },
  {
    icon: FileCheck2,
    title: 'A signed pack',
    body: 'One compact file per country or region: every section hashed, the whole signed, its sources credited inside.',
  },
  {
    icon: Boxes,
    title: 'Your app',
    body: 'Memory-mapped by the engine in any language, bundled or downloaded on demand. Queries never touch the network.',
  },
];

const ways = [
  {
    icon: WifiOff,
    title: 'In your app, offline',
    body: 'Bundle a pack, or let the store download, verify and keep the countries your users pick.',
    href: '/docs/concepts/store/',
    cta: 'The store',
  },
  {
    icon: Network,
    title: 'As an HTTP server',
    body: 'tgd serve answers JSON, GeoJSON and Nominatim-compatible routes from the same packs.',
    href: '/docs/servers/http/',
    cta: 'HTTP server',
  },
  {
    icon: Sparkles,
    title: 'For AI agents',
    body: 'tgd mcp gives assistants the same queries as tools over the Model Context Protocol.',
    href: '/docs/servers/mcp/',
    cta: 'MCP server',
  },
];

const faq = [
  {
    q: 'What is teigeodata?',
    a: 'An open-source geodata SDK. It answers where places are and what is at a point (search, reverse geocoding, the admin hierarchy, boundaries, timezones and elevation) from compact signed packs that work offline, with the same API in twelve languages.',
  },
  {
    q: 'Does it need a network connection or an API key?',
    a: 'No. Every query runs on a pack on the device or server. A network is only used to download a pack, and the store verifies each one against a signed catalog before it is used.',
  },
  {
    q: 'Which countries are covered?',
    a: "Nepal in full, down to wards, plus the world's countries, first-level units and cities of 5,000 or more, and India's districts and towns. Coverage grows by data releases: India, South Asia, Asia, the USA, then the rest of the world.",
  },
  {
    q: 'Can I store and redistribute the results?',
    a: 'Yes. The code is Apache-2.0. The main data build is under ODbL and the lite build under CC BY 4.0, both of which allow keeping, shipping and redistributing the data with the credits each pack carries.',
  },
  {
    q: 'How accurate is it?',
    a: 'Coordinates and elevation come first: the target is a vertical error of at most 50 m at the 90th percentile, and each target is a gate a build must pass. Names follow official sources and each country’s lexicon. The figures ship in every pack’s summary.',
  },
  {
    q: 'How does it handle disputed borders?',
    a: "From one stated perspective, recorded in each country's profile with its sources: Nepal's official map of 2020, including Kalapani, Lipulekh and Limpiyadhura.",
  },
  {
    q: 'Can it replace a Nominatim server?',
    a: 'For lookups by name and point, yes: tgd serve answers Nominatim-compatible search and reverse routes from a pack, with a few megabytes of memory.',
  },
];

export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col overflow-x-clip">
      <JsonLd things={[organization, website, softwareLibrary, faqPage(faq)]} />

      {/* Hero */}
      <section className="relative isolate -mt-[var(--fd-nav-height,3.5rem)] pt-[var(--fd-nav-height,3.5rem)]">
        <div aria-hidden="true" className="tg-grid absolute inset-0 -z-10" />
        <div aria-hidden="true" className="tg-glow absolute -top-32 right-[-10%] -z-10 size-[42rem] rounded-full" />
        <div className="absolute inset-0 -z-10 opacity-70 dark:opacity-50">
          <Contours />
        </div>
        <div className="mx-auto grid w-full max-w-6xl items-center gap-14 px-6 pt-16 pb-20 md:pt-24 lg:grid-cols-[1.05fr_1fr] lg:pb-28">
          <div className="min-w-0">
            <Link
              href={site.releases}
              data-hero=""
              style={delay(0)}
              className="group mb-7 inline-flex items-center gap-2 rounded-full border bg-fd-background/70 py-1 ps-1 pe-3 text-xs backdrop-blur transition-colors hover:border-fd-primary/50"
            >
              <span className="rounded-full bg-fd-primary px-2 py-0.5 font-medium text-fd-primary-foreground">
                v{site.version}
              </span>
              <span className="text-fd-muted-foreground group-hover:text-fd-foreground">
                The first stable release is out
              </span>
              <ArrowRight className="size-3 text-fd-muted-foreground transition-transform group-hover:translate-x-0.5" />
            </Link>
            <h1
              data-hero=""
              style={delay(80)}
              className="text-[2.6rem] leading-[1.05] font-semibold tracking-tight text-balance sm:text-6xl"
            >
              Accurate places. <span className="tg-gradient-text">Offline,</span> in every language.
            </h1>
            <p
              data-hero=""
              style={delay(180)}
              className="mt-6 max-w-xl text-lg text-fd-muted-foreground text-pretty"
            >
              Search, reverse geocoding, the admin hierarchy down to the ward, boundaries, local
              names, exact timezones and elevation, cleaned from the best open sources into small
              signed packs. One API for twelve languages, no server and no API key.
            </p>
            <div data-hero="" style={delay(280)} className="mt-9 flex flex-wrap items-center gap-3">
              <ButtonLink href="/docs/getting-started/" primary>
                Get started <ArrowRight className="size-4" />
              </ButtonLink>
              <ButtonLink href="/playground/">Try it in the browser</ButtonLink>
            </div>
            <div data-hero="" style={delay(380)} className="mt-6 max-w-full">
              <CopyCommand
                command="cargo add teigeodata-engine --git https://github.com/teispace/teigeodata --tag v1.0.0"
                label="Copy the install command"
              />
            </div>
          </div>
          <div data-hero="" style={delay(320)} className="min-w-0">
            <QueryCard queries={queries} />
          </div>
        </div>
      </section>

      {/* Proof */}
      <section aria-label="Measured" className="border-y bg-fd-card/40">
        <div className="mx-auto grid w-full max-w-6xl grid-cols-2 px-6 lg:grid-cols-4">
          {proof.map((p, i) => (
            <div
              key={p.body}
              data-reveal=""
              style={delay(i * 90)}
              className={`py-8 pe-4 lg:py-10 ${i > 0 ? 'lg:border-s lg:ps-8' : ''} ${i % 2 === 1 ? 'border-s ps-6 lg:ps-8' : ''} ${i > 1 ? 'border-t lg:border-t-0' : ''}`}
            >
              <p className="text-3xl font-semibold tracking-tight tabular-nums md:text-4xl">
                {p.value}
                <span className="ms-1.5 text-base font-medium text-fd-muted-foreground">{p.unit}</span>
              </p>
              <p className="mt-1.5 text-sm text-fd-muted-foreground">{p.body}</p>
            </div>
          ))}
        </div>
        <p className="mx-auto w-full max-w-6xl px-6 pb-4 text-xs text-fd-muted-foreground">
          Measured by the gates on 2026-11-15; the method, machine and commit of each are on{' '}
          <Link href="/docs/data/measured/" className="underline underline-offset-4 hover:text-fd-foreground">
            Measured
          </Link>
          .
        </p>
      </section>

      {/* What it answers */}
      <Section
        id="features"
        eyebrow="What it answers"
        title="Everything an app needs to know about a place"
        lead="Six kinds of question, each answered in microseconds from a file on the device, with the source of every value named."
      >
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {features.map(({ icon: Icon, title, body }, i) => (
            <div key={title} data-reveal="" style={delay((i % 3) * 90)} className="tg-card p-6">
              <span className="inline-flex size-10 items-center justify-center rounded-xl border bg-fd-background text-fd-primary">
                <Icon className="size-5" aria-hidden="true" />
              </span>
              <h3 className="mt-5 font-semibold">{title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-fd-muted-foreground">{body}</p>
            </div>
          ))}
        </div>
      </Section>

      {/* Code */}
      <section id="languages" className="border-y bg-fd-card/30">
        <div className="mx-auto grid w-full max-w-6xl gap-12 px-6 py-20 md:py-28 lg:grid-cols-[0.85fr_1.15fr]">
          <div data-reveal="" className="lg:sticky lg:top-24 lg:self-start">
            <Eyebrow text="One API, twelve languages" />
            <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
              <Words>The same call wherever you write code</Words>
            </h2>
            <p className="mt-5 text-lg text-fd-muted-foreground text-pretty">
              Rust, C, C++, Swift, Kotlin and Java, Dart and Flutter, Python, Node.js, WebAssembly,
              Go and .NET share one definition, and a parity suite checks every value they return
              against the same fixture.
            </p>
            <p className="mt-4 text-sm text-fd-muted-foreground">
              Every snippet here is cut from a guide&apos;s program that ran on the released pack.
            </p>
            <div className="mt-8">
              <ButtonLink href="/docs/guides/rust/">
                Pick your language guide <ArrowRight className="size-4" />
              </ButtonLink>
            </div>
          </div>
          <div data-reveal="blur" style={delay(120)} className="min-w-0">
            <Tabs items={languages.map((l) => l.name)} className="tg-window rounded-2xl">
              {languages.map((l) => (
                <Tab key={l.name} value={l.name}>
                  <ServerCodeBlock
                    lang={l.lang}
                    code={region(l.file, 'reverse')}
                    codeblock={{ title: `Nearest places · ${l.file}` }}
                  />
                </Tab>
              ))}
            </Tabs>
            <div className="mt-4 overflow-hidden rounded-xl border bg-fd-background/60 font-mono text-[12.5px]">
              <p className="border-b px-4 py-2 text-[11px] uppercase tracking-wider text-fd-muted-foreground">
                Output, the same in every language
              </p>
              <pre className="overflow-x-auto px-4 py-3">{output('reverse')}</pre>
            </div>
          </div>
        </div>
      </section>

      {/* How it works */}
      <Section
        id="how"
        eyebrow="How it works"
        title="From open sources to a file you can trust"
        lead="The work happens once, in the forge. Your app gets the result: one verified file, no service to call."
      >
        <ol className="relative grid gap-4 md:grid-cols-4">
          <div aria-hidden="true" className="tg-wire absolute top-[2.2rem] right-[calc(25%-1rem)] left-[2.2rem] hidden h-px md:block" />
          {pipeline.map(({ icon: Icon, title, body }, i) => (
            <li key={title} data-reveal="" style={delay(i * 120)} className="relative">
              <span className="relative z-10 mx-auto flex size-[4.4rem] items-center justify-center rounded-2xl border bg-fd-background text-fd-primary shadow-sm md:mx-0">
                <Icon className="size-6" aria-hidden="true" />
                <span className="absolute -top-2 -right-2 flex size-6 items-center justify-center rounded-full bg-fd-foreground text-xs font-semibold text-fd-background">
                  {i + 1}
                </span>
              </span>
              <h3 className="mt-5 font-semibold">{title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-fd-muted-foreground">{body}</p>
            </li>
          ))}
        </ol>
      </Section>

      {/* Ways to run */}
      <section className="mx-auto w-full max-w-6xl px-6 pb-20 md:pb-28">
        <div className="grid gap-4 md:grid-cols-3">
          {ways.map(({ icon: Icon, title, body, href, cta }, i) => (
            <Link key={title} href={href} data-reveal="" style={delay(i * 90)} className="tg-card group flex flex-col p-6">
              <Icon className="size-5 text-fd-primary" aria-hidden="true" />
              <h3 className="mt-4 font-semibold">{title}</h3>
              <p className="mt-2 flex-1 text-sm leading-relaxed text-fd-muted-foreground">{body}</p>
              <span className="mt-5 inline-flex items-center gap-1 text-sm font-medium text-fd-primary">
                {cta} <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
              </span>
            </Link>
          ))}
        </div>
      </section>

      {/* Data */}
      <section id="data" className="relative isolate overflow-hidden border-y">
        <div aria-hidden="true" className="tg-grid absolute inset-0 -z-10 opacity-60" />
        <div className="mx-auto grid w-full max-w-6xl gap-12 px-6 py-20 md:py-28 lg:grid-cols-2">
          <div data-reveal="">
            <Eyebrow text="Data you can keep" />
            <h2 className="text-3xl font-semibold tracking-tight text-balance md:text-5xl">
              <Words>Right first, then complete</Words>
            </h2>
            <p className="mt-5 text-lg text-fd-muted-foreground text-pretty">
              Coordinates and elevation come first, then names, then population. Every target is a
              gate a build must pass, and the measured figures ship inside each pack. Optional values
              are null, never guessed.
            </p>
            <ul className="mt-8 space-y-4 text-sm">
              {[
                [ShieldCheck, 'Signed and verified', 'Every section hashed, every pack signed; the store checks each byte before use.'],
                [FileCheck2, 'Yours to store and ship', 'Main build under ODbL, lite build under CC BY 4.0, every source credited.'],
                [Globe2, 'One stated perspective', "Disputed borders drawn from Nepal's official map of 2020, recorded with sources."],
              ].map(([Icon, title, body]) => {
                const I = Icon as typeof ShieldCheck;
                return (
                  <li key={title as string} className="flex gap-3">
                    <I className="mt-0.5 size-4 shrink-0 text-fd-primary" aria-hidden="true" />
                    <span>
                      <span className="font-medium">{title as string}.</span>{' '}
                      <span className="text-fd-muted-foreground">{body as string}</span>
                    </span>
                  </li>
                );
              })}
            </ul>
          </div>
          <div data-reveal="scale" style={delay(120)} className="grid content-start gap-4 sm:grid-cols-2">
            {[
              ['Nepal', '95,716', 'places down to 2,622 wards, in Nepali, English and Newari'],
              ['India', '8,111', 'districts and statutory towns'],
              ['The world', '248', 'countries and territories, first-level units and cities of 5,000+'],
              ['Timezones', '377', 'IANA zone areas, with the tzdb rules in the pack'],
            ].map(([title, value, body]) => (
              <div key={title} className="tg-card p-6">
                <p className="text-sm text-fd-muted-foreground">{title}</p>
                <p className="mt-2 text-3xl font-semibold tracking-tight tabular-nums">{value}</p>
                <p className="mt-1.5 text-sm text-fd-muted-foreground">{body}</p>
              </div>
            ))}
            <Link
              href="/docs/data/coverage/"
              className="group inline-flex items-center gap-1 text-sm font-medium text-fd-primary sm:col-span-2"
            >
              Coverage of the released pack <ArrowRight className="size-3.5 transition-transform group-hover:translate-x-0.5" />
            </Link>
          </div>
        </div>
      </section>

      {/* FAQ */}
      <Section id="faq" eyebrow="Questions" title="Frequently asked">
        <div className="divide-y rounded-2xl border bg-fd-card/40">
          {faq.map(({ q, a }) => (
            <details key={q} data-reveal="" className="group px-6 py-5 [&_summary::-webkit-details-marker]:hidden">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-6 font-medium">
                {q}
                <span
                  aria-hidden="true"
                  className="flex size-6 shrink-0 items-center justify-center rounded-full border text-fd-muted-foreground transition-transform duration-300 group-open:rotate-45"
                >
                  +
                </span>
              </summary>
              <p className="mt-3 max-w-3xl text-fd-muted-foreground">{a}</p>
            </details>
          ))}
        </div>
      </Section>

      {/* Call to action */}
      <section className="mx-auto w-full max-w-6xl px-6 pb-24">
        <div
          data-reveal="scale"
          className="relative isolate overflow-hidden rounded-3xl border bg-fd-card px-8 py-16 text-center md:py-20"
        >
          <div className="absolute inset-0 -z-10 opacity-60">
            <Contours />
          </div>
          <h2 className="mx-auto max-w-2xl text-3xl font-semibold tracking-tight text-balance md:text-5xl">
            Put the world&apos;s places in your app today.
          </h2>
          <p className="mx-auto mt-5 max-w-xl text-lg text-fd-muted-foreground">
            Download a pack, open it, ask. Five minutes from nothing to answers.
          </p>
          <div className="mt-9 flex flex-wrap justify-center gap-3">
            <ButtonLink href="/docs/getting-started/" primary>
              Get started <ArrowRight className="size-4" />
            </ButtonLink>
            <ButtonLink href={site.github}>Star on GitHub</ButtonLink>
          </div>
        </div>
      </section>
    </main>
  );
}
