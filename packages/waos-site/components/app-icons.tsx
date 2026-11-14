import type { ReactNode } from 'react';

/** The icons page's apps, side by side. */
export function AppIcons({ children }: { children: ReactNode }) {
  return <ul className="not-prose my-6 grid grid-cols-2 gap-6 sm:grid-cols-4">{children}</ul>;
}

/** An app's icon, as the site serves it from `public/apps`, above its name. */
export function AppIcon({ name, src }: { name: string; src: string }) {
  return (
    <li className="flex flex-col items-center gap-2 text-sm">
      <img src={src} alt="" width={512} height={512} loading="lazy" className="w-full rounded-2xl" />
      {name}
    </li>
  );
}
