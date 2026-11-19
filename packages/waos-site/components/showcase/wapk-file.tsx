import { useId } from 'react';
import { showcase } from './app';

/** Breeze's icon: a sun behind a cloud on a sky, in the rounded square every WaOS app has. */
export function BreezeIcon({ size, className = '' }: { size: number; className?: string }) {
  const sky = useId();
  return (
    <svg viewBox="0 0 64 64" width={size} height={size} className={className} aria-hidden>
      <defs>
        <linearGradient id={sky} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#5cb8ff" />
          <stop offset="1" stopColor="#2563eb" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="14.5" fill={`url(#${sky})`} />
      <circle cx="41" cy="23" r="10" fill="#ffd34e" />
      <path d="M18 47h26a9 9 0 0 0 0-18 12 12 0 0 0-23 3 7.5 7.5 0 0 0-3 15z" fill="#fff" />
    </svg>
  );
}

/**
 * The package as Finder shows a file: its page with the app's icon on it, its name and its size
 * beneath.
 */
export function WapkFile({ className = '' }: { className?: string }) {
  return (
    <div className={`flex w-[180px] flex-col items-center text-center ${className}`}>
      <div
        className="relative h-[104px] w-[84px] rounded-[7px] bg-white"
        style={{
          clipPath: 'polygon(0 0, calc(100% - 20px) 0, 100% 20px, 100% 100%, 0 100%)',
          boxShadow: 'inset 0 0 0 1px rgba(0,0,0,0.1)',
          filter: 'drop-shadow(0 10px 18px rgba(15,23,42,0.18)) drop-shadow(0 1px 2px rgba(15,23,42,0.12))',
        }}
      >
        <span
          className="absolute top-0 right-0 size-5 rounded-bl-[5px]"
          style={{ background: 'linear-gradient(225deg, transparent 50%, #e9ebef 50%)' }}
        />
        <BreezeIcon size={46} className="absolute top-[30px] left-[19px]" />
        <span className="absolute inset-x-0 bottom-[9px] text-[9px] font-semibold tracking-wide text-black/40">
          WAPK
        </span>
      </div>
      <span className="mt-3 text-[13px] font-medium break-all">{showcase.file}</span>
      <span className="mt-0.5 text-[12px] text-fd-muted-foreground">{showcase.size}</span>
    </div>
  );
}
