import type { LucideIcon } from 'lucide-react';
import {
  Activity,
  Camera,
  ChevronUp,
  Folder,
  Globe,
  Plug,
  RotateCw,
  Search,
  Shield,
  Square,
  SquareTerminal,
  X,
} from 'lucide-react';
import { showcase } from './app';
import { fluent } from './fluent';
import { MacWindow } from './mac-window';
import { BreezeIcon } from './wapk-file';

/**
 * WaOS's Inspector watching Breeze, as its Fluent layout draws a program's page
 * (`builtin-apps/inspector/src/ui/fluent`): the header with the program's commands, the tabs of
 * its report and live panels, each with its count, and the Network panel listening, its refused
 * request selected beside its detail. Its words are the app's own (`src/words.rs`).
 *
 * `rows` is how many of the panel's events have come in, `detail` whether the refused one is
 * selected. In a narrow window the columns that step out are out, and a refused row says why
 * beneath it, as the app's phone layout does.
 */
export function InspectorWindow({
  rows,
  detail,
  className = '',
}: {
  rows: number;
  detail: boolean;
  className?: string;
}) {
  const heard = events.slice(0, rows);
  const refused = heard.filter((event) => event.refused).length;
  const selected = detail && refused > 0;
  return (
    <MacWindow title="Inspector" className={className}>
      <div className="@container flex size-full flex-col" style={{ background: fluent.layer, color: fluent.text }}>
        <Header />
        <Tabs heard={heard.length} refused={refused} />
        <Toolbar heard={heard} refused={refused} />
        <div className="flex min-h-0 flex-1" style={{ borderTop: `1px solid ${fluent.stroke}` }}>
          <Table heard={heard} selected={selected} />
          {selected && <Detail />}
        </div>
        <StatusBar heard={heard.length} refused={refused} />
      </div>
    </MacWindow>
  );
}

type Kind = 'Lookup' | 'Connect' | 'Request';

interface NetworkEvent {
  kind: Kind;
  name: string;
  status?: string;
  refused?: boolean;
  took?: string;
  started: string;
}

const events: readonly NetworkEvent[] = [
  { kind: 'Lookup', name: 'api.open-meteo.com', started: '10:42:07.106' },
  { kind: 'Connect', name: 'api.open-meteo.com (188.40.89.12:443)', started: '10:42:07.131' },
  {
    kind: 'Request',
    name: 'GET https://geocoding-api.open-meteo.com/v1/search?name=Lisbon',
    status: '200',
    took: '96 ms',
    started: '10:42:07.140',
  },
  {
    kind: 'Request',
    name: 'GET https://api.open-meteo.com/v1/forecast?latitude=38.72&longitude=-9.14',
    status: '200',
    took: '184 ms',
    started: '10:42:07.262',
  },
  { kind: 'Request', name: 'POST https://telemetry.skyfeed.dev/v1/events', refused: true, started: '10:42:07.270' },
  {
    kind: 'Request',
    name: 'GET https://api.open-meteo.com/v1/forecast?latitude=59.91&longitude=10.75',
    status: '200',
    took: '171 ms',
    started: '10:42:09.018',
  },
];

/** The domain rule's words, as `refused_by("domains")` says them. */
const refusedBecause =
  "The domain is not among those it may reach. Change it under Network, in its settings in WaOS's library.";

const kindGlyph: Record<Kind, LucideIcon> = { Lookup: Search, Connect: Plug, Request: Globe };

function Header() {
  return (
    <div className="flex items-center gap-3 px-4 pt-3 pb-2">
      <BreezeIcon size={32} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[15px] leading-tight font-semibold">{showcase.name}</p>
        <p className="mt-0.5 flex items-center gap-1.5 text-[11px]" style={{ color: fluent.secondary }}>
          <span className="font-mono text-[10.5px]">{showcase.id}</span>
          <span className="size-[3px] rounded-full" style={{ background: fluent.tertiary }} />
          <span className="size-[7px] rounded-full" style={{ background: fluent.success }} />
          Running
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-0.5 text-[11.5px]">
        <span className="flex items-center gap-1.5 rounded-[4px] px-2 py-1">
          <RotateCw aria-hidden className="size-3.5" />
          <span className="@max-[440px]:hidden">Restart inspected</span>
        </span>
        <span className="flex items-center gap-1.5 rounded-[4px] px-2 py-1">
          <Square aria-hidden className="size-3" />
          <span className="@max-[440px]:hidden">Stop</span>
        </span>
      </div>
    </div>
  );
}

function Badge({ value, critical }: { value: number; critical?: boolean }) {
  return (
    <span
      className="flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-[10px] leading-none font-semibold text-white tabular-nums"
      style={{ background: critical ? fluent.critical : fluent.accent }}
    >
      {value}
    </span>
  );
}

