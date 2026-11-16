import { ImageResponse } from 'next/og';

// The square icon as a PNG, for the places that take no SVG (apple-touch-icon, schema.org logo).
export const dynamic = 'force-static';

export function GET() {
  return new ImageResponse(
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: '#0b6e6e',
        borderRadius: 112,
      }}
    >
      <svg width="360" height="360" viewBox="0 0 64 64">
        <path
          d="M32 12c-8.3 0-15 6.4-15 14.7 0 10.6 11.8 24.2 13.8 26.5a1.6 1.6 0 0 0 2.4 0c2-2.3 13.8-15.9 13.8-26.5C47 18.4 40.3 12 32 12Z"
          fill="rgba(255,255,255,0.22)"
          stroke="#fff"
          strokeWidth="3"
        />
        <circle cx="32" cy="26.5" r="5.5" fill="#fff" />
      </svg>
    </div>,
    { width: 512, height: 512 },
  );
}
