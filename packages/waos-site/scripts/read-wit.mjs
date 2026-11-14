/**
 * Reads a WIT package's interfaces from its files, for the reference: each interface's doc
 * comment, its `use`s, and its items in the order written, each with its doc comment and its WIT
 * as written, a resource's functions beneath it.
 *
 * It reads only the structure the reference shows, and stops on anything else it meets, so a
 * construct it does not know breaks the site's build instead of going missing from its pages.
 * Whether the WIT is valid is for `wasm-tools component wit` to check.
 */
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

/** The kinds of type whose body is fields, cases or flags, which stay in their WIT. */
const TYPES_WITH_BODY = new Set(['record', 'variant', 'enum', 'flags']);
const PUNCTUATION = '{}()<>;:,=./@*';
const WORD = /[%A-Za-z0-9_+-]+/y;

/** The package in `dir`, from its `.wit` files: its id, name, version, doc comment and interfaces, by name. */
export function readPackage(dir) {
  const files = readdirSync(dir)
    .filter((file) => file.endsWith('.wit'))
    .sort()
    .map((file) => readFile(join(dir, file)));
  const ids = [...new Set(files.map((file) => file.packageId))];
  if (ids.length !== 1) throw new Error(`${dir}: its files name ${ids.length} packages (${ids.join(', ')})`);
  const [id] = ids;
  const [name, version] = id.split('@');
  return {
    id,
    name,
    version,
    docs: files.map((file) => file.packageDocs).filter(Boolean).join('\n\n'),
    interfaces: files.flatMap((file) => file.interfaces).sort(byName),
  };
}

function readFile(path) {
  const cursor = new Cursor(path, readFileSync(path, 'utf8'));
  const packageDocs = leading(cursor).docs;
  cursor.expect('package');
  const packageId = readPackageId(cursor);
  const interfaces = [];
  for (;;) {
    const before = leading(cursor);
    if (cursor.done()) return { packageId, packageDocs, interfaces };
    const keyword = cursor.word();
    if (keyword.text === 'interface') interfaces.push(readInterface(cursor, before.docs));
    else if (keyword.text === 'world') skipWorld(cursor);
    else if (keyword.text === 'use') statementEnd(cursor);
    else throw cursor.error(keyword, 'expected an interface, a world or a `use`');
  }
}

/** The id after `package`, up to its `;`; a package written with a body is not read. */
function readPackageId(cursor) {
  const start = cursor.peek()?.start;
  for (;;) {
    const token = cursor.next();
    if (token.text === '{') throw cursor.error(token, 'a package with a body is not read');
    if (token.text === ';') return cursor.source.slice(start, token.start).trim();
  }
}

function skipWorld(cursor) {
  cursor.word();
  blockEnd(cursor);
}

function readInterface(cursor, docs) {
  const name = cursor.word().text;
  cursor.expect('{');
  const uses = [];
  const items = [];
  for (;;) {
    const before = leading(cursor);
    if (cursor.peekIs('}')) break;
    const item = readItem(cursor, before);
    if (item.kind === 'use') uses.push(item.wit);
    else items.push(item);
  }
  cursor.expect('}');
  return { name: plainName(name), docs, uses, items };
}

function readItem(cursor, { docs, start }) {
  const first = cursor.word();
  const from = start ?? first.start;
  const item = (kind, name, end) => ({
    kind,
    name: plainName(name),
    docs,
    wit: witText(cursor.source, from, end),
  });
  if (first.text === 'use') return item('use', '', statementEnd(cursor));
  if (first.text === 'type') return item('type', cursor.word().text, statementEnd(cursor));
  if (first.text === 'resource') return readResource(cursor, docs, from);
  if (TYPES_WITH_BODY.has(first.text)) return item(first.text, cursor.word().text, blockEnd(cursor));
  if (cursor.peekIs(':')) return item('func', first.text, statementEnd(cursor));
  throw cursor.error(first, 'expected an item of an interface');
}

/**
 * A resource, with its functions as items of their own; its WIT is the resource without its
 * functions' doc comments, which go with each function.
 */
function readResource(cursor, docs, from) {
  const name = plainName(cursor.word().text);
  if (cursor.peekIs(';')) {
    return { kind: 'resource', name, docs, wit: witText(cursor.source, from, cursor.next().end), members: [] };
  }
  const open = cursor.expect('{');
  const members = [];
  for (;;) {
    const before = leading(cursor);
    if (cursor.peekIs('}')) break;
    members.push(readMember(cursor, before));
  }
  const close = cursor.expect('}');
  const wit =
    members.length === 0
      ? witText(cursor.source, from, close.end)
      : [witText(cursor.source, from, open.end), ...members.map((member) => indented(member.wit)), '}'].join('\n');
  return { kind: 'resource', name, docs, wit, members };
}

/** One of a resource's functions: its constructor, a method or a static function. */
function readMember(cursor, { docs, start }) {
  const first = cursor.word();
  const isConstructor = first.text === 'constructor' && cursor.peekIs('(');
  if (!isConstructor && !cursor.peekIs(':')) throw cursor.error(first, 'expected a function of a resource');
  const end = statementEnd(cursor);
  return { name: plainName(first.text), docs, wit: witText(cursor.source, start ?? first.start, end) };
}

/** The doc comment before an item, and where its gates (`@since(…)` and the like) start, if it has any. */
function leading(cursor) {
  const docs = [];
  let start = null;
  for (;;) {
    const token = cursor.peek();
    if (token?.type === 'doc') {
      docs.push(cursor.next().text);
    } else if (token?.text === '@') {
      start ??= token.start;
      skipGate(cursor);
    } else {
      return { docs: docs.join('\n').trim(), start };
    }
  }
}

