import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { WaosIcon } from '@/components/waos-icon';
import { appName, docsRoute, repositoryUrl } from './shared';

/** The header: the icon and name, Docs, docs.rs menu, GitHub and Fumadocs' search, in colors that follow the system's. */
export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="flex items-center gap-2 font-semibold">
          <WaosIcon size={24} />
          {appName}
        </span>
      ),
    },
    links: [
      { text: 'Docs', url: docsRoute, active: 'nested-url' },
      {
        type: 'menu',
        text: 'docs.rs',
        items: [
          {
            text: 'wasdk',
            description: 'App SDK — build WaOS programs in Rust',
            url: 'https://docs.rs/wasdk/latest/wasdk/',
            external: true,
          },
          {
            text: 'waos',
            description: 'Runtime engine — embed WaOS inside a host app',
            url: 'https://docs.rs/waos/latest/waos/',
            external: true,
          },
          {
            text: 'All API & WIT reference',
            description: 'WIT interfaces, wasdk-adapter-waft, and waos-capabilities',
            url: `${docsRoute}/reference`,
          },
        ],
      },
    ],
    githubUrl: repositoryUrl,
    themeSwitch: { enabled: false },
  };
}