function Tabs({ heard, refused }: { heard: number; refused: number }) {
  const tabs: { name: string; icon: LucideIcon; count?: number; critical?: boolean }[] = [
    { name: 'Report', icon: Shield },
    { name: 'Network', icon: Globe, count: heard, critical: refused > 0 },
    { name: 'Files', icon: Folder, count: 3 },
    { name: 'Storage', icon: Folder },
    { name: 'Access', icon: Camera },
    { name: 'Output', icon: SquareTerminal, count: 2 },
    { name: 'In use', icon: Activity, count: 2 },
  ];
  return (
    <div
      className="flex shrink-0 items-center overflow-hidden px-2 pb-1 @max-[440px]:[mask-image:linear-gradient(90deg,#000_85%,transparent)]"
    >
      {tabs.map((tab) => {
        const shown = tab.name === 'Network';
        const Icon = tab.icon;
        return (
          <span
            key={tab.name}
            className="relative flex shrink-0 items-center gap-1.5 px-[7px] pt-1.5 pb-2 text-[11.5px]"
            style={{ color: shown ? fluent.text : fluent.secondary, fontWeight: shown ? 600 : 400 }}
          >
            <Icon aria-hidden className="size-3.5" strokeWidth={1.75} />
            {tab.name}
            {tab.count ? <Badge value={tab.count} critical={tab.critical} /> : null}
            {shown && (
              <span
                className="absolute bottom-0 left-1/2 h-[3px] w-4 -translate-x-1/2 rounded-full"
                style={{ background: fluent.accent }}
              />
            )}
          </span>
        );
      })}
    </div>
  );
}

function Toolbar({ heard, refused }: { heard: readonly NetworkEvent[]; refused: number }) {
  const requests = heard.filter((event) => event.kind === 'Request').length;
  const lookups = heard.filter((event) => event.kind === 'Lookup').length;
  const toggle = 'flex h-6 items-center gap-1.5 rounded-[4px] px-2 text-[11px]';
  const rest = { background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}, 0 1px 0 ${fluent.strokeStrong}` };
  return (
    <div className="flex shrink-0 items-center gap-1.5 px-4 py-2">
      <Search aria-hidden className="size-3.5 shrink-0" style={{ color: fluent.secondary }} />
      <span
        className="flex h-6 w-[150px] shrink-0 items-center rounded-[4px] px-2 text-[11px] @max-[440px]:hidden"
        style={{ background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}, 0 1px 0 rgba(0,0,0,0.4)`, color: fluent.tertiary }}
      >
        Filter by what it reached
      </span>
      <span className={toggle} style={{ background: fluent.accent, color: '#fff' }}>
        All <span className="tabular-nums">{heard.length}</span>
      </span>
      <span className={toggle} style={rest}>
        Requests <span className="tabular-nums">{requests}</span>
      </span>
      <span className={`${toggle} @max-[440px]:hidden`} style={rest}>
        Lookups <span className="tabular-nums">{lookups}</span>
      </span>
      <span className={toggle} style={rest}>
        Refused {refused > 0 ? <Badge value={refused} critical /> : <span className="tabular-nums">0</span>}
      </span>
    </div>
  );
}

function Table({ heard, selected }: { heard: readonly NetworkEvent[]; selected: boolean }) {
  // The columns as `NetworkRow::COLUMNS` has them; Status and Time step out beside the detail.
  const columns = selected
    ? 'grid-cols-[minmax(0,1fr)_58px_84px]'
    : 'grid-cols-[minmax(0,1fr)_34px_58px_48px_84px] @max-[440px]:grid-cols-[minmax(0,1fr)_58px]';
  const stepsOut = selected ? 'hidden' : '@max-[440px]:hidden';
  const head = 'truncate text-[11px] font-normal';
  return (
    <div className="min-w-0 flex-1 overflow-hidden">
      <div
        className={`grid ${columns} gap-x-2.5 px-4 py-1.5`}
        style={{ color: fluent.secondary, borderBottom: `1px solid ${fluent.stroke}` }}
      >
        <span className={head}>Name</span>
        <span className={`${head} text-right ${stepsOut}`}>Status</span>
        <span className={head}>Decided</span>
        <span className={`${head} text-right ${stepsOut}`}>Time</span>
        <span className={`${head} text-right @max-[440px]:hidden`}>Started</span>
      </div>
      {heard.map((event) => {
        const Glyph = kindGlyph[event.kind];
        const chosen = selected && event.refused;
        return (
          <div
            key={event.started}
            className="relative px-4"
            style={{
              borderBottom: `1px solid ${fluent.stroke}`,
              background: chosen ? 'rgba(0,0,0,0.05)' : undefined,
            }}
          >
            {chosen && (
              <span
                className="absolute top-[6px] left-1 h-3.5 w-[3px] rounded-full"
                style={{ background: fluent.accent }}
              />
            )}
            <div className={`grid ${columns} items-center gap-x-2.5 py-[5px] font-mono text-[10.5px]`}>
              <span className="flex min-w-0 items-center gap-2">
                <Glyph aria-hidden className="size-3 shrink-0" style={{ color: fluent.secondary }} strokeWidth={1.75} />
                <span className="truncate" style={{ color: event.refused ? fluent.critical : fluent.text }}>
                  {event.name}
                </span>
              </span>
              <span className={`text-right ${stepsOut}`}>{event.status}</span>
              <span className="font-sans text-[11px]" style={{ color: event.refused ? fluent.critical : fluent.success }}>
                {event.refused ? 'Refused' : 'Allowed'}
              </span>
              <span className={`text-right ${stepsOut}`} style={{ color: fluent.secondary }}>
                {event.took}
              </span>
              <span className="text-right @max-[440px]:hidden" style={{ color: fluent.secondary }}>
                {event.started}
              </span>
            </div>
            {event.refused && (
              <p className="hidden pb-1.5 pl-5 text-[10.5px] leading-snug @max-[440px]:block" style={{ color: fluent.secondary }}>
                {refusedBecause}
              </p>
            )}
          </div>
        );
      })}
    </div>
  );
}

