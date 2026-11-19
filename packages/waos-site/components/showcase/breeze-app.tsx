import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { ChevronDown, Cloud, CloudRain, CloudSun, MapPin, Search, Sun } from 'lucide-react';
import { showcase } from './app';
import { fluent } from './fluent';

/**
 * Breeze, the story's app, drawn as Waft's WinUI kit lays it out: its cities beside the forecast
 * in a desktop's window, one column of the same parts on a phone. `AppScreen` shows a capture of
 * the running app instead where `showcase.screenshots` has one.
 */
export function AppScreen({ layout }: { layout: 'desktop' | 'phone' }) {
  const capture = showcase.screenshots[layout];
  if (capture) {
    return <img src={capture} alt="" className="size-full object-cover object-top" />;
  }
  return layout === 'desktop' ? <BreezeDesktop /> : <BreezePhone />;
}

type Sky = 'sun' | 'partly' | 'cloud' | 'rain';

const skyIcon: Record<Sky, { icon: LucideIcon; color: string }> = {
  sun: { icon: Sun, color: '#f5a524' },
  partly: { icon: CloudSun, color: '#e8a33d' },
  cloud: { icon: Cloud, color: '#8a94a6' },
  rain: { icon: CloudRain, color: '#3b82f6' },
};

const cities = [
  { name: 'Lisbon', said: 'Mostly sunny', now: 21 },
  { name: 'Oslo', said: 'Cloudy', now: 8 },
  { name: 'Tokyo', said: 'Light rain', now: 17 },
  { name: 'New York', said: 'Partly cloudy', now: 12 },
] as const;

const hours: readonly { at: string; sky: Sky; degrees: number }[] = [
  { at: 'Now', sky: 'sun', degrees: 21 },
  { at: '15', sky: 'sun', degrees: 22 },
  { at: '16', sky: 'partly', degrees: 22 },
  { at: '17', sky: 'partly', degrees: 21 },
  { at: '18', sky: 'cloud', degrees: 19 },
  { at: '19', sky: 'cloud', degrees: 17 },
];

const days: readonly { day: string; sky: Sky; low: number; high: number }[] = [
  { day: 'Today', sky: 'sun', low: 15, high: 24 },
  { day: 'Fri', sky: 'partly', low: 14, high: 22 },
  { day: 'Sat', sky: 'rain', low: 13, high: 19 },
  { day: 'Sun', sky: 'partly', low: 14, high: 21 },
  { day: 'Mon', sky: 'sun', low: 16, high: 25 },
];

/** The coolest and warmest of the week, the ends of every day's bar. */
const coolest = 12;
const warmest = 26;

function SkyGlyph({ sky, size }: { sky: Sky; size: number }) {
  const { icon: Icon, color } = skyIcon[sky];
  return <Icon aria-hidden style={{ color, width: size, height: size }} strokeWidth={2} />;
}

