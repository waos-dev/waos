import type { CSSProperties, ReactNode } from 'react';

/**
 * A window on macOS, as WaOS opens a program's: the traffic lights and the title over the
 * program's own drawing, light or dark.
 */
export function MacWindow({
  title,
  tone = 'light',
  className = '',
  style,
  children,
}: {
  title: string;
  tone?: 'light' | 'dark';
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}) {
  const dark = tone === 'dark';
  return (
    <div
      className={`flex flex-col overflow-hidden rounded-[10px] ${className}`}
      style={{
        background: dark ? '#141518' : '#f3f3f3',
        boxShadow: dark
          ? '0 0 0 1px rgba(255,255,255,0.08), 0 24px 60px -18px rgba(0,0,0,0.55), 0 8px 20px -10px rgba(0,0,0,0.35)'
          : '0 0 0 0.5px rgba(0,0,0,0.16), 0 24px 56px -20px rgba(15,23,42,0.32), 0 8px 18px -10px rgba(15,23,42,0.16)',
        ...style,
      }}
    >
      <div
        className="relative flex h-7 shrink-0 items-center px-2.5"
        style={{
          background: dark ? '#1c1d21' : '#ececec',
          borderBottom: dark ? '1px solid rgba(255,255,255,0.06)' : '1px solid rgba(0,0,0,0.07)',
        }}
      >
        <div className="flex gap-[7px]">
          <span className="size-[11px] rounded-full" style={{ background: '#ff5f57', boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.18)' }} />
          <span className="size-[11px] rounded-full" style={{ background: '#febc2e', boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.18)' }} />
          <span className="size-[11px] rounded-full" style={{ background: '#28c840', boxShadow: 'inset 0 0 0 0.5px rgba(0,0,0,0.18)' }} />
        </div>
        <span
          className="absolute inset-x-20 truncate text-center text-[12px] font-semibold"
          style={{ color: dark ? 'rgba(255,255,255,0.6)' : 'rgba(0,0,0,0.62)' }}
        >
          {title}
        </span>
      </div>
      <div className="relative min-h-0 flex-1">{children}</div>
    </div>
  );
}
