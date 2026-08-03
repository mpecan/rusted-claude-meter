/** @vitest-environment jsdom */
// Every element handle the frontend holds is resolved by *name* — an id
// through `requireElement`, a `data-role` through `requireChild` — against
// markup that lives in a separate file the type checker never reads. Rename
// one side and nothing complains until the view throws at construction, in
// front of a user.
//
// So this derives the names from the TypeScript and checks them against the
// shipped `index.html`. Derived, never listed: a hand-maintained list of ids
// is one more thing to forget to update, and would go stale in exactly the
// situation it exists to catch. The three negative-control tests below are
// what make a green run mean something — they feed the same checker a
// synthetic module with a name `index.html` does not have and prove it says
// so.
//
// The check is deliberately one-directional. An id that exists in the markup
// with no TypeScript reference is fine and common — structural containers and
// `<label for=…>` targets have none. This catches a reference that points at
// nothing, on either side of the rename; it does not find dead markup.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, it } from "vitest";

import { parseIndexHtml } from "./dom-harness";

/** One name the TypeScript expects `index.html` to contain. */
interface ElementReference {
  /** The module whose lookup asked for it, so a failure says where to look. */
  module: string;
  /** An `id` (global) or a `data-role` (scoped to a container). */
  kind: "id" | "role";
  name: string;
}

interface Scan {
  references: ElementReference[];
  /** Lookups whose argument the scanner could not reduce to a literal. Always
   * asserted empty: a silent skip would let a whole module drop out of the
   * check without anyone noticing. */
  unresolved: string[];
}

/** Replace every comment with spaces, preserving offsets, string literals and
 * line breaks.
 *
 * Comments in this codebase are dense and quote the very identifiers being
 * matched (`[`requireElement`]`), and `DOCS_URL` contains a `//` inside a
 * string, so neither a naive strip nor no strip at all works. Tracks template
 * literals and their `${…}` holes because both appear in the modules scanned. */
function blankComments(source: string): string {
  const out = [...source];
  const stack: { kind: "code" | "template"; braces: number }[] = [{ kind: "code", braces: 0 }];
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to; k += 1) {
      if (out[k] !== "\n") {
        out[k] = " ";
      }
    }
  };
  let i = 0;
  while (i < source.length) {
    const top = stack[stack.length - 1];
    const ch = source[i];
    if (top.kind === "template") {
      if (ch === "\\") {
        i += 2;
      } else if (ch === "`") {
        stack.pop();
        i += 1;
      } else if (ch === "$" && source[i + 1] === "{") {
        stack.push({ kind: "code", braces: 0 });
        i += 2;
      } else {
        i += 1;
      }
      continue;
    }
    if (ch === "/" && source[i + 1] === "/") {
      const end = source.indexOf("\n", i);
      const stop = end < 0 ? source.length : end;
      blank(i, stop);
      i = stop;
    } else if (ch === "/" && source[i + 1] === "*") {
      const end = source.indexOf("*/", i + 2);
      const stop = end < 0 ? source.length : end + 2;
      blank(i, stop);
      i = stop;
    } else if (ch === '"' || ch === "'") {
      i += 1;
      while (i < source.length && source[i] !== ch) {
        i += source[i] === "\\" ? 2 : 1;
      }
      i += 1;
    } else if (ch === "`") {
      stack.push({ kind: "template", braces: 0 });
      i += 1;
    } else if (ch === "{") {
      top.braces += 1;
      i += 1;
    } else if (ch === "}") {
      if (top.braces === 0 && stack.length > 1) {
        stack.pop();
      } else {
        top.braces = Math.max(top.braces - 1, 0);
      }
      i += 1;
    } else {
      i += 1;
    }
  }
  return out.join("");
}

/** The comma-separated arguments of the call whose `(` is at `openParen`,
 * ignoring commas nested in brackets or inside string literals. */
function readArguments(code: string, openParen: number): string[] {
  const args: string[] = [];
  let depth = 0;
  let start = openParen + 1;
  let i = openParen;
  while (i < code.length) {
    const ch = code[i];
    if (ch === '"' || ch === "'" || ch === "`") {
      i += 1;
      while (i < code.length && code[i] !== ch) {
        i += code[i] === "\\" ? 2 : 1;
      }
      i += 1;
    } else if (ch === "(" || ch === "[" || ch === "{") {
      depth += 1;
      i += 1;
    } else if (ch === ")" || ch === "]" || ch === "}") {
      depth -= 1;
      if (depth === 0) {
        args.push(code.slice(start, i).trim());
        return args.filter((arg) => arg.length > 0);
      }
      i += 1;
    } else if (ch === "," && depth === 1) {
      args.push(code.slice(start, i).trim());
      start = i + 1;
      i += 1;
    } else {
      i += 1;
    }
  }
  return [];
}

