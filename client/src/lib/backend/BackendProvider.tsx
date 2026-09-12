import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type { BackendClient } from "./client";
import { createBackend } from "./index";
import type {
  CreateVaultInput,
  DaemonStatus,
  JoinCode,
  OrchestrationServer,
  Vault,
} from "./types";

export interface BackendContextValue {
  client: BackendClient;
  servers: OrchestrationServer[];
  status: DaemonStatus | null;
  loading: boolean;
  error: string | null;
  refresh(): Promise<void>;
  addServer(input: { name: string; address: string }): Promise<OrchestrationServer>;
  createVault(input: CreateVaultInput): Promise<Vault>;
  joinVault(code: JoinCode): Promise<Vault>;
}

const BackendContext = createContext<BackendContextValue | null>(null);

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function BackendProvider({ children }: { children: ReactNode }) {
  // One client for the lifetime of the app: Tauri inside the shell, mock in the browser.
  const [client] = useState<BackendClient>(() => createBackend());
  const [servers, setServers] = useState<OrchestrationServer[]>([]);
  const [status, setStatus] = useState<DaemonStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const alive = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextServers] = await Promise.all([
        client.status(),
        client.listServers(),
      ]);
      if (!alive.current) return;
      setStatus(nextStatus);
      setServers(nextServers);
      setError(null);
    } catch (e) {
      if (!alive.current) return;
      setError(message(e));
    } finally {
      if (alive.current) setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    alive.current = true;
    void refresh();
    const unsubscribe = client.subscribe((e) => {
      switch (e.type) {
        case "servers-changed":
          void refresh();
          break;
        case "daemon-status":
          setStatus(e.status);
          break;
        case "profile-changed":
          // The name is one thing, but a contribution change moves every vault's
          // capacity, and the rows on the home screen read it.
          void refresh();
          break;
        case "vault-removed":
          // The vault list is what changed, and `refresh` is the only thing that reads
          // it.
          void refresh();
          // The reason still has to reach someone. The workspace's own handler
          // explains a removal, but it is subscribed only while a workspace is
          // mounted, so at home a row would vanish with no account of why — which
          // reads as a bug. Announced as a window event rather than written into
          // the shell notice store directly: this is the backend seam, and it
          // stays clear of a feature's store (the home screen owns that line).
          window.dispatchEvent(
            new CustomEvent("qfs:vault-removed", {
              detail: { vaultId: e.vaultId, reason: e.reason },
            }),
          );
          break;
        default:
          // Workspace events (fs, presence, members, recents) belong to the workspace
          // store, not to this provider; ignoring them here keeps the two independent.
          break;
      }
    });
    return () => {
      alive.current = false;
      unsubscribe();
    };
  }, [client, refresh]);

  const addServer = useCallback(
    async (input: { name: string; address: string }) => {
      const server = await client.addServer(input);
      await refresh();
      return server;
    },
    [client, refresh],
  );

  const createVault = useCallback(
    async (input: CreateVaultInput) => {
      const vault = await client.createVault(input);
      await refresh();
      return vault;
    },
    [client, refresh],
  );

  const joinVault = useCallback(
    async (code: JoinCode) => {
      const vault = await client.joinVault(code);
      await refresh();
      return vault;
    },
    [client, refresh],
  );

  const value = useMemo<BackendContextValue>(
    () => ({
      client,
      servers,
      status,
      loading,
      error,
      refresh,
      addServer,
      createVault,
      joinVault,
    }),
    [client, servers, status, loading, error, refresh, addServer, createVault, joinVault],
  );

  return <BackendContext.Provider value={value}>{children}</BackendContext.Provider>;
}

export function useBackend(): BackendContextValue {
  const value = useContext(BackendContext);
  if (!value) throw new Error("useBackend must be used inside <BackendProvider>");
  return value;
}

/**
 * Just the client, for the many callers that only want to invoke the seam.
 *
 * The client is stable for the life of the app while `servers`/`status` change on
 * every event, so depending on the whole context value re-renders those callers for
 * data they never read.
 */
export function useBackendClient(): BackendClient {
  return useBackend().client;
}
