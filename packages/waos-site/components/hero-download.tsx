'use client';

import { Fragment } from 'react';
import { Check, Download } from 'lucide-react';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { BuildYourOwn } from '@/components/build-your-own';
import { platformInfo, platformList, type PlatformInfo } from '@/lib/platform';
import { useVisitorPlatform } from '@/lib/use-visitor-platform';

/**
 * The Hero's download CTA, agent dialog trigger, and platform links. Renders a neutral
 * "Download WaOS" button in static HTML so `/` stays CDN-cached, then specializes to the
 * visitor's platform on hydration while keeping all four platforms one click away below.
 */
export function HeroDownload({ prompt, startUrl }: { prompt: string; startUrl: string }) {
  const visitor = useVisitorPlatform();
  const current = visitor === 'other' ? undefined : platformInfo(visitor);
  const others = current ? platformList.filter((item) => item.id !== current.id) : platformList;
  const linuxCommand = platformInfo('linux').installCommand!.command;
  const [copiedInstall, copyInstall] = useCopyButton(() => navigator.clipboard.writeText(linuxCommand));

  return (
    <>
      <div className="relative mt-9 flex flex-wrap justify-center gap-3">
        <a
          href={current ? current.href : '#downloads'}
          className={buttonVariants({ color: 'primary', className: 'gap-2 px-6 py-3 text-base' })}
        >
          <Download aria-hidden className="size-5" strokeWidth={2} />
          {current ? `Download for ${current.name}` : 'Download WaOS'}
        </a>
        <BuildYourOwn prompt={prompt} startUrl={startUrl} />
      </div>
      <p id="downloads" className="relative mt-6 text-sm text-fd-muted-foreground">
        {current ? (
          <>
            {current.id === 'macos' ? current.detail : `${current.name} (${current.detail})`}
            {current.id === 'linux' && (
              <>
                , or copy{' '}
                <button
                  type="button"
                  onClick={copyInstall}
                  className="inline-flex items-center gap-1 font-medium text-fd-foreground underline underline-offset-4"
                >
                  <code>install.sh</code>
                  {copiedInstall && <Check aria-hidden className="size-3.5" />}
                </button>
              </>
            )}
            . Also for <PlatformLinks items={others} />.
          </>
        ) : (
          <>
            Available for <PlatformLinks items={others} />.
          </>
        )}
      </p>
    </>
  );
}

function PlatformLinks({ items }: { items: readonly PlatformInfo[] }) {
  return (
    <>
      {items.map((item, index) => {
        const separator = index === 0 ? '' : index === items.length - 1 ? ' and ' : ', ';
        const label = item.id === 'macos' ? `${item.name} (${item.detail})` : item.name;
        return (
          <Fragment key={item.id}>
            {separator}
            <a href={item.href} className="font-medium text-fd-foreground underline underline-offset-4">
              {label}
            </a>
          </Fragment>
        );
      })}
    </>
  );
}
