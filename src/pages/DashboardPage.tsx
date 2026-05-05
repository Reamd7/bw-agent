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
  listActiveSessions,
  revokeSession,
  type ApprovalRequest,
  type ApprovalSessionInfo,
} from "../lib/tauri";

type Tab = "keys" | "logs" | "approvals" | "sessions";

function navigate(path: string) {
  window.location.hash = "#" + path;
}

export default function DashboardPage() {
  const [activeTab, setActiveTab] = createSignal<Tab>("keys");
  const [currentApproval, setCurrentApproval] = createSignal<ApprovalRequest | null>(null);

  // Manual data fetching — no createResource/Suspense blocking
  const [keys, setKeys] = createSignal<import("../lib/tauri").SshKeyInfo[]>([]);
  const [logs, setLogs] = createSignal<import("../lib/tauri").AccessLogEntry[]>([]);

  const handleKeyUpdated = (entryId: string, updatedFields: import("../lib/tauri").CustomFieldInfo[]) => {
    setKeys((prev) =>
      prev.map((k) =>
        k.entry_id === entryId
          ? {
              ...k,
              custom_fields: updatedFields,
              match_patterns: updatedFields
                .filter((f) => f.name === "gh-match")
                .map((f) => f.value),
            }
          : k,
      ),
    );
  };
  const [keysLoading, setKeysLoading] = createSignal(true);
  const [logsLoading, setLogsLoading] = createSignal(true);

  const [sessions, setSessions] = createSignal<ApprovalSessionInfo[]>([]);
  const [sessionsLoading, setSessionsLoading] = createSignal(false);

  const [synced, setSynced] = createSignal(false);
  const [syncing, setSyncing] = createSignal(false);

  // Countdown timer — updates sessions every second to refresh remaining time
  let sessionsInterval: ReturnType<typeof setInterval> | undefined;
  const startSessionsTimer = () => {
    if (sessionsInterval) clearInterval(sessionsInterval);
    sessionsInterval = setInterval(() => {
      setSessions((prev) =>
        prev
          .map((s) => ({
            ...s,
            remaining_secs: Math.max(0, s.expires_at_unix - Math.floor(Date.now() / 1000)),
          }))
          .filter((s) => s.remaining_secs > 0),
      );
    }, 1000);
  };

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
  const fetchSessions = async () => {
    setSessionsLoading(true);
    try {
      setSessions(await listActiveSessions());
      setSessionsLoading(false);
      startSessionsTimer();
    } catch {
      setSessionsLoading(false);
    }
  };

  onMount(async () => {
    // Kick off data fetches in parallel — non-blocking
    fetchKeys();
    fetchLogs();
    fetchSessions();

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
    if (tab === "sessions") fetchSessions();
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

  const handleApprovalResponseWithSession = async (requestId: string, durationSecs: number, scopeType: string, scopeExePath?: string) => {
    try {
      await import("../lib/tauri").then(m => m.approveRequestWithSession(requestId, durationSecs, scopeType, scopeExePath));
      setStore("pendingApprovals", (prev) => prev.filter((req) => req.id !== requestId));
      if (currentApproval()?.id === requestId) {
        setCurrentApproval(null);
      }
      if (activeTab() === "logs") {
        fetchLogs();
      }
      // Refresh sessions so sidebar badge updates immediately
      fetchSessions();
    } catch (e) {
      console.error("Failed to respond to approval with session:", e);
    }
  };

  const handleRevokeSession = async (sessionId: string) => {
    setSessions((s) => s.filter((session) => session.id !== sessionId));
    await revokeSession(sessionId);
  };

  const formatRemaining = (secs: number): string => {
    if (secs <= 0) return "Expired";
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    if (h > 0) return `${h}h ${m}m ${s}s`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  };

  const scopeLabel = (scope: import("../lib/tauri").SessionScope): string => {
    if (scope.type === "any_process") return "Any Process";
    return scope.exe_path?.split(/[/\\]/).pop() || "Executable";
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
          <button
            onClick={() => handleTabChange("sessions")}
            classList={{
              "sidebar-item": true,
              "active": activeTab() === "sessions",
            }}
          >
            <div class="flex items-center gap-2.5 w-full">
              <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <span class="flex-1 text-left">Sessions</span>
              <Show when={sessions().length > 0}>
                <span class="badge" style={{ "font-size": "11px", padding: "1px 6px", background: "var(--brand-100)", color: "var(--brand-700)" }}>
                  {sessions().length}
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
              <Match when={activeTab() === "sessions"}>Active Sessions</Match>
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
                <KeyTable keys={keys()} onRefresh={fetchKeys} onKeyUpdated={handleKeyUpdated} />
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

            <Match when={activeTab() === "sessions"}>
              <Show
                when={!sessionsLoading()}
                fallback={
                  <div class="flex items-center justify-center py-20">
                    <svg class="w-6 h-6 animate-spin" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24">
                      <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                      <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                    </svg>
                  </div>
                }
              >
                <Show
                  when={sessions().length > 0}
                  fallback={
                    <div class="card empty-state">
                      <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                      </svg>
                      <h3>No active sessions</h3>
                      <p>Approve a request with "Remember" to create a session.</p>
                    </div>
                  }
                >
                  <div class="space-y-3">
                    <For each={sessions()}>
                      {(session) => {
                        const pct = () => {
                          const total = session.expires_at_unix - session.created_at_unix;
                          if (total <= 0) return 0;
                          return Math.max(0, Math.min(100, (session.remaining_secs / total) * 100));
                        };
                        const isLow = () => session.remaining_secs < 120;
                        return (
                          <div class="card" style={{ padding: "20px 24px" }}>
                            <div class="flex items-start justify-between gap-4">
                              <div class="min-w-0 flex-1">
                                <div class="flex items-center gap-2">
                                  <h4 class="text-sm font-semibold" style={`color: var(--text-primary)`}>
                                    {session.key_fingerprint.slice(0, 16)}…
                                  </h4>
                                  <span
                                    class="text-xs px-1.5 py-0.5 rounded"
                                    style={{
                                      background: session.scope.type === "any_process" ? "var(--warning-bg, #fef3c7)" : "var(--brand-100)",
                                      color: session.scope.type === "any_process" ? "var(--warning-text, #92400e)" : "var(--brand-700)",
                                    }}
                                  >
                                    {session.scope.type === "any_process" ? "⚠ Any Process" : `🔗 ${scopeLabel(session.scope)}`}
                                  </span>
                                </div>
                                <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1">
                                  <span class="text-xs flex items-center gap-1" style={`color: var(--text-tertiary)`}>
                                    Uses: {session.usage_count}
                                  </span>
                                  <span class="text-xs flex items-center gap-1" style={`color: var(--text-tertiary)`}>
                                    Created: {new Date(session.created_at_unix * 1000).toLocaleTimeString()}
                                  </span>
                                </div>
                                {/* Countdown bar */}
                                <div class="mt-3">
                                  <div class="flex items-center justify-between mb-1">
                                    <span class="text-xs font-medium" style={{ color: isLow() ? "var(--danger)" : "var(--text-secondary)" }}>
                                      {formatRemaining(session.remaining_secs)}
                                    </span>
                                  </div>
                                  <div class="w-full h-1.5 rounded-full" style={{ background: "var(--bg-secondary)" }}>
                                    <div
                                      class="h-full rounded-full transition-all duration-1000"
                                      style={{
                                        width: `${pct()}%`,
                                        background: isLow() ? "var(--danger)" : "var(--brand-500)",
                                      }}
                                    />
                                  </div>
                                </div>
                                <Show when={session.scope.type === "executable" && session.scope.exe_path}>
                                  <div class="mt-2 text-xs font-mono truncate" style={`color: var(--text-tertiary)`}>
                                    {session.scope.exe_path}
                                  </div>
                                </Show>
                              </div>
                              <button
                                onClick={() => handleRevokeSession(session.id)}
                                class="btn btn-ghost text-xs shrink-0"
                                style={`color: var(--danger)`}
                              >
                                Revoke
                              </button>
                            </div>
                          </div>
                        );
                      }}
                    </For>
                  </div>
                </Show>
              </Show>
            </Match>
          </Switch>
        </div>
      </main>

      {/* Modal Overlay */}
      <ApprovalDialog
        request={currentApproval()}
        onRespond={handleApprovalResponse}
        onRespondWithSession={handleApprovalResponseWithSession}
        onDismiss={() => setCurrentApproval(null)}
      />
    </div>
  );
}
