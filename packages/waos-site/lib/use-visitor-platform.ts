'use client';

import { useEffect, useState } from 'react';
import { detectClientPlatform, type VisitorPlatform } from '@/lib/platform';

interface NavigatorWithUAData extends Navigator {
  userAgentData?: { platform?: string };
}

/**
 * Detects the visitor's platform after hydration so statically prerendered pages stay identical
 * between server HTML and the first client render.
 */
export function useVisitorPlatform(): VisitorPlatform {
  const [platform, setPlatform] = useState<VisitorPlatform>('other');

  useEffect(() => {
    const nav = navigator as NavigatorWithUAData;
    setPlatform(
      detectClientPlatform(nav.userAgent, nav.userAgentData?.platform ?? nav.platform, nav.maxTouchPoints),
    );
  }, []);

  return platform;
}