function Card({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={`rounded-[7px] ${className}`}
      style={{ background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}` }}
    >
      {children}
    </div>
  );
}

function Hourly({ count }: { count: number }) {
  return (
    <Card className="flex justify-between px-2 py-2">
      {hours.slice(0, count).map((hour) => (
        <div key={hour.at} className="flex min-w-0 flex-1 flex-col items-center gap-1">
          <span className="text-[10px]" style={{ color: fluent.secondary }}>
            {hour.at}
          </span>
          <SkyGlyph sky={hour.sky} size={15} />
          <span className="text-[11px] font-semibold tabular-nums" style={{ color: fluent.text }}>
            {hour.degrees}°
          </span>
        </div>
      ))}
    </Card>
  );
}

function Daily({ count, rowClass, narrow = false }: { count: number; rowClass: string; narrow?: boolean }) {
  return (
    <Card className="px-2.5 py-1">
      {days.slice(0, count).map((day, index) => {
        const left = ((day.low - coolest) / (warmest - coolest)) * 100;
        const width = ((day.high - day.low) / (warmest - coolest)) * 100;
        return (
          <div
            key={day.day}
            className={`flex items-center ${narrow ? 'gap-1.5' : 'gap-2'} ${rowClass}`}
            style={{ borderTop: index === 0 ? undefined : `1px solid ${fluent.stroke}` }}
          >
            <span className={`${narrow ? 'w-8' : 'w-9'} text-[11px]`} style={{ color: fluent.text }}>
              {day.day}
            </span>
            <SkyGlyph sky={day.sky} size={14} />
            <span className="w-[22px] text-right text-[11px] tabular-nums" style={{ color: fluent.secondary }}>
              {day.low}°
            </span>
            <span className="relative h-[4px] flex-1 rounded-full" style={{ background: 'rgba(0,0,0,0.07)' }}>
              <span
                className="absolute inset-y-0 rounded-full"
                style={{
                  left: `${left}%`,
                  width: `${width}%`,
                  background: 'linear-gradient(90deg, #38bdf8, #fbbf24)',
                }}
              />
            </span>
            <span className="w-[22px] text-[11px] font-semibold tabular-nums" style={{ color: fluent.text }}>
              {day.high}°
            </span>
          </div>
        );
      })}
    </Card>
  );
}

function Now({ big, stacked = false }: { big: string; stacked?: boolean }) {
  return (
    <div className="flex items-end justify-between gap-3">
      <div>
        <p className={`leading-none font-light tracking-tight ${big}`} style={{ color: fluent.text }}>
          21°
        </p>
        <p className="mt-1.5 text-[11.5px]" style={{ color: fluent.secondary }}>
          Mostly sunny{stacked ? <br /> : ', '}H 24° L 15°
        </p>
      </div>
      <SkyGlyph sky="sun" size={34} />
    </div>
  );
}

/** Breeze in a desktop's window: the cities in a side pane beside Lisbon's forecast. */
function BreezeDesktop() {
  return (
    <div className="@container flex size-full" style={{ background: fluent.base }}>
      <aside className="hidden w-[132px] shrink-0 flex-col gap-1 px-2 pt-2.5 @min-[360px]:flex">
        <div
          className="mb-1.5 flex items-center gap-1.5 rounded-[5px] px-2 py-[5px] text-[10.5px]"
          style={{ background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}`, color: fluent.tertiary }}
        >
          <Search aria-hidden className="size-3" />
          Search cities
        </div>
        {cities.map((city, index) => {
          const chosen = index === 0;
          return (
            <div
              key={city.name}
              className="relative flex items-center justify-between rounded-[5px] py-[5px] pr-2 pl-2.5"
              style={{ background: chosen ? 'rgba(0,0,0,0.055)' : undefined }}
            >
              {chosen && (
                <span
                  className="absolute top-1/2 left-0 h-3.5 w-[3px] -translate-y-1/2 rounded-full"
                  style={{ background: fluent.accent }}
                />
              )}
              <span className="min-w-0">
                <span className="block truncate text-[11.5px]" style={{ color: fluent.text }}>
                  {city.name}
                </span>
                <span className="block truncate text-[10px]" style={{ color: fluent.secondary }}>
                  {city.said}
                </span>
              </span>
              <span className="text-[12px] tabular-nums" style={{ color: fluent.text }}>
                {city.now}°
              </span>
            </div>
          );
        })}
      </aside>
      <main
        className="flex min-w-0 flex-1 flex-col gap-3 p-4 @min-[360px]:mt-1.5 @min-[360px]:rounded-tl-[8px]"
        style={{ background: fluent.layer, boxShadow: `-1px -1px 0 ${fluent.stroke}` }}
      >
        <div>
          <p className="text-[17px] font-semibold" style={{ color: fluent.text }}>
            Lisbon
          </p>
          <p className="text-[10.5px]" style={{ color: fluent.secondary }}>
            Thursday 14:00
          </p>
        </div>
        <Now big="text-[44px]" />
        <Hourly count={6} />
        <Daily count={4} rowClass="py-[5px]" />
      </main>
    </div>
  );
}

/** Breeze on a phone: the same parts in one column, the cities behind the name at the top. */
function BreezePhone() {
  return (
    <div className="flex size-full flex-col gap-2.5 px-3 pt-1" style={{ background: fluent.base }}>
      <div className="flex items-center justify-between py-1">
        <span className="flex items-center gap-1 text-[14px] font-semibold" style={{ color: fluent.text }}>
          <MapPin aria-hidden className="size-3.5" style={{ color: fluent.accent }} />
          Lisbon
          <ChevronDown aria-hidden className="size-3.5" style={{ color: fluent.secondary }} />
        </span>
        <Search aria-hidden className="size-4" style={{ color: fluent.secondary }} />
      </div>
      <div className="pt-2 pb-1">
        <Now big="text-[46px]" stacked />
      </div>
      <Hourly count={5} />
      <Daily count={5} rowClass="py-[6px]" narrow />
    </div>
  );
}
