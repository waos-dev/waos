import type { ReactNode } from 'react';

interface Moment {
  title: string;
  detail: string;
  /** WaOS's own apps that do it, by their names and icons in `public/apps`. */
  apps: readonly { name: string; icon: string }[];
  picture: ReactNode;
}

const moments: readonly Moment[] = [
  {
    title: 'GPU and 3D',
    detail: 'WebGPU for 3D scenes, shaders and video effects.',
    apps: [{ name: 'Model Viewer', icon: '/apps/model-viewer.png' }],
    picture: <Scene />,
  },
  {
    title: 'Camera, mic and audio',
    detail: 'Live capture, playback and sound made in the app.',
    apps: [
      { name: 'Camera', icon: '/apps/camera.png' },
      { name: 'Synth', icon: '/apps/synth.png' },
    ],
    picture: <Capture />,
  },
  {
    title: 'Files and local tools',
    detail: 'Open and save dialogs, watched folders and commands.',
    apps: [
      { name: 'Documents', icon: '/apps/documents.png' },
      { name: 'Terminal', icon: '/apps/terminal.png' },
    ],
    picture: <Files />,
  },
  {
    title: 'Windows and background work',
    detail: 'Many windows, notifications and workers.',
    apps: [{ name: 'Reader', icon: '/apps/reader.png' }],
    picture: <Background />,
  },
];

/**
 * Four things an app can do, each a picture of the moment it does it and the WaOS apps that
 * already do: a 3D scene, a live capture with its sound, a save dialog and a shell, and a
 * notification over an app's windows. Each picture is an SVG of one size, scaled to its tile.
 */
