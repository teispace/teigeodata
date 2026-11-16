import { ImageResponse } from 'next/og';
import { cards } from '@/lib/og-pages';
import { site } from '@/lib/site';

// Rendered once per page at build time (static export).
export const dynamic = 'force-static';

interface Props {
  params: Promise<{ slug: string[] }>;
}

/** Contour rings for the card's corner, the same shapes as the site's hero. */
function rings(): string[] {
  return Array.from({ length: 12 }, (_, k) => {
    const r = 30 + k * 34;
    const pts: string[] = [];
    for (let i = 0; i <= 72; i++) {
      const a = (i / 72) * Math.PI * 2;
      const w = 1 + 0.16 * Math.sin(3 * a + 0.4 + k * 0.21) + 0.08 * Math.sin(5 * a - 0.7 + k * 0.13);
      pts.push(`${i ? 'L' : 'M'}${(1060 + Math.cos(a) * r * w * 1.35).toFixed(1)} ${(90 + Math.sin(a) * r * w).toFixed(1)}`);
    }
    return `${pts.join('')}Z`;
  });
}

export async function GET(_request: Request, { params }: Props) {
  const { slug } = await params;
  const key = slug.slice(0, -1).join('/');
  const card = cards().find((c) => c.slug.join('/') === key);
  if (!card) return new Response('Not found', { status: 404 });
  return new ImageResponse(
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
        padding: '72px 80px',
        backgroundColor: '#07100f',
        backgroundImage:
          'radial-gradient(900px 480px at 85% -10%, rgba(32,178,170,0.32), transparent), radial-gradient(700px 420px at -10% 120%, rgba(32,178,170,0.12), transparent)',
        color: '#f5f7f7',
        position: 'relative',
      }}
    >
      <svg width="1200" height="630" viewBox="0 0 1200 630" style={{ position: 'absolute', top: 0, left: 0 }}>
        {rings().map((d, k) => (
          <path key={d} d={d} fill="none" stroke="#3ccfc4" strokeWidth={k % 5 === 4 ? 1.6 : 1} strokeOpacity={k % 5 === 4 ? 0.35 : 0.18} />
        ))}
      </svg>
      <div style={{ display: 'flex', alignItems: 'center', gap: 18 }}>
        <svg width="52" height="52" viewBox="0 0 24 24" fill="none">
          <path
            d="M12 2.5c-3.6 0-6.5 2.8-6.5 6.4 0 4.6 5.1 10.5 6 11.5a.7.7 0 0 0 1 0c.9-1 6-6.9 6-11.5 0-3.6-2.9-6.4-6.5-6.4Z"
            stroke="#5fe0d5"
            strokeWidth="1.6"
            fill="rgba(95,224,213,0.18)"
          />
          <circle cx="12" cy="9" r="2.4" fill="#5fe0d5" />
        </svg>
        <span style={{ fontSize: 40, letterSpacing: -0.5 }}>{site.name}</span>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 22 }}>
        <span style={{ fontSize: 24, textTransform: 'uppercase', letterSpacing: 4, color: '#5fe0d5' }}>
          {card.eyebrow}
        </span>
        <span style={{ fontSize: card.title.length > 40 ? 64 : 76, lineHeight: 1.08, letterSpacing: -2, maxWidth: 1000 }}>
          {card.title}
        </span>
        {card.description ? (
          <span style={{ fontSize: 30, lineHeight: 1.35, color: '#9fb0ae', maxWidth: 980 }}>
            {card.description.length > 150 ? `${card.description.slice(0, 147).trimEnd()}…` : card.description}
          </span>
        ) : null}
      </div>
      <span style={{ fontSize: 24, color: '#6b7d7b' }}>teigeodata.teispace.com</span>
    </div>,
    { width: 1200, height: 630 },
  );
}

export function generateStaticParams() {
  return cards().map((card) => ({ slug: [...card.slug, 'image.png'] }));
}
