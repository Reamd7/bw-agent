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

  const fetchKeys = async () => {
    setKeysLoading(true);
    try {
      setKeys(await listKeys());
      setKeysLoading(false);
      setSynced(true);
    } catch {
      // If we've previously had a successful fetch (synced), a failure
      // means the vault has genuinely locked/expired.
      // If we've never synced, this is just the initial race condition
      // (background sync_and_unlock hasn't finished yet) — stay loading
      // and wait for the vault-synced event to trigger a retry.
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
    <div class="min-h-screen bg-gray-50">
      {/* Header */}
      <header class="bg-white shadow-sm">
        <div class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">
          <div class="flex h-16 justify-between items-center">
            <div class="flex items-center">
              <h1 class="text-xl font-bold text-gray-900">Bitwarden SSH Agent</h1>
            </div>
            <div class="flex items-center space-x-4">
              <span class="text-sm text-gray-500">{store.email}</span>
              <button
                onClick={() => navigate("/settings")}
                class="p-2 text-gray-400 hover:text-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 rounded-full"
                title="Settings"
              >
                <svg class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                </svg>
              </button>
              <button
                onClick={handleLock}
                class="inline-flex items-center rounded-md border border-transparent bg-gray-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-gray-500 focus:ring-offset-2"
              >
                <svg class="-ml-1 mr-2 h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                </svg>
                Lock
              </button>
            </div>
          </div>
        </div>
      </header>

      <main class="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8 py-8">
        {/* Tabs */}
        <div class="mb-8 border-b border-gray-200">
          <nav class="-mb-px flex space-x-8" aria-label="Tabs">
            <button
              onClick={() => handleTabChange("keys")}
              class={`${
                activeTab() === "keys"
                  ? "border-blue-500 text-blue-600"
                  : "border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700"
              } whitespace-nowrap border-b-2 py-4 px-1 text-sm font-medium`}
            >
              SSH Keys
            </button>
            <button
              onClick={() => handleTabChange("logs")}
              class={`${
                activeTab() === "logs"
                  ? "border-blue-500 text-blue-600"
                  : "border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700"
              } whitespace-nowrap border-b-2 py-4 px-1 text-sm font-medium`}
            >
              Access Logs
            </button>
            <button
              onClick={() => handleTabChange("approvals")}
              class={`${
                activeTab() === "approvals"
                  ? "border-blue-500 text-blue-600"
                  : "border-transparent text-gray-500 hover:border-gray-300 hover:text-gray-700"
              } whitespace-nowrap border-b-2 py-4 px-1 text-sm font-medium flex items-center`}
            >
              Pending Approvals
              <Show when={store.pendingApprovals.length > 0}>
                <span class="ml-2 rounded-full bg-red-100 px-2.5 py-0.5 text-xs font-medium text-red-800">
                  {store.pendingApprovals.length}
                </span>
              </Show>
            </button>
          </nav>
        </div>

        {/* Tab Content */}
        <div class="mt-4">
          <Switch>
            <Match when={activeTab() === "keys"}>
              <Show when={!keysLoading()} fallback={<div class="text-center py-10 text-gray-500">Loading keys...</div>}>
                <KeyTable keys={keys()} />
              </Show>
            </Match>
            
            <Match when={activeTab() === "logs"}>
              <Show when={!logsLoading()} fallback={<div class="text-center py-10 text-gray-500">Loading logs...</div>}>
                <LogTable logs={logs()} />
              </Show>
            </Match>
            
            <Match when={activeTab() === "approvals"}>
              <div class="space-y-4">
                <Show
                  when={store.pendingApprovals.length > 0}
                  fallback={
                    <div class="rounded-lg border border-gray-200 bg-white p-8 text-center shadow-sm">
                      <svg class="mx-auto h-12 w-12 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                      </svg>
                      <h3 class="mt-2 text-sm font-medium text-gray-900">No pending approvals</h3>
                      <p class="mt-1 text-sm text-gray-500">You're all caught up.</p>
                    </div>
                  }
                >
                  <For each={store.pendingApprovals}>
                    {(req) => (
                      <div class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
                        <div>
                          <h4 class="text-lg font-medium text-gray-900">{req.key_name}</h4>
                          <div class="mt-1 flex flex-col sm:flex-row sm:flex-wrap sm:space-x-6">
                            <div class="mt-2 flex items-center text-sm text-gray-500 sm:mt-0">
                              <svg class="mr-1.5 h-5 w-5 flex-shrink-0 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
                              </svg>
                              {req.client_exe} (PID: {req.client_pid})
                            </div>
                            <div class="mt-2 flex items-center text-sm text-gray-500 sm:mt-0">
                              <svg class="mr-1.5 h-5 w-5 flex-shrink-0 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                              </svg>
                              {new Date(req.timestamp * 1000).toLocaleString()}
                            </div>
                          </div>
                        </div>
                        <div class="ml-4 flex flex-shrink-0 space-x-3">
                          <button
                            onClick={() => handleApprovalResponse(req.id, false)}
                            class="inline-flex items-center rounded-md border border-transparent bg-red-100 px-4 py-2 text-sm font-medium text-red-700 hover:bg-red-200 focus:outline-none focus:ring-2 focus:ring-red-500 focus:ring-offset-2"
                          >
                            Deny
                          </button>
                          <button
                            onClick={() => handleApprovalResponse(req.id, true)}
                            class="inline-flex items-center rounded-md border border-transparent bg-green-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-green-700 focus:outline-none focus:ring-2 focus:ring-green-500 focus:ring-offset-2"
                          >
                            Approve
                          </button>
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