function Detail() {
  const refusedEvent = events.find((event) => event.refused)!;
  return (
    <div
      className="hidden w-[224px] shrink-0 flex-col gap-2 overflow-hidden p-2.5 @min-[440px]:flex"
      style={{ borderLeft: `1px solid ${fluent.stroke}`, background: fluent.layer }}
    >
      <div className="flex items-start gap-2">
        <Globe aria-hidden className="mt-0.5 size-3.5 shrink-0" style={{ color: fluent.critical }} strokeWidth={1.75} />
        <div className="min-w-0 flex-1">
          <p className="text-[11.5px] font-semibold">Request refused</p>
          <p className="font-mono text-[10px] leading-snug break-all" style={{ color: fluent.secondary }}>
            {refusedEvent.name}
          </p>
        </div>
        <X aria-hidden className="size-3 shrink-0" style={{ color: fluent.secondary }} />
      </div>
      <div
        className="flex gap-2 rounded-[4px] p-2"
        style={{ background: fluent.criticalBackground, boxShadow: `0 0 0 1px ${fluent.stroke}` }}
      >
        <span
          className="mt-px flex size-3.5 shrink-0 items-center justify-center rounded-full"
          style={{ background: fluent.critical }}
        >
          <X aria-hidden className="size-2.5 text-white" strokeWidth={3} />
        </span>
        <p className="text-[10.5px] leading-snug">
          <span className="font-semibold">Refused by WaOS</span>
          <br />
          {refusedBecause}
        </p>
      </div>
      <div className="rounded-[4px]" style={{ background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}` }}>
        <div className="flex items-center justify-between px-2 py-1.5 text-[11px]">
          Event
          <ChevronUp aria-hidden className="size-3" style={{ color: fluent.secondary }} />
        </div>
        <dl
          className="grid grid-cols-[64px_minmax(0,1fr)] gap-y-1 px-2 pt-1.5 pb-2 text-[10.5px]"
          style={{ borderTop: `1px solid ${fluent.stroke}` }}
        >
          <dt style={{ color: fluent.secondary }}>Kind</dt>
          <dd>Request</dd>
          <dt style={{ color: fluent.secondary }}>Through</dt>
          <dd>WaOS&apos;s HTTP client</dd>
          <dt style={{ color: fluent.secondary }}>At</dt>
          <dd className="font-mono text-[10px]">{refusedEvent.started}</dd>
        </dl>
      </div>
    </div>
  );
}

function StatusBar({ heard, refused }: { heard: number; refused: number }) {
  return (
    <div
      className="flex h-7 shrink-0 items-center gap-3 px-4 text-[11px]"
      style={{ borderTop: `1px solid ${fluent.stroke}`, background: fluent.layer }}
    >
      <span className="size-[7px] shrink-0 rounded-full" style={{ background: fluent.success }} />
      <span className="truncate">Listening since 10:42:06</span>
      <span className="shrink-0" style={{ color: fluent.secondary }}>
        {heard} events
      </span>
      {refused > 0 && (
        <span className="shrink-0" style={{ color: fluent.secondary }}>
          {refused} refused
        </span>
      )}
      <span className="ml-auto flex shrink-0 items-center gap-1.5 @max-[440px]:hidden" style={{ color: fluent.text }}>
        <X aria-hidden className="size-3" />
        Clear
      </span>
    </div>
  );
}