export function CapabilityMoments() {
  return (
    <div className="grid gap-3 p-3 sm:grid-cols-2 sm:p-4">
      {moments.map((moment) => (
        <div key={moment.title} className="flex flex-col overflow-hidden rounded-2xl border bg-fd-background">
          <div className="p-2 pb-0">{moment.picture}</div>
          <div className="flex items-start gap-3 px-4 pt-3 pb-4">
            <div className="min-w-0 flex-1">
              <p className="text-[15px] font-semibold tracking-tight">{moment.title}</p>
              <p className="mt-0.5 text-[13px] leading-snug text-fd-muted-foreground">{moment.detail}</p>
              <p className="mt-2 flex items-center gap-1.5 text-[12px] text-fd-muted-foreground">
                <span className="flex -space-x-1">
                  {moment.apps.map((app) => (
                    // eslint-disable-next-line @next/next/no-img-element -- a small icon the site already serves
                    <img key={app.name} src={app.icon} alt="" width={16} height={16} className="size-4 rounded-[4px] ring-1 ring-fd-background" />
                  ))}
                </span>
                As in {moment.apps.map((app) => app.name).join(' and ')}
              </p>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

const WIDTH = 280;
const HEIGHT = 150;

function Picture({ children }: { children: ReactNode }) {
  return (
    <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} className="block h-auto w-full rounded-xl">
      {children}
    </svg>
  );
}

type Vec = readonly [number, number, number];

function normalize([x, y, z]: Vec): Vec {
  const length = Math.hypot(x, y, z);
  return [x / length, y / length, z / length];
}

/** A geodesic sphere's faces: an icosahedron's, each split in four and pushed out to the sphere. */
function sphereFaces(): Vec[][] {
  const t = (1 + Math.sqrt(5)) / 2;
  const corners: Vec[] = [
    [-1, t, 0], [1, t, 0], [-1, -t, 0], [1, -t, 0],
    [0, -1, t], [0, 1, t], [0, -1, -t], [0, 1, -t],
    [t, 0, -1], [t, 0, 1], [-t, 0, -1], [-t, 0, 1],
  ].map((corner) => normalize(corner as unknown as Vec));
  const faces = [
    [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
    [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
    [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
  ];
  const middle = (a: Vec, b: Vec) => normalize([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2]);
  return faces.flatMap(([i, j, k]) => {
    const [a, b, c] = [corners[i], corners[j], corners[k]];
    const [ab, bc, ca] = [middle(a, b), middle(b, c), middle(c, a)];
    return [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]];
  });
}

function turn([x, y, z]: Vec, yaw: number, pitch: number): Vec {
  const [cy, sy, cp, sp] = [Math.cos(yaw), Math.sin(yaw), Math.cos(pitch), Math.sin(pitch)];
  const x1 = x * cy + z * sy;
  const z1 = -x * sy + z * cy;
  return [x1, y * cp - z1 * sp, y * sp + z1 * cp];
}

/** The sphere's faces that face the viewer, farthest first, each with its points and how lit it is. */
const sphere = (() => {
  const light = normalize([-0.55, 0.7, 0.75]);
  const [cx, cy, radius] = [WIDTH / 2, 68, 50];
  return sphereFaces()
    .map((face) => face.map((point) => turn(point, 0.5, 0.32)))
    .map((face) => {
      const centre = normalize([
        (face[0][0] + face[1][0] + face[2][0]) / 3,
        (face[0][1] + face[1][1] + face[2][1]) / 3,
        (face[0][2] + face[1][2] + face[2][2]) / 3,
      ]);
      const lit = Math.max(0, centre[0] * light[0] + centre[1] * light[1] + centre[2] * light[2]);
      const points = face.map(([x, y]) => `${(cx + x * radius).toFixed(1)},${(cy - y * radius).toFixed(1)}`).join(' ');
      return { depth: centre[2], across: centre[0], lit, points };
    })
    .filter((face) => face.depth > 0)
    .sort((a, b) => a.depth - b.depth);
})();

function mix(from: readonly number[], to: readonly number[], t: number): string {
  const [r, g, b] = from.map((value, index) => Math.round(value + (to[index] - value) * t));
  return `rgb(${r},${g},${b})`;
}

/** Model Viewer's moment: a lit, faceted sphere on a grid floor, its wireframe over one side. */
function Scene() {
  const floor = Array.from({ length: 9 }, (_, index) => index - 4);
  return (
    <Picture>
      <defs>
        <radialGradient id="scene-sky" cx="0.5" cy="0.35" r="0.8">
          <stop offset="0" stopColor="#1d2340" />
          <stop offset="1" stopColor="#0a0c14" />
        </radialGradient>
        <radialGradient id="scene-shadow">
          <stop offset="0" stopColor="#000" stopOpacity="0.55" />
          <stop offset="1" stopColor="#000" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect width={WIDTH} height={HEIGHT} fill="url(#scene-sky)" />
      <g stroke="#8ea0ff" strokeOpacity="0.16" strokeWidth="0.6">
        {floor.map((step) => (
          <line key={`depth${step}`} x1={WIDTH / 2 + step * 14} y1={118} x2={WIDTH / 2 + step * 44} y2={HEIGHT} />
        ))}
        {[118, 124, 132, 141].map((y) => (
          <line key={`across${y}`} x1={0} y1={y} x2={WIDTH} y2={y} />
        ))}
      </g>
      <ellipse cx={WIDTH / 2} cy={124} rx={46} ry={7} fill="url(#scene-shadow)" />
      {sphere.map((face, index) => (
        <polygon
          key={index}
          points={face.points}
          fill={mix([46, 38, 140], [168, 190, 255], 0.12 + face.lit * 0.88)}
          stroke={face.across > 0.15 ? '#e0e7ff' : '#0b0d18'}
          strokeOpacity={face.across > 0.15 ? 0.6 : 0.25}
          strokeWidth="0.5"
          strokeLinejoin="round"
        />
      ))}
      <g strokeWidth="1.2" strokeLinecap="round">
        <line x1="20" y1="134" x2="34" y2="134" stroke="#f87171" />
        <line x1="20" y1="134" x2="20" y2="120" stroke="#4ade80" />
        <line x1="20" y1="134" x2="12" y2="140" stroke="#60a5fa" />
      </g>
    </Picture>
  );
}

/** Camera's and Synth's moment: a framed, focused picture, and the sound it hears as a wave. */
function Capture() {
  const bars = Array.from({ length: 46 }, (_, index) => {
    const swell = Math.abs(Math.sin(index * 0.33) * 0.65 + Math.sin(index * 0.91) * 0.35);
    return 3 + swell * 15;
  });
  const corner = (x: number, y: number, dx: number, dy: number) => `M${x} ${y + dy}V${y}H${x + dx}`;
  return (
    <Picture>
      <defs>
        <linearGradient id="capture-sky" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#5b6fd6" />
          <stop offset="0.55" stopColor="#e58bb1" />
          <stop offset="1" stopColor="#ffc58a" />
        </linearGradient>
        <linearGradient id="capture-wave" x1="0" x2="1">
          <stop offset="0" stopColor="#3b82f6" />
          <stop offset="0.55" stopColor="#d946ef" />
          <stop offset="1" stopColor="#fb923c" />
        </linearGradient>
      </defs>
      <rect width={WIDTH} height={HEIGHT} fill="#0c0c0e" />
      <g>
        <rect x="8" y="8" width={WIDTH - 16} height="100" rx="8" fill="url(#capture-sky)" />
        <circle cx="196" cy="62" r="13" fill="#fff4d6" opacity="0.9" />
        <path d="M8 86 C 50 66, 84 72, 120 82 S 196 70, 272 80 V100 a8 8 0 0 1 -8 8 H16 a8 8 0 0 1 -8 -8Z" fill="#3a2a5c" opacity="0.75" />
        <path d="M8 96 C 60 84, 110 92, 160 90 S 236 86, 272 94 V100 a8 8 0 0 1 -8 8 H16 a8 8 0 0 1 -8 -8Z" fill="#1f1838" opacity="0.9" />
      </g>
      <g fill="none" stroke="#fff" strokeWidth="1.4" strokeOpacity="0.9" strokeLinecap="round">
        <path d={corner(18, 18, 12, 12)} />
        <path d={corner(WIDTH - 18, 18, -12, 12)} />
        <path d={corner(18, 98, 12, -12)} />
        <path d={corner(WIDTH - 18, 98, -12, -12)} />
      </g>
      <rect x="120" y="40" width="40" height="40" rx="3" fill="none" stroke="#ffd60a" strokeWidth="1.2" />
      <circle cx="22" cy="129" r="4" fill="#ff453a" />
      <g fill="url(#capture-wave)">
        {bars.map((height, index) => (
          <rect key={index} x={36 + index * 5.1} y={129 - height / 2} width="2.6" height={height} rx="1.3" />
        ))}
      </g>
    </Picture>
  );
}

/** Documents' and Terminal's moment: the system's save dialog, and a shell beside it. */
function Files() {
  const field = { fill: '#fff', stroke: 'rgba(0,0,0,0.14)', strokeWidth: 0.6 };
  return (
    <Picture>
      <rect width={WIDTH} height={HEIGHT} fill="#e9edf3" />
      <rect x="0" y="0" width={WIDTH} height={HEIGHT} fill="url(#files-glow)" />
      <defs>
        <radialGradient id="files-glow" cx="0.2" cy="0" r="1">
          <stop offset="0" stopColor="#ffffff" stopOpacity="0.9" />
          <stop offset="1" stopColor="#ffffff" stopOpacity="0" />
        </radialGradient>
        <filter id="files-shadow" x="-20%" y="-20%" width="140%" height="160%">
          <feDropShadow dx="0" dy="6" stdDeviation="7" floodColor="#0f172a" floodOpacity="0.22" />
        </filter>
      </defs>
      <g filter="url(#files-shadow)">
        <rect x="14" y="12" width="190" height="104" rx="9" fill="#f6f6f7" stroke="rgba(0,0,0,0.12)" strokeWidth="0.6" />
      </g>
      <g fontSize="8.5" fill="#1d1d1f">
        <text x="30" y="36" fill="#6e6e73">Save As:</text>
        <rect x="72" y="26" width="118" height="15" rx="3.5" {...field} />
        <text x="78" y="36.5">Chapter 3.typ</text>
        <text x="40.5" y="58" fill="#6e6e73">Where:</text>
        <rect x="72" y="48" width="118" height="15" rx="3.5" {...field} />
        <path d="M78 52.5h3.4l1.2 1.3h5.4v5.7h-10z" fill="#5fa8f5" />
        <text x="92" y="58.5">Documents</text>
        <path d="M181 54l2.5 2.5 2.5-2.5" fill="none" stroke="#6e6e73" strokeWidth="0.9" />
        <rect x="92" y="90" width="44" height="15" rx="3.5" fill="#fff" stroke="rgba(0,0,0,0.14)" strokeWidth="0.6" />
        <text x="114" y="100.5" textAnchor="middle">Cancel</text>
        <rect x="142" y="90" width="48" height="15" rx="3.5" fill="#0a63e0" />
        <text x="166" y="100.5" textAnchor="middle" fill="#fff" fontWeight="600">
          Save
        </text>
      </g>
      <g filter="url(#files-shadow)">
        <rect x="132" y="70" width="136" height="70" rx="8" fill="#16171b" />
      </g>
      <circle cx="142" cy="79" r="2.4" fill="#ff5f57" />
      <circle cx="150" cy="79" r="2.4" fill="#febc2e" />
      <circle cx="158" cy="79" r="2.4" fill="#28c840" />
      <g fontSize="8" fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace" fill="#e5e7eb">
        <text x="141" y="98">
          <tspan fill="#86efac">~/Documents</tspan> % git pull
        </text>
        <text x="141" y="110" fill="#9ca3af">
          Already up to date.
        </text>
        <text x="141" y="122">
          <tspan fill="#86efac">~/Documents</tspan> %
        </text>
        <rect x="200" y="115" width="4.5" height="9" fill="#e5e7eb" opacity="0.85" />
      </g>
    </Picture>
  );
}

/** Reader's moment: two of its windows, and the notification it posts of new items. */
function Background() {
  const lines = (x: number, y: number, widths: number[]) =>
    widths.map((width, index) => <rect key={index} x={x} y={y + index * 8} width={width} height="3.2" rx="1.6" fill="rgba(0,0,0,0.12)" />);
  return (
    <Picture>
      <defs>
        <linearGradient id="background-desk" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#c9d8f5" />
          <stop offset="1" stopColor="#ecd3e6" />
        </linearGradient>
        <filter id="background-shadow" x="-20%" y="-20%" width="140%" height="160%">
          <feDropShadow dx="0" dy="5" stdDeviation="6" floodColor="#0f172a" floodOpacity="0.2" />
        </filter>
      </defs>
      <rect width={WIDTH} height={HEIGHT} fill="url(#background-desk)" />
      <g filter="url(#background-shadow)">
        <rect x="16" y="30" width="150" height="104" rx="7" fill="#fbfbfb" />
      </g>
      <rect x="16" y="30" width="150" height="14" rx="7" fill="#ececec" />
      <rect x="16" y="37" width="150" height="7" fill="#ececec" />
      {[24, 31, 38].map((x, index) => (
        <circle key={x} cx={x} cy={37} r="2.2" fill={['#ff5f57', '#febc2e', '#28c840'][index]} />
      ))}
      <rect x="16" y="44" width="46" height="90" fill="#f3f3f3" />
      {lines(22, 52, [30, 24, 34, 20, 28])}
      <rect x="20" y="49" width="2" height="7" rx="1" fill="#005fb8" />
      {lines(70, 52, [70, 84, 60, 80, 76, 50, 72, 64, 40])}
      <g filter="url(#background-shadow)">
        <rect x="98" y="58" width="140" height="84" rx="7" fill="#fbfbfb" />
      </g>
      <rect x="98" y="58" width="140" height="14" rx="7" fill="#ececec" />
      <rect x="98" y="65" width="140" height="7" fill="#ececec" />
      {[106, 113, 120].map((x, index) => (
        <circle key={x} cx={x} cy={65} r="2.2" fill={['#ff5f57', '#febc2e', '#28c840'][index]} />
      ))}
      <rect x="108" y="80" width="56" height="34" rx="3" fill="url(#background-desk)" opacity="0.9" />
      {lines(170, 82, [56, 48, 58, 40])}
      {lines(108, 120, [110, 96])}
      <g filter="url(#background-shadow)">
        <rect x="150" y="10" width="122" height="38" rx="9" fill="rgba(246,246,248,0.97)" stroke="rgba(0,0,0,0.08)" strokeWidth="0.5" />
      </g>
      <image href="/apps/reader.png" x="158" y="18" width="22" height="22" />
      <text x="186" y="26" fontSize="8.5" fontWeight="600" fill="#1d1d1f">
        3 new items
      </text>
      <text x="186" y="37.5" fontSize="8" fill="#6e6e73">
        NASA, Rust Blog
      </text>
      <text x="264" y="26" fontSize="7" fill="#8e8e93" textAnchor="end">
        now
      </text>
    </Picture>
  );
}
