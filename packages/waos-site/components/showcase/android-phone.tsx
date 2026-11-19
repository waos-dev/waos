import type { CSSProperties, ReactNode } from 'react';

/**
 * An Android phone: a graphite frame with its side keys, the punch-hole camera over the status
 * bar, and the gesture bar beneath the app it shows. Its size is its width; it keeps a phone's
 * proportions.
 */
export function AndroidPhone({
  width,
  time,
  className = '',
  style,
  children,
}: {
  width: number;
  time: string;
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}) {
  const height = Math.round(width * 2.08);
  return (
    <div className={`relative shrink-0 ${className}`} style={{ width, height, ...style }}>
      <span
        className="absolute top-[22%] -right-[2px] h-[11%] w-[3px] rounded-r-[2px]"
        style={{ background: 'linear-gradient(90deg, #2a2c30, #55585e)' }}
      />
      <span
        className="absolute top-[37%] -right-[2px] h-[7%] w-[3px] rounded-r-[2px]"
        style={{ background: 'linear-gradient(90deg, #2a2c30, #55585e)' }}
      />
      <div
        className="absolute inset-0 rounded-[34px] p-[3px]"
        style={{
          background: 'linear-gradient(145deg, #5b5e64 0%, #2b2d31 28%, #1b1c1f 62%, #44474c 100%)',
          boxShadow:
            '0 30px 60px -22px rgba(15,23,42,0.5), 0 12px 24px -12px rgba(15,23,42,0.35), inset 0 0 0 1px rgba(255,255,255,0.08)',
        }}
      >
        <div className="size-full rounded-[31px] bg-black p-[6px]">
          <div className="relative flex size-full flex-col overflow-hidden rounded-[25px] bg-[#f3f3f3]">
            <StatusBar time={time} />
            <div className="relative min-h-0 flex-1">{children}</div>
            <div className="flex h-[18px] shrink-0 items-center justify-center">
              <span className="h-[4px] w-[34%] rounded-full bg-black/80" />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function StatusBar({ time }: { time: string }) {
  return (
    <div className="relative flex h-[26px] shrink-0 items-center justify-between px-4 text-[10.5px] font-medium text-black/85">
      <span className="tabular-nums">{time}</span>
      <span className="absolute top-[7px] left-1/2 size-[11px] -translate-x-1/2 rounded-full bg-[#0b0b0c] ring-1 ring-black/40" />
      <span className="flex items-center gap-[5px]">
        <WifiGlyph />
        <SignalGlyph />
        <BatteryGlyph />
      </span>
    </div>
  );
}

function WifiGlyph() {
  return (
    <svg viewBox="0 0 16 12" width="12" height="9" aria-hidden>
      <path d="M8 11.5 0.6 3.6a10.6 10.6 0 0 1 14.8 0Z" fill="currentColor" />
    </svg>
  );
}

function SignalGlyph() {
  return (
    <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden>
      <path d="M11.5 0.5v11h-11Z" fill="currentColor" />
    </svg>
  );
}

function BatteryGlyph() {
  return (
    <svg viewBox="0 0 8 14" width="6" height="11" aria-hidden>
      <rect x="2.5" y="0" width="3" height="1.6" rx="0.5" fill="currentColor" />
      <rect x="0.6" y="1.4" width="6.8" height="12" rx="1.4" fill="none" stroke="currentColor" strokeWidth="1.1" />
      <rect x="1.8" y="4" width="4.4" height="8.3" rx="0.6" fill="currentColor" />
    </svg>
  );
}
