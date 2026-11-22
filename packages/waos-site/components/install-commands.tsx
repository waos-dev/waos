'use client';

import { useState, type ReactNode } from 'react';
import { useVisitorPlatform } from '@/lib/use-visitor-platform';

type ShellTab = 'unix' | 'windows';

/**
 * Switches between the pre-rendered sh (macOS / Linux) and PowerShell (Windows) quickstart
 * snippets. Defaults to the visitor's platform after hydration unless they picked a tab.
 */
export function InstallCommands({ unix, windows }: { unix: ReactNode; windows: ReactNode }) {
  const visitor = useVisitorPlatform();
  const [chosen, setChosen] = useState<ShellTab | null>(null);
  const active: ShellTab = chosen ?? (visitor === 'windows' ? 'windows' : 'unix');

  return (
    <div className="flex flex-col gap-2">
      <div role="tablist" aria-label="Install commands by system" className="flex gap-1.5">
        <TabButton active={active === 'unix'} onClick={() => setChosen('unix')}>
          macOS / Linux
        </TabButton>
        <TabButton active={active === 'windows'} onClick={() => setChosen('windows')}>
          Windows
        </TabButton>
      </div>
      <div role="tabpanel">{active === 'windows' ? windows : unix}</div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
        active
          ? 'border-fd-foreground bg-fd-foreground text-fd-background'
          : 'border-fd-border bg-fd-card text-fd-muted-foreground hover:text-fd-foreground'
      }`}
    >
      {children}
    </button>
  );
}
