import { useEffect, useMemo, useRef, useState } from "react";

import { useWorkspace } from "@/features/workspace/store";
import type { NodeKind, SearchHit, SearchQuery, VaultId } from "@/lib/backend";
import { parseQuery } from "@/lib/search";
import type { ParsedQuery } from "@/lib/search";

/**
 * Long enough to swallow the burst of a fast typist, short enough that the list
 * still looks like it is reacting to the keystroke rather than to a pause.
 */
const DEBOUNCE_MS = 60;

/** More than fits on screen; the ranking, not the cut-off, is what people read. */
const DEFAULT_LIMIT = 60;

export interface UseSearchOptions {
  /** null searches every vault the user belongs to (the "All vaults" scope). */
  vaultId: VaultId | null;
  /** Icon category from the chip row ("image", "code", …), or null for no category filter. */
  category: string | null;
  kind: "all" | "folder" | "file";
  /** 0 stops the hook from asking at all — what a closed modal passes. */
  limit?: number;
}

export interface UseSearchResult {
  hits: SearchHit[];
  loading: boolean;
  /** The parsed form of what is actually being searched, chip included. */
  parsed: ParsedQuery;
  error: string | null;
}

/**
 * The chip row and the typed grammar are the same language, so a chip is
 * expressed as the token it stands for rather than as a parallel filter. That
 * keeps one source of truth for "what is being filtered" — the parsed query —
 * which is what the footer reads back to the user.
 */
function compose(raw: string, category: string | null): string {
  if (!category) return raw;
  const text = raw.trim();
  return text.length > 0 ? `${text} type:${category}` : `type:${category}`;
}

/**
 * Ranked results for a raw query string, kept in step with the field.
 *
 * Search is a pass over the in-memory tree (docs/decisions/client-workspace.md),
 * so this deliberately does not show a spinner or clear the list while a request
 * is out: the previous results stay put and are replaced in one frame, which is
 * what makes the modal feel like the answers were already there. A monotonic
 * token discards any response that is no longer the newest — with a debounce and
 * an async client, two keystrokes can otherwise land out of order and leave the
 * list showing the answer to the query before last.
 */
export function useSearch(raw: string, opts: UseSearchOptions): UseSearchResult {
  const client = useWorkspace((s) => s.client);
  const { vaultId, category, kind, limit = DEFAULT_LIMIT } = opts;

  const text = compose(raw, category);
  const parsed = useMemo(() => parseQuery(text), [text]);

  const [hits, setHits] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const token = useRef(0);

  useEffect(() => {
    if (!client || limit <= 0) {
      setHits([]);
      setLoading(false);
      return;
    }

    const mine = ++token.current;
    setLoading(true);

    const query: SearchQuery = {
      text,
      vaultId,
      kinds: kind === "all" ? null : [kind as NodeKind],
      // The rest of the grammar is parsed out of `text` by the search library
      // itself; these structured fields exist for filters that never get typed.
      exts: null,
      inFolderId: null,
      by: null,
      availability: null,
      modifiedAfter: null,
      limit,
    };

    const timer = window.setTimeout(() => {
      void Promise.resolve(client.search(query))
        .then((next) => {
          if (mine !== token.current) return;
          setHits(next);
          setError(null);
          setLoading(false);
        })
        .catch((failure: unknown) => {
          if (mine !== token.current) return;
          setHits([]);
          setError(failure instanceof Error ? failure.message : String(failure));
          setLoading(false);
        });
    }, DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      // Invalidate the in-flight request too: the next effect (or the unmount)
      // owns the list from here.
      token.current += 1;
    };
  }, [client, text, vaultId, kind, limit]);

  return { hits, loading, parsed, error };
}
