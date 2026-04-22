import { createSignal, onMount, Switch, Match, Show, For } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { store, setStore } from "../lib/store";
import { KeyTable } from "../components/KeyTable";
import { LogTable } from "../components/LogTable";
import { ApprovalDialog } from "../components/ApprovalDialog";
import {
  listKeys,
  getAccessLogs,
  approveRequest,
  getPendingApprovals,
  lockVault,
  manualSync,
  type ApprovalRequest,
} from "../lib/tauri";

type Tab = "keys" | "logs" | "approvals";

function navigate(path: string) {
  window.location.hash = "#" + path;
}

export default function DashboardPage() {
  const [activeTab, setActiveTab] = createSignal<Tab>("keys");
  const [currentApproval, setCurrentApproval] = createSignal<ApprovalRequest | null>(null);

  // Manual data fetching — no createResource/Suspense blocking
  const [keys, setKeys] = createSignal<import("../lib/tauri").SshKeyInfo[]>([]);
  const [logs, setLogs] = createSignal<import("../lib/tauri").AccessLogEntry[]>([]);
  const [keysLoading, setKeysLoading] = createSignal(true);
  const [logsLoading, setLogsLoading] = createSignal(true);

  const [synced, setSynced] = createSignal(false);
  const [syncing, setSyncing] = createSignal(false);

  const fetchKeys = async () => {
    setKeysLoading(true);
    try {
      setKeys(await listKeys());
      setKeysLoading(false);
      setSynced(true);
    } catch {
      if (synced()) {
        setStore("locked", true);
        navigate("/");
      }
    }
  };
  const fetchLogs = async () => {
    setLogsLoading(true);
    try {
      setLogs(await getAccessLogs(50));
      setLogsLoading(false);
    } catch {
      if (synced()) {
        setStore("locked", true);
        navigate("/");
      }
    }
  };

  onMount(async () => {
    // Kick off data fetches in parallel — non-blocking
    fetchKeys();
    fetchLogs();

    try {
      const pending = await getPendingApprovals();
      setStore("pendingApprovals", pending);
    } catch (e) {
      console.error("Failed to fetch pending approvals:", e);
    }

    // Listen for lock state changes — navigate to login when locked
    const unlistenLock = await listen<{ locked: boolean }>("lock-state-changed", (event) => {
      if (event.payload.locked) {
        setStore("locked", true);
        navigate("/");
      }
    });

    // Listen for new approval requests
    const unlistenApproval = await listen<ApprovalRequest>("approval-requested", (event) => {
      setCurrentApproval(event.payload);
    });

    // Listen for vault sync completion — refresh data
    const unlistenSync = await listen<{ success: boolean; error: string | null }>("vault-synced", (event) => {
      setSyncing(false);
      if (event.payload.success) {
        fetchKeys();
        fetchLogs();
      } else {
        console.error("Vault sync failed:", event.payload.error);
      }
    });

    return () => {
      unlistenLock();
      unlistenApproval();
      unlistenSync();
    };
  });

  const handleTabChange = (tab: Tab) => {
    setActiveTab(tab);
    if (tab === "keys") fetchKeys();
    if (tab === "logs") fetchLogs();
  };

  const handleLock = async () => {
    try {
      await lockVault();
      navigate("/");
    } catch (e) {
      console.error("Failed to lock vault:", e);
    }
  };

  const handleSync = async () => {
    if (syncing()) return;
    setSyncing(true);
    try {
      await manualSync();
    } catch (e) {
      console.error("Manual sync failed:", e);
      setSyncing(false);
    }
  };

  const handleApprovalResponse = async (requestId: string, approved: boolean) => {
    try {
      await approveRequest(requestId, approved);
      setStore("pendingApprovals", (prev) => prev.filter((req) => req.id !== requestId));
      if (currentApproval()?.id === requestId) {
        setCurrentApproval(null);
      }
      if (activeTab() === "logs") {
        fetchLogs();
      }
    } catch (e) {
      console.error("Failed to respond to approval:", e);
    }
  };

  return (
    <div class="flex h-screen" style={`background: var(--bg-secondary)`}>
      {/* ── Sidebar ──────────────────────────────────────────────── */}
      <aside
        class="flex flex-col shrink-0 h-full border-r"
        style={{
          width: "var(--sidebar-width)",
          background: "var(--bg-primary)",
          "border-color": "var(--border-primary)",
        }}
      >
        {/* Brand */}
        <div class="flex items-center gap-2.5 px-5" style={`height: var(--header-height)`}>
          <div class="flex h-8 w-8 items-center justify-center">
            <svg width="32" height="32" viewBox="0 0 100 100" fill="none">
              <rect width="100" height="100" rx="18" fill="#4f46e5" />
              <circle cx="50" cy="50" r="30" fill="#FFFFFF" />
              <rect x="44" y="14" width="12" height="13" rx="3" fill="#4f46e5" />
              <rect x="44" y="73" width="12" height="13" rx="3" fill="#4f46e5" />
              <rect x="14" y="44" width="13" height="12" rx="3" fill="#4f46e5" />
              <rect x="73" y="44" width="13" height="12" rx="3" fill="#4f46e5" />
              <circle cx="50" cy="50" r="17" fill="#4f46e5" />
              <circle cx="50" cy="46.5" r="5.5" fill="#FFFFFF" />
              <path d="M46 53 L46.8 62 H53.2 L54 53 Z" fill="#FFFFFF" />
            </svg>
          </div>
          <span class="text-sm font-semibold" style={`color: var(--text-primary)`}>BW Agent</span>
        </div>

        {/* Nav items */}
        <div class="divider" style={{ margin: "0 16px" }}></div>
        <nav class="flex-1 px-3 py-2 space-y-0.5">
          <button
            onClick={() => handleTabChange("keys")}
            classList={{
              "sidebar-item": true,
              "active": activeTab() === "keys",
            }}
          >
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
            </svg>
            SSH Keys
          </button>
          <button
            onClick={() => handleTabChange("logs")}
            classList={{
              "sidebar-item": true,
              "active": activeTab() === "logs",
            }}
          >
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            Access Logs
          </button>
          <button
            onClick={() => handleTabChange("approvals")}
            classList={{
              "sidebar-item": true,
              "active": activeTab() === "approvals",
            }}
          >
            <div class="flex items-center gap-2.5 w-full">
              <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
              </svg>
              <span class="flex-1 text-left">Approvals</span>
              <Show when={store.pendingApprovals.length > 0}>
                <span class="badge badge-danger" style={{ "font-size": "11px", padding: "1px 6px" }}>
                  {store.pendingApprovals.length}
                </span>
              </Show>
            </div>
          </button>
        </nav>

        {/* Bottom actions */}
        <div class="divider" style={{ margin: "0 16px" }}></div>
        <div class="px-3 py-2 space-y-0.5">
          <button onClick={handleSync} disabled={syncing()} class="sidebar-item" style={{ opacity: syncing() ? 0.5 : 1 }}>
            <svg class={syncing() ? "animate-spin" : ""} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
            </svg>
            {syncing() ? "Syncing..." : "Sync Vault"}
          </button>
          <button onClick={() => navigate("/settings")} class="sidebar-item">
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281z" />
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
            Settings
          </button>
          <button onClick={handleLock} class="sidebar-item" style={`color: var(--danger)`}>
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
            </svg>
            Lock Vault
          </button>
        </div>

        {/* User info */}
        <div class="divider" style={{ margin: "0 16px" }}></div>
        <div class="px-4 py-3 flex items-center gap-2.5">
          <div class="flex h-7 w-7 items-center justify-center rounded-full text-xs font-medium" style={`background: var(--brand-100); color: var(--brand-700)`}>
            {(store.email || "U")[0].toUpperCase()}
          </div>
          <span class="text-xs truncate" style={`color: var(--text-secondary)`}>{store.email}</span>
        </div>
      </aside>

      {/* ── Main Content ─────────────────────────────────────────── */}
      <main class="flex-1 flex flex-col min-w-0 overflow-hidden">
        {/* Header */}
        <header
          class="flex items-center justify-between px-8 shrink-0 border-b"
          style={{
            height: "var(--header-height)",
            background: "var(--bg-primary)",
            "border-color": "var(--border-primary)",
          }}
        >
          <h2 class="text-base font-semibold" style={`color: var(--text-primary)`}>
            <Switch>
              <Match when={activeTab() === "keys"}>SSH Keys</Match>
              <Match when={activeTab() === "logs"}>Access Logs</Match>
              <Match when={activeTab() === "approvals"}>Pending Approvals</Match>
            </Switch>
          </h2>
          <div class="flex items-center gap-2">
            <Show when={activeTab() === "keys"}>
              <span class="text-xs" style={`color: var(--text-tertiary)`}>
                {keys().length} key{keys().length !== 1 ? "s" : ""}
              </span>
            </Show>
          </div>
        </header>

        {/* Content */}
        <div class="flex-1 overflow-auto p-8">
          <Switch>
            <Match when={activeTab() === "keys"}>
              <Show when={!keysLoading()} fallback={
                <div class="flex items-center justify-center py-20">
                  <svg class="w-6 h-6 animate-spin" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                </div>
              }>
                <KeyTable keys={keys()} />
              </Show>
            </Match>
            
            <Match when={activeTab() === "logs"}>
              <Show when={!logsLoading()} fallback={
                <div class="flex items-center justify-center py-20">
                  <svg class="w-6 h-6 animate-spin" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                </div>
              }>
                <LogTable logs={logs()} />
              </Show>
            </Match>
            
            <Match when={activeTab() === "approvals"}>
              <div class="space-y-3">
                <Show
                  when={store.pendingApprovals.length > 0}
                  fallback={
                    <div class="card empty-state">
                      <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                      </svg>
                      <h3>No pending approvals</h3>
                      <p>You're all caught up.</p>
                    </div>
                  }
                >
                  <For each={store.pendingApprovals}>
                    {(req) => (
                      <div class="card" style={{ padding: "20px 24px" }}>
                        <div class="flex items-start justify-between gap-4">
                          <div class="min-w-0">
                            <h4 class="text-sm font-semibold" style={`color: var(--text-primary)`}>{req.key_name}</h4>
                            <div class="mt-1.5 flex flex-wrap gap-x-4 gap-y-1">
                              <span class="text-xs flex items-center gap-1" style={`color: var(--text-tertiary)`}>
                                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1.007a3 3 0 01-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0115 18.257V17.25m6-12V15a2.25 2.25 0 01-2.25 2.25H5.25A2.25 2.25 0 013 15V5.25m18 0A2.25 2.25 0 0018.75 3H5.25A2.25 2.25 0 003 5.25m18 0V12a2.25 2.25 0 01-2.25 2.25H5.25A2.25 2.25 0 013 12V5.25" />
                                </svg>
                                {req.client_exe} (PID: {req.client_pid})
                              </span>
                              <span class="text-xs flex items-center gap-1" style={`color: var(--text-tertiary)`}>
                                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                                {new Date(req.timestamp * 1000).toLocaleString()}
                              </span>
                            </div>
                          </div>
                          <div class="flex gap-2 shrink-0">
                            <button
                              onClick={() => handleApprovalResponse(req.id, false)}
                              class="btn btn-ghost text-xs"
                              style={`color: var(--danger)`}
                            >
                              Deny
                            </button>
                            <button
                              onClick={() => handleApprovalResponse(req.id, true)}
                              class="btn btn-primary text-xs"
                            >
                              Approve
                            </button>
                          </div>
                        </div>
                      </div>
                    )}
                  </For>
                </Show>
              </div>
            </Match>
          </Switch>
        </div>
      </main>

      {/* Modal Overlay */}
      <ApprovalDialog
        request={currentApproval()}
        onRespond={handleApprovalResponse}
      />
    </div>
  );
}