/** The value of `expression` if it is a plain string literal. */
function stringLiteral(expression: string): string | undefined {
  return /^(["'])([^"'\\]*)\1$/.exec(expression)?.[2];
}

/** The declared parameters, stripped of their types and defaults, so their
 * positions line up with a call's arguments. */
function parameterNames(parameters: readonly string[]): string[] {
  return parameters.map((parameter) => /^\.{0,3}([A-Za-z_$][\w$]*)/.exec(parameter)?.[1] ?? "");
}

/** Every call of `name` anywhere in `sources`, as argument lists. */
function callSites(sources: ReadonlyMap<string, string>, name: string): string[][] {
  const calls: string[][] = [];
  for (const code of sources.values()) {
    const pattern = new RegExp(String.raw`\b${name}\s*(?:<[^<>()]*>)?\s*\(`, "g");
    for (const match of code.matchAll(pattern)) {
      calls.push(readArguments(code, match.index + match[0].length - 1));
    }
  }
  return calls;
}

/** The literals a caller-supplied name can actually be.
 *
 * `createStatuslineSetup(backend, containerId)` looks up an id it was handed,
 * so the id is not in that module at all — it is in each of its call sites.
 * Resolving it means finding the function the lookup sits in, which parameter
 * position `identifier` occupies, and what every call passes there. One
 * mechanism rather than a special case per module: it resolves the status-line
 * block's two containers and `main.ts`'s two views alike. One level deep on
 * purpose — nothing here forwards a name twice, and the empty result is a
 * loud failure rather than a silent skip. */
function resolveIdentifier(
  sources: ReadonlyMap<string, string>,
  code: string,
  offset: number,
  identifier: string,
): string[] {
  let enclosing: RegExpExecArray | undefined;
  for (const match of code.matchAll(/\bfunction\s+([A-Za-z_$][\w$]*)\s*\(/g)) {
    if (match.index < offset) {
      enclosing = match;
    }
  }
  if (!enclosing) {
    return [];
  }
  const parameters = readArguments(code, enclosing.index + enclosing[0].length - 1);
  const position = parameterNames(parameters).indexOf(identifier);
  if (position < 0) {
    return [];
  }
  return callSites(sources, enclosing[1])
    .map((args) => stringLiteral(args[position] ?? ""))
    .filter((literal): literal is string => literal !== undefined);
}

/** Every id and `data-role` the given modules ask the markup for.
 *
 * `sources` must already have been through [`blankComments`], so a lookup
 * quoted in prose is not mistaken for one the app performs. */
function extractReferences(sources: ReadonlyMap<string, string>): Scan {
  const references: ElementReference[] = [];
  const unresolved: string[] = [];
  for (const [module, code] of sources) {
    const pattern = /\b(requireElement|requireChild|getElementById)\s*(?:<[^<>()]*>)?\s*\(/g;
    for (const match of code.matchAll(pattern)) {
      const helper = match[1];
      const open = match.index + match[0].length - 1;
      const args = readArguments(code, open);
      const argument = args[helper === "requireChild" ? 1 : 0] ?? "";
      const kind = helper === "requireChild" ? "role" : "id";
      const literal = stringLiteral(argument);
      if (literal !== undefined) {
        references.push({ module, kind, name: literal });
        continue;
      }
      const resolved = /^[A-Za-z_$][\w$]*$/.test(argument)
        ? resolveIdentifier(sources, code, open, argument)
        : [];
      if (resolved.length === 0) {
        unresolved.push(`${module}: ${helper}(${argument})`);
        continue;
      }
      for (const name of resolved) {
        references.push({ module, kind, name });
      }
    }
  }
  return { references, unresolved };
}

/** The references `markup` cannot satisfy, named so a failure is actionable. */
function unsatisfied(references: readonly ElementReference[], markup: Document): string[] {
  return references
    .filter(({ kind, name }) =>
      kind === "id"
        ? markup.getElementById(name) === null
        : markup.querySelector(`[data-role="${name}"]`) === null,
    )
    .map(({ module, kind, name }) => `${module} wants ${kind} "${name}"`);
}

// `node:path` rather than `new URL(…, import.meta.url)`: see `dom-harness.ts`
// — under jsdom the global `URL` resolves against the document, not the file.
const SRC_DIR = dirname(fileURLToPath(import.meta.url));

/** Every shipped module, comments blanked. `dom.ts` is left out because it
 * *defines* the two helpers — its `requireElement(id)` is a declaration, not
 * a lookup. Test files are left out because this one deliberately contains a
 * typo'd id. */
function readModules(): Map<string, string> {
  const modules = new Map<string, string>();
  for (const name of readdirSync(SRC_DIR)) {
    if (!name.endsWith(".ts") || name.endsWith(".test.ts") || name === "dom.ts") {
      continue;
    }
    modules.set(name, blankComments(readFileSync(join(SRC_DIR, name), "utf8")));
  }
  return modules;
}

const MODULES = readModules();
const SCAN = extractReferences(MODULES);
const MARKUP = parseIndexHtml();

it("resolves every element id the TypeScript asks for against index.html", () => {
  const ids = SCAN.references.filter(({ kind }) => kind === "id");
  expect(unsatisfied(ids, MARKUP)).toEqual([]);
});

it("resolves every data-role the TypeScript asks for against index.html", () => {
  const roles = SCAN.references.filter(({ kind }) => kind === "role");
  expect(unsatisfied(roles, MARKUP)).toEqual([]);
});

it("reduces every lookup it finds to a name, rather than skipping the hard ones", () => {
  // A lookup the scanner cannot reduce is a hole in the check, so it fails
  // here instead of quietly shrinking what the two tests above cover.
  expect(SCAN.unresolved).toEqual([]);
});

it("checks at least one reference from every module that resolves elements by hand", () => {
  // The guard against a green run that scanned nothing: a broken regex or a
  // changed file layout would empty the reference list, and every assertion
  // above passes on an empty list.
  const checked = new Set(SCAN.references.map(({ module }) => module));
  const resolvers = [...MODULES]
    .filter(([, code]) => /\b(requireElement|requireChild|getElementById)\s*[<(]/.test(code))
    .map(([module]) => module);

  expect(resolvers.length).toBeGreaterThan(0);
  expect(resolvers.filter((module) => !checked.has(module))).toEqual([]);
});

it("resolves a caller-supplied container id back to the literals its call sites pass", () => {
  // A self-check of the scanner, not of the markup: these four ids reach the
  // check only through `resolveIdentifier`, so their absence would mean the
  // status-line block and both views had silently stopped being covered.
  const ids = new Set(SCAN.references.filter(({ kind }) => kind === "id").map(({ name }) => name));

  expect(ids).toContain("statusline-setup");
  expect(ids).toContain("wizard-statusline-setup");
  expect(ids).toContain("popover-view");
  expect(ids).toContain("settings-view");
});

it("notices an id index.html does not have", () => {
  const synthetic = new Map([
    ["synthetic-view.ts", blankComments('requireElement<HTMLElement>("wizard-panel-typo");')],
  ]);

  const scan = extractReferences(synthetic);

  expect(scan.unresolved).toEqual([]);
  expect(unsatisfied(scan.references, MARKUP)).toEqual([
    'synthetic-view.ts wants id "wizard-panel-typo"',
  ]);
});

it("notices a data-role index.html does not have", () => {
  const synthetic = new Map([
    ["synthetic-view.ts", blankComments('requireChild<HTMLElement>(container, "commandd");')],
  ]);

  const scan = extractReferences(synthetic);

  expect(unsatisfied(scan.references, MARKUP)).toEqual(['synthetic-view.ts wants role "commandd"']);
});

it("reads no lookup out of prose that merely mentions one", () => {
  // Comment density here is high and quotes these identifiers constantly, so
  // a scanner that read comments would drown the real references in names
  // nobody ever looks up.
  const synthetic = new Map([
    ["synthetic-view.ts", blankComments('// requireElement<HTMLElement>("not-a-real-id")\n')],
  ]);

  expect(extractReferences(synthetic)).toEqual({ references: [], unresolved: [] });
});