function skipGate(cursor) {
  cursor.expect('@');
  cursor.word();
  if (!cursor.peekIs('(')) return;
  let depth = 0;
  for (;;) {
    const token = cursor.next();
    if (token.text === '(') depth += 1;
    else if (token.text === ')' && --depth === 0) return;
  }
}

/** Where the statement at the cursor ends: past its `;`, braces such as a `use`'s skipped. */
function statementEnd(cursor) {
  let depth = 0;
  for (;;) {
    const token = cursor.next();
    if (token.type === 'doc') continue;
    if (token.text === '{') depth += 1;
    else if (token.text === '}' && --depth < 0) throw cursor.error(token, 'expected `;`');
    else if (token.text === ';' && depth === 0) return token.end;
  }
}

/** Where the body at the cursor ends: past the `}` that closes its `{`. */
function blockEnd(cursor) {
  cursor.expect('{');
  let depth = 1;
  for (;;) {
    const token = cursor.next();
    if (token.type === 'doc') continue;
    if (token.text === '{') depth += 1;
    else if (token.text === '}' && --depth === 0) return token.end;
  }
}

/** The source from `start` to `end`, its later lines without the indentation of the line it starts on. */
function witText(source, start, end) {
  const lineStart = source.lastIndexOf('\n', start - 1) + 1;
  const indentation = /^[ \t]*/.exec(source.slice(lineStart, start))[0];
  return source
    .slice(start, end)
    .split('\n')
    .map((line, index) => (index > 0 && line.startsWith(indentation) ? line.slice(indentation.length) : line))
    .join('\n');
}

function indented(text) {
  return text
    .split('\n')
    .map((line) => (line === '' ? line : `  ${line}`))
    .join('\n');
}

/** A name without the `%` that lets a WIT name be a keyword. */
function plainName(name) {
  return name.replace(/^%/, '');
}

function byName(a, b) {
  return a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
}

/** A walk through one file's tokens. */
class Cursor {
  constructor(file, source) {
    this.file = file;
    this.source = source;
    this.tokens = tokenize(file, source);
    this.at = 0;
  }

  done() {
    return this.at >= this.tokens.length;
  }

  peek() {
    return this.tokens[this.at];
  }

  peekIs(text) {
    const token = this.peek();
    return token !== undefined && token.type !== 'doc' && token.text === text;
  }

  next() {
    const token = this.tokens[this.at];
    if (token === undefined) throw this.error(undefined, 'the file ends early');
    this.at += 1;
    return token;
  }

  expect(text) {
    const token = this.next();
    if (token.type === 'doc' || token.text !== text) throw this.error(token, `expected \`${text}\``);
    return token;
  }

  word() {
    const token = this.next();
    if (token.type !== 'word') throw this.error(token, 'expected a name');
    return token;
  }

  error(token, message) {
    const offset = token?.start ?? this.source.length;
    const line = this.source.slice(0, offset).split('\n').length;
    return new Error(`${this.file}:${line}: ${message}, at ${token ? `\`${token.text}\`` : 'its end'}`);
  }
}

/** The file's words, punctuation and doc comments, each with where it is; other comments are dropped. */
function tokenize(file, source) {
  const tokens = [];
  let at = 0;
  while (at < source.length) {
    const token = tokenAt(file, source, at);
    if (token.type !== 'space') tokens.push(token);
    at = token.end;
  }
  return tokens;
}

function tokenAt(file, source, start) {
  const token = (type, end, text = source.slice(start, end)) => ({ type, text, start, end });
  if (/\s/.test(source[start])) return token('space', start + 1);
  if (source.startsWith('///', start)) {
    const end = lineEnd(source, start);
    return token('doc', end, source.slice(start + 3, end).replace(/^ /, ''));
  }
  if (source.startsWith('//', start)) return token('space', lineEnd(source, start));
  if (source.startsWith('/*', start)) {
    const end = blockCommentEnd(file, source, start);
    const isDoc = source.startsWith('/**', start) && end - start > 4;
    return isDoc ? token('doc', end, blockDocText(source.slice(start + 3, end - 2))) : token('space', end);
  }
  if (source.startsWith('->', start)) return token('punctuation', start + 2);
  if (PUNCTUATION.includes(source[start])) return token('punctuation', start + 1);
  WORD.lastIndex = start;
  if (!WORD.test(source)) {
    const line = source.slice(0, start).split('\n').length;
    throw new Error(`${file}:${line}: unexpected \`${source[start]}\``);
  }
  return token('word', WORD.lastIndex);
}

function lineEnd(source, start) {
  const end = source.indexOf('\n', start);
  return end === -1 ? source.length : end;
}

/** Where the block comment at `start` ends, past the comments nested in it. */
function blockCommentEnd(file, source, start) {
  let depth = 0;
  for (let at = start; at < source.length; ) {
    if (source.startsWith('/*', at)) {
      depth += 1;
      at += 2;
    } else if (source.startsWith('*/', at)) {
      depth -= 1;
      at += 2;
      if (depth === 0) return at;
    } else {
      at += 1;
    }
  }
  throw new Error(`${file}: a block comment that never ends`);
}

/** A `/** … *\/` comment's text, without the `*` its lines may start with. */
function blockDocText(text) {
  return text
    .split('\n')
    .map((line) => line.replace(/^\s*\* ?/, ''))
    .join('\n')
    .trim();
}
