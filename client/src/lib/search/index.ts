/**
 * Client-side search over the loaded vault tree.
 *
 * The whole tree is already in memory (it is replicated to every member, so it
 * is small by definition), which means search is a synchronous pass over an
 * object map and can run on every keystroke with no request, no index and no
 * debounce fighting the caret. That is what makes results feel like they were
 * already there.
 *
 * Everything the ranking needs is injected through `SearchContext` — member
 * names, vault names, and the icon category of a file — so this module stays a
 * pure function of its inputs and never imports the backend client or the icon
 * registry. Budget: 5,000 nodes in well under 10 ms, which is why each node is
 * lowercased at most once per call and no regex is built inside the loop.
 */

import { splitName } from "@/lib/path";

import { fuzzyMatch } from "./fuzzy";
import { parseQuery } from "./query";
import type { ParsedQuery } from "./query";

export { fuzzyMatch, highlightRanges } from "./fuzzy";
export { parseQuery, describeQuery } from "./query";
export type { FuzzyResult } from "./fuzzy";
export type { ParsedQuery } from "./query";

/** The projection of a node that search needs; the real node type is a superset. */
export interface SearchableNode {
  id: string;
  vaultId: string;
  parentId: string | null;
  name: string;
  kind: "folder" | "file";
  modifiedAt: number;
  createdBy: string;
  modifiedBy: string;
  availability: "local" | "remote" | "downloading";
}

export interface SearchContext {
  nodesByVault: Record<string, Record<string, SearchableNode>>;
  vaultNames: Record<string, string>;
  /** peerId → display name, so `by:maya` can match a person rather than a key. */
  memberNames: Record<string, string>;
  /** Injected so this library never imports the icon registry. */
  categoryOf(name: string): string;
  now?: number;
}

export interface SearchOptions {
  /** `null` searches every loaded vault — the global palette; a string scopes to one vault. */
  vaultId: string | null;
  limit: number;
}

export interface RankedHit {
  node: SearchableNode;
  vaultName: string;
  /** Ancestor names from below the root down to the parent, inclusive. */
  path: string[];
  score: number;
  /** Matched runs in `node.name`, ready for {@link highlightRanges}. */
  matches: [number, number][];
}

/** A path match says less about intent than a name match, so it counts for half. */
const PATH_WEIGHT = 0.5;

/** Does `by:` name this peer — either by display name or by a fragment of the id? */
function matchesPerson(peerId: string, fragment: string, memberNames: Record<string, string>): boolean {
  if (peerId.toLowerCase().includes(fragment)) return true;
  const name = memberNames[peerId];
  return name !== undefined && name.toLowerCase().includes(fragment);
}

/**
 * Rank the nodes of one vault (or all of them) against a raw query string.
 *
 * Root nodes are never candidates — "Designs" the vault is not a result inside
 * itself. With no free text the filters stand alone and the answer is simply
 * the most recently touched things, which is also what the empty search modal
 * shows. With text, the name carries the ranking and the path only breaks ties,
 * so typing a folder name surfaces the folder before everything inside it.
 */
export function searchNodes(raw: string, ctx: SearchContext, opts: SearchOptions): RankedHit[] {
  const now = ctx.now ?? Date.now();
  const q: ParsedQuery = parseQuery(raw, now);
  const limit = Math.max(0, opts.limit);
  if (limit === 0) return [];

  const vaultIds = opts.vaultId === null ? Object.keys(ctx.nodesByVault) : [opts.vaultId];
  const byFragment = q.by ? q.by.toLowerCase() : null;
  const inFragment = q.inFolder ? q.inFolder.toLowerCase() : null;
  const hits: RankedHit[] = [];

  for (const vaultId of vaultIds) {
    const nodes = ctx.nodesByVault[vaultId];
    if (!nodes) continue;
    const vaultName = ctx.vaultNames[vaultId] ?? "";
    const pathCache = new Map<string, string[]>();

    /**
     * Ancestor names, root excluded, parent last. Memoised because siblings share it.
     *
     * The walk is iterative and carries a visited set rather than recursing: two
     * peers moving folders into each other concurrently can land a parent cycle in
     * the replicated tree, and search must degrade to the partial path it has
     * rather than overflow the stack under the user's caret (`pathOf` guards the
     * same case for the same reason).
     */
    const ancestorNames = (node: SearchableNode): string[] => {
      const cached = pathCache.get(node.id);
      if (cached) return cached;

      // Climb to the first node whose ancestors are known — a child of the root, a
      // dangling parent, a cache hit, or a repeat that proves a cycle.
      const chain: SearchableNode[] = [];
      const seen = new Set<string>([node.id]);
      let names: string[] = [];
      let current = node;
      for (;;) {
        chain.push(current);
        const parent = current.parentId ? nodes[current.parentId] : undefined;
        if (!parent || parent.parentId === null || seen.has(parent.id)) break;
        const above = pathCache.get(parent.id);
        if (above) {
          names = [...above, parent.name];
          break;
        }
        seen.add(parent.id);
        current = parent;
      }

      // Unwind back down, filling the cache for every node the climb passed through.
      pathCache.set(chain[chain.length - 1].id, names);
      for (let i = chain.length - 2; i >= 0; i--) {
        names = [...names, chain[i + 1].name];
        pathCache.set(chain[i].id, names);
      }
      return names;
    };

    for (const id in nodes) {
      const node = nodes[id];
      if (node.parentId === null) continue;

      if (q.kinds && !q.kinds.includes(node.kind)) continue;
      if (q.availability && node.availability !== q.availability) continue;
      if (q.modifiedAfter !== null && node.modifiedAt < q.modifiedAfter) continue;
      if (q.exts && !q.exts.includes(splitName(node.name).ext.toLowerCase())) continue;
      if (q.category && ctx.categoryOf(node.name) !== q.category) continue;
      if (
        byFragment &&
        !matchesPerson(node.createdBy, byFragment, ctx.memberNames) &&
        !matchesPerson(node.modifiedBy, byFragment, ctx.memberNames)
      ) {
        continue;
      }

      const path = ancestorNames(node);
      if (inFragment && !path.some((name) => name.toLowerCase().includes(inFragment))) continue;

      if (q.text.length === 0) {
        hits.push({ node, vaultName, path, score: 0, matches: [] });
        continue;
      }

      const nameHit = fuzzyMatch(q.text, node.name);
      const pathHit = path.length > 0 ? fuzzyMatch(q.text, path.join("/")) : null;
      if (!nameHit && !pathHit) continue;

      const score =
        (nameHit ? nameHit.score : 0) + (pathHit ? Math.round(pathHit.score * PATH_WEIGHT) : 0);
      hits.push({ node, vaultName, path, score, matches: nameHit ? nameHit.matches : [] });
    }
  }

  hits.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    if (b.node.modifiedAt !== a.node.modifiedAt) return b.node.modifiedAt - a.node.modifiedAt;
    return a.node.name.localeCompare(b.node.name);
  });

  return hits.length > limit ? hits.slice(0, limit) : hits;
}
