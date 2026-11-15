import Link from 'next/link';
import {
  Boxes,
  Clock,
  Globe2,
  Languages,
  Mountain,
  Network,
  Search,
  ShieldCheck,
  WifiOff,
} from 'lucide-react';
import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';

const features = [
  {
    icon: Search,
    title: 'Search in every script',
    body: 'काठमाडौं, Kathmandu and Kathamandu find the same place. "Name, Parent" ranks the place in that parent first; a focus point ranks what is near.',
  },
  {
    icon: Globe2,
    title: 'Reverse and admin',
    body: 'From a point to the nearest places with their distance and bearing, and to every admin unit that holds it, country first.',
  },
  {
    icon: Clock,
    title: 'Exact timezones',
    body: 'The IANA zone at any point as its boundaries draw it, offsets and transitions from the zone rules, local times resolved, tzdb version stated.',
  },
  {
    icon: Mountain,
    title: 'Elevation',
    body: 'A height for every place and at any point with terrain: metres above EGM2008, interpolated between samples, with the source named.',
  },
  {
    icon: WifiOff,
    title: 'Offline first',
    body: 'Packs are memory-mapped files. Bundle one in your app, or download a country when a user needs it; queries never touch the network.',
  },
  {
    icon: ShieldCheck,
    title: 'Signed and verified',
    body: 'Every section is hashed and every pack signed. The store checks each byte against a signed catalog before it is used.',
  },
  {
    icon: Boxes,
    title: 'Pay for what you use',
    body: 'Packs split into components (points, names, search, boundaries, timezones, terrain). Install and link only the ones you need.',
  },
  {
    icon: Network,
    title: 'Servers included',
    body: 'tgd serve answers HTTP, GeoJSON and Nominatim-compatible routes; tgd mcp gives AI agents the same queries over MCP.',
  },
  {
    icon: Languages,
    title: 'One API everywhere',
    body: 'Rust, C, C++, Swift, Kotlin and Java, Dart and Flutter, Python, Node.js, WebAssembly, Go and .NET, generated from one definition.',
  },
];

const languages = [
  ['Rust', 'rust'],
  ['Python', 'python'],
  ['Node.js', 'node'],
  ['Browser (WebAssembly)', 'wasm'],
  ['Swift', 'swift'],
  ['Kotlin and Java', 'kotlin-java'],
  ['Dart and Flutter', 'dart-flutter'],
  ['Go', 'go'],
  ['C', 'c'],
  ['C++', 'cpp'],
  ['.NET', 'dotnet'],
  ['Command line', '../cli'],
] as const;

const sample = `import teigeodata
from teigeodata.blob import Reader

engine = teigeodata.Engine()
engine.open_file("world-np-lite.tgd")

for place in Reader(engine.search("Pokhara", 2)):
    print(place["id"], place["name"], place["kind"])
# tgd:np/1xhb Pokhara settlement
# tgd:np/1xnr Pokhara admin

(area,) = Reader(engine.tz_at(27.7172, 85.3240))
print(area["tz"])  # Asia/Kathmandu`;

export default function HomePage() {
  return (
    <main className="flex flex-col flex-1">
      <section className="relative overflow-hidden border-b">
        <div
          aria-hidden="true"
          className="absolute inset-0 opacity-[0.35] [background-image:radial-gradient(circle_at_20%_20%,var(--color-fd-primary)_0,transparent_45%),radial-gradient(circle_at_85%_60%,var(--color-fd-primary)_0,transparent_40%)] [mask-image:linear-gradient(to_bottom,black,transparent)]"
        />
        <div className="relative mx-auto grid max-w-6xl gap-10 px-6 py-20 md:grid-cols-[1.1fr_1fr] md:py-28">
          <div className="flex flex-col justify-center">
            <p className="mb-4 text-sm font-medium text-fd-primary">Open source · Apache-2.0 code · open data</p>
            <h1 className="text-4xl font-bold leading-tight tracking-tight md:text-5xl">
              Accurate places,
              <br />
              offline, in every language.
            </h1>
            <p className="mt-5 max-w-xl text-lg text-fd-muted-foreground">
              Coordinates, elevation, the admin hierarchy to the lowest level, boundaries, local names,
              exact timezones and postal codes, cleaned and deduplicated from the best open sources, in
              small signed packs you ship with your app or download when you need them.
            </p>
            <div className="mt-8 flex flex-wrap gap-3">
              <Link
                href="/docs/getting-started"
                className="rounded-lg bg-fd-primary px-5 py-2.5 text-sm font-medium text-fd-primary-foreground hover:opacity-90"
              >
                Get started
              </Link>
              <Link href="/docs" className="rounded-lg px-5 py-2.5 text-sm font-medium hover:bg-fd-accent">
                Read the docs →
              </Link>
            </div>
          </div>
          <div className="min-w-0 self-center">
            <DynamicCodeBlock lang="python" code={sample} />
            <p className="mt-3 text-xs text-fd-muted-foreground">
              The same calls in every language; see the guides.
            </p>
          </div>
        </div>
      </section>

      <section className="mx-auto w-full max-w-6xl px-6 py-16">
        <h2 className="text-2xl font-semibold tracking-tight">What it answers</h2>
        <div className="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {features.map(({ icon: Icon, title, body }) => (
            <div key={title} className="rounded-xl border bg-fd-card p-5">
              <Icon className="size-5 text-fd-primary" aria-hidden="true" />
              <h3 className="mt-3 font-semibold">{title}</h3>
              <p className="mt-1.5 text-sm text-fd-muted-foreground">{body}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="border-t bg-fd-secondary/40">
        <div className="mx-auto w-full max-w-6xl px-6 py-16">
          <h2 className="text-2xl font-semibold tracking-tight">Pick your language</h2>
          <p className="mt-2 text-fd-muted-foreground">
            Each guide takes you from install to every query, with programs that ran on a released pack.
          </p>
          <div className="mt-8 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {languages.map(([name, slug]) => (
              <Link
                key={slug}
                href={slug.startsWith('../') ? `/docs/${slug.slice(3)}` : `/docs/guides/${slug}`}
                className="rounded-lg border bg-fd-card px-4 py-3 text-sm font-medium transition-colors hover:border-fd-primary hover:text-fd-primary"
              >
                {name}
              </Link>
            ))}
          </div>
        </div>
      </section>

      <section className="mx-auto w-full max-w-6xl px-6 py-16">
        <div className="grid gap-8 md:grid-cols-2">
          <div>
            <h2 className="text-2xl font-semibold tracking-tight">Data you are allowed to keep</h2>
            <p className="mt-3 text-fd-muted-foreground">
              Commercial geocoding APIs limit how long you may store their answers. teigeodata&apos;s data
              is yours to store, ship and redistribute: the main build under ODbL, the lite build under CC
              BY 4.0, with every source credited in every pack and every answer.
            </p>
            <Link href="/docs/concepts/licences" className="mt-4 inline-block text-sm font-medium text-fd-primary">
              Licences and attribution →
            </Link>
          </div>
          <div>
            <h2 className="text-2xl font-semibold tracking-tight">Coverage grows by data releases</h2>
            <p className="mt-3 text-fd-muted-foreground">
              Nepal in full today, with the world&apos;s countries, first-level units and cities of 5,000 or
              more, and India&apos;s districts and towns. Then India, South Asia, Asia, the USA and the rest of
              the world.
            </p>
            <Link href="/docs/data" className="mt-4 inline-block text-sm font-medium text-fd-primary">
              Countries and sources →
            </Link>
          </div>
        </div>
      </section>
    </main>
  );
}
