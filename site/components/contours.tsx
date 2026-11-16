/**
 * The hero's backdrop: contour lines around two summits, drawn once at build time. A ring's
 * radius wobbles with a few fixed sine terms, so the map is the same on every build and costs
 * no script.
 */
const peaks = [
  { x: 980, y: 210, rings: 15, step: 30, phase: 0.4 },
  { x: 300, y: 560, rings: 10, step: 34, phase: 2.1 },
];

function ring(cx: number, cy: number, r: number, phase: number, k: number): string {
  const points: string[] = [];
  const n = 96;
  for (let i = 0; i <= n; i++) {
    const a = (i / n) * Math.PI * 2;
    const wobble =
      1 +
      0.16 * Math.sin(3 * a + phase + k * 0.21) +
      0.08 * Math.sin(5 * a - phase * 1.7 + k * 0.13) +
      0.05 * Math.cos(7 * a + k * 0.37);
    const x = cx + Math.cos(a) * r * wobble * 1.35;
    const y = cy + Math.sin(a) * r * wobble;
    points.push(`${i === 0 ? 'M' : 'L'}${x.toFixed(1)} ${y.toFixed(1)}`);
  }
  return `${points.join('')}Z`;
}

export function Contours() {
  return (
    <svg
      aria-hidden="true"
      className="tg-contours pointer-events-none absolute inset-0 h-full w-full"
      viewBox="0 0 1200 760"
      preserveAspectRatio="xMidYMid slice"
      fill="none"
    >
      {peaks.flatMap((p, pi) =>
        Array.from({ length: p.rings }, (_, k) => (
          <path
            key={`${pi}-${k}`}
            d={ring(p.x, p.y, 18 + k * p.step, p.phase, k)}
            stroke="currentColor"
            strokeWidth={k % 5 === 4 ? 1.2 : 0.7}
            opacity={k % 5 === 4 ? 0.55 : 0.32}
            pathLength={1}
            style={{ '--k': k } as React.CSSProperties}
          />
        )),
      )}
    </svg>
  );
}
