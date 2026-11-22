import { downloadPath, type Platform } from '@/lib/releases';

/** The visitor's operating system, or `'other'` when no WaOS build targets it (such as iOS). */
export type VisitorPlatform = Platform | 'other';

/** Everything the site shows for one supported platform. */
export interface PlatformInfo {
  id: Platform;
  /** Short name shown in buttons and links, e.g. "macOS", "Windows". */
  name: string;
  /** Architecture or package hint, e.g. "Apple Silicon", "x64". */
  detail: string;
  /** Site redirect to the newest release's file for this platform. */
  href: string;
  /** One-line CLI installer where available. */
  installCommand?: {
    shell: 'sh' | 'powershell';
    command: string;
  };
}

const shInstall = {
  shell: 'sh',
  command: 'curl -fsSL https://waos.app/install.sh | sh',
} as const;

const psInstall = {
  shell: 'powershell',
  command: 'irm https://waos.app/install.ps1 | iex',
} as const;

const byId: Record<Platform, PlatformInfo> = {
  macos: {
    id: 'macos',
    name: 'macOS',
    detail: 'Apple Silicon',
    href: downloadPath('macos'),
    installCommand: shInstall,
  },
  windows: {
    id: 'windows',
    name: 'Windows',
    detail: 'x64',
    href: downloadPath('windows'),
    installCommand: psInstall,
  },
  linux: {
    id: 'linux',
    name: 'Linux',
    detail: 'x86_64',
    href: downloadPath('linux'),
    installCommand: shInstall,
  },
  android: {
    id: 'android',
    name: 'Android',
    detail: 'ARM64',
    href: downloadPath('android'),
  },
};

export const platformList: readonly PlatformInfo[] = [
  byId.macos,
  byId.windows,
  byId.linux,
  byId.android,
];

export function platformInfo(platform: Platform): PlatformInfo {
  return byId[platform];
}

/**
 * Pure detection from browser navigator values. Checks `userAgent` first (so DevTools and test
 * emulation match too) and falls back to `userAgentData.platform` / `navigator.platform`.
 * Checks Android before Linux (Android UAs contain "Linux") and rejects iOS/iPadOS (including
 * desktop-mode iPad Safari, which reports Mac with touch points) so unsupported systems return `'other'`.
 */
export function detectClientPlatform(
  userAgent: string,
  platform = '',
  maxTouchPoints = 0,
): VisitorPlatform {
  const ua = userAgent.toLowerCase();
  if (ua.includes('android')) return 'android';
  if (ua.includes('iphone') || ua.includes('ipad') || ua.includes('ipod')) return 'other';
  if (ua.includes('cros')) return 'other';
  if (ua.includes('windows') || ua.includes('win64') || ua.includes('win32')) return 'windows';
  if (ua.includes('macintosh') || ua.includes('mac os x')) {
    return maxTouchPoints > 1 ? 'other' : 'macos';
  }
  if (ua.includes('linux') || ua.includes('x11')) return 'linux';

  const hint = platform.toLowerCase();
  if (hint === 'android') return 'android';
  if (hint === 'windows' || hint.startsWith('win')) return 'windows';
  if (hint === 'macos' || hint.startsWith('mac')) return maxTouchPoints > 1 ? 'other' : 'macos';
  if (hint === 'linux') return 'linux';
  return 'other';
}
