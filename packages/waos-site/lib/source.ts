import { loader } from 'fumadocs-core/source';
import type * as PageTree from 'fumadocs-core/page-tree';
import { defineDocs } from 'fumadocs-mdx/macro';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';
import { docsRoute, siteOrigin } from './shared';

const docs = defineDocs({
  // The docs live in the crate that ships them, `cargo waos`, which prints the same files.
  dir: '../../crates/waos-cli/docs',
  docs: {
    schema: pageSchema,
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
});

export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource(),
});

type Page = (typeof source)['$inferPage'];

export function getPageMarkdownUrl(page: Page) {
  return { url: `${page.url}.md` };
}

export async function getLLMText(page: Page) {
  const processed = await page.data.getText('processed');

  return `# ${page.data.title} (${siteOrigin()}${page.url})

${processed}`;
}

/** The guide's pages, one per thing a program has, in the guide's order. */
export function guideTopics(): PageTree.Item[] {
  const guide = source.getPageTree().children.find(isGuideFolder);
  return guide?.children.filter((node): node is PageTree.Item => node.type === 'page') ?? [];
}

function isGuideFolder(node: PageTree.Node): node is PageTree.Folder {
  return node.type === 'folder' && node.index?.url === `${docsRoute}/guide`;
}
