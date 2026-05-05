import { For, Show, createSignal, createEffect } from "solid-js";
import type { ApprovalRequest } from "../lib/tauri";

interface ApprovalDialogProps {
  request: ApprovalRequest | null;
  onRespond: (requestId: string, approved: boolean) => void;
  onRespondWithSession: (requestId: string, durationSecs: number, scopeType: string, scopeExePath?: string) => void;
  onDismiss: () => void;
}

const formatDuration = (secs: number): string => {
  if (secs < 3600) return `${secs / 60}min`;
  if (secs < 7200) return "1h";
  return `${secs / 3600}h`;
};

export function ApprovalDialog(props: ApprovalDialogProps) {
  const [rememberOpen, setRememberOpen] = createSignal(false);
  const [duration, setDuration] = createSignal(900); // 15 minutes default
  const [scopeType, setScopeType] = createSignal<"executable" | "any_process">("executable");

  createEffect(() => {
    if (props.request) {
      setRememberOpen(false);
      setDuration(900);
      setScopeType("executable");
    }
  });

  const formatTime = (timestamp: number) => {
    try {
      return new Date(timestamp * 1000).toLocaleString();
    } catch (e) {
      return timestamp.toString();
    }
  };

  const extractExeName = (fullPath: string) => {
    const parts = fullPath.split(/[/\\]/);
    return parts[parts.length - 1] || fullPath;
  };

  return (
    <Show when={props.request}>
      {(req) => (
        <div class="overlay">
          <div class="modal" onClick={(e) => e.stopPropagation()}>
            {/* Header */}
            <div class="flex items-center justify-between px-6 pt-6 pb-0">
              <div class="flex items-center gap-3">
                <div class="flex h-10 w-10 items-center justify-center rounded-xl" style={`background: var(--brand-50)`}>
                  <svg class="h-5 w-5" style={`color: var(--brand-500)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                  </svg>
                </div>
                <div>
                  <h3 class="text-base font-semibold" style={`color: var(--text-primary)`}>SSH Key Access Request</h3>
                  <p class="text-xs mt-0.5" style={`color: var(--text-tertiary)`}>An application is requesting access</p>
                </div>
              </div>
              <button
                class="btn-ghost"
                style={{ "border-radius": "var(--radius-md)", padding: "6px" }}
                onClick={props.onDismiss}
              >
                <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            {/* Details */}
            <div class="px-6 py-4">
              <div class="space-y-2.5 rounded-lg p-4" style={`background: var(--bg-secondary)`}>
                <div class="flex justify-between text-sm">
                  <span style={`color: var(--text-tertiary)`}>Key</span>
                  <span class="font-medium" style={`color: var(--text-primary)`}>{req().key_name}</span>
                </div>
                <div class="flex justify-between text-sm">
                  <span style={`color: var(--text-tertiary)`}>Fingerprint</span>
                  <span class="font-mono text-xs" style={`color: var(--text-secondary)`} title={req().key_fingerprint}>
                    {req().key_fingerprint.length > 24
                      ? req().key_fingerprint.slice(0, 20) + "..." + req().key_fingerprint.slice(-4)
                      : req().key_fingerprint}
                  </span>
                </div>
                <div class="flex justify-between items-start text-sm">
                  <span style={`color: var(--text-tertiary)`}>Process</span>
                  <div class="text-right">
                    <Show
                      when={req().process_chain.length > 0}
                      fallback={
                        <span class="font-medium" style={`color: var(--text-primary)`} title={req().client_exe}>
                          {extractExeName(req().client_exe)} (PID: {req().client_pid})
                        </span>
                      }
                    >
                      <div class="flex items-center gap-1 flex-wrap justify-end">
                        <For each={req().process_chain}>
                          {(proc, index) => (
                            <>
                              <Show when={index() > 0}>
                                <span class="text-xs" style={`color: var(--text-tertiary)`}>&rarr;</span>
                              </Show>
                              <span
                                class="font-medium text-sm"
                                style={`color: var(--text-primary)`}
                                title={`${proc.exe}\nPID: ${proc.pid}\n${proc.cmdline}`}
                              >
                                {extractExeName(proc.exe)}
                              </span>
                            </>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                </div>
                <Show when={req().process_chain.length > 0}>
                  <div class="flex justify-between items-start text-sm">
                    <span style={`color: var(--text-tertiary)`}>Target</span>
                    <span
                      class="font-mono text-xs truncate max-w-[220px]"
                      style={`color: var(--text-secondary)`}
                      title={req().process_chain[req().process_chain.length - 1].cmdline}
                    >
                      {req().process_chain[req().process_chain.length - 1].cmdline}
                    </span>
                  </div>
                </Show>
                <div class="flex justify-between text-sm">
                  <span style={`color: var(--text-tertiary)`}>Time</span>
                  <span style={`color: var(--text-secondary)`}>{formatTime(req().timestamp)}</span>
                </div>
              </div>
            </div>

            {/* Remember section */}
            <div class="px-6 py-3" style={`border-top: 1px solid var(--border-primary)`}>
              <button
                class="flex items-center gap-2 w-full text-left"
                onClick={() => setRememberOpen(!rememberOpen())}
              >
                <svg class="w-4 h-4 transition-transform" style={{
                  color: "var(--text-tertiary)",
                  transform: rememberOpen() ? "rotate(90deg)" : "rotate(0deg)"
                }} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M8.25 4.5l7.5 7.5-7.5 7.5" />
                </svg>
                <span class="text-xs font-medium" style={`color: var(--text-secondary)`}>
                  Remember this decision
                </span>
              </button>
              <Show when={rememberOpen()}>
                <div class="mt-3 space-y-3 pl-6">
                  {/* Duration selector */}
                  <div class="flex items-center gap-3">
                    <span class="text-xs shrink-0" style={`color: var(--text-tertiary); width: 70px`}>
                      Duration
                    </span>
                    <select
                      class="input text-xs"
                      style={{ padding: "4px 28px 4px 8px", width: "140px", appearance: "none",
                        "background-image": "url(\"data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e\")",
                        "background-position": "right 6px center", "background-repeat": "no-repeat", "background-size": "16px"
                      }}
                      value={duration()}
                      onChange={(e) => setDuration(parseInt(e.currentTarget.value))}
                    >
                      <option value={300}>5 minutes</option>
                      <option value={600}>10 minutes</option>
                      <option value={900}>15 minutes</option>
                      <option value={1800}>30 minutes</option>
                      <option value={3600}>1 hour</option>
                      <option value={7200}>2 hours</option>
                      <option value={14400}>4 hours</option>
                    </select>
                  </div>
                  {/* Scope selector */}
                  <div class="space-y-1.5">
                    <label class="flex items-center gap-2 cursor-pointer">
                      <input
                        type="radio" name="scope"
                        checked={scopeType() === "executable"}
                        onChange={() => setScopeType("executable")}
                        style={{ "accent-color": "var(--brand-500)" }}
                      />
                      <span class="text-xs" style={`color: var(--text-primary)`}>This program only</span>
                      <span class="text-xs" style={`color: var(--text-tertiary)`}>(recommended)</span>
                    </label>
                    <label class="flex items-center gap-2 cursor-pointer">
                      <input
                        type="radio" name="scope"
                        checked={scopeType() === "any_process"}
                        onChange={() => setScopeType("any_process")}
                        style={{ "accent-color": "var(--brand-500)" }}
                      />
                      <span class="text-xs" style={`color: var(--text-primary)`}>Any program</span>
                    </label>
                    <Show when={scopeType() === "any_process"}>
                      <div class="ml-5 text-xs rounded-md px-2.5 py-1.5" style={`background: var(--warning-bg); color: var(--warning-text)`}>
                        Any application on this computer can use this key during the session
                      </div>
                    </Show>
                  </div>
                </div>
              </Show>
            </div>

            {/* Actions */}
            <div class="flex gap-2.5 px-6 pb-6">
              <button
                class="btn btn-secondary flex-1"
                onClick={() => props.onRespond(req().id, false)}
              >
                Deny
              </button>
              <button
                class="btn btn-primary flex-1"
                onClick={() => props.onRespond(req().id, true)}
              >
                Approve Once
              </button>
              <Show when={rememberOpen()}>
                <button
                  class="btn flex-1"
                  style={{
                    background: "var(--brand-600)",
                    color: "white",
                  }}
                  onClick={() => {
                    const initiatorExe = req().process_chain.length > 0
                      ? req().process_chain[0].exe
                      : req().client_exe;
                    props.onRespondWithSession(
                      req().id,
                      duration(),
                      scopeType(),
                      scopeType() === "executable" ? initiatorExe : undefined,
                    );
                  }}
                >
                  Allow {formatDuration(duration())}
                </button>
              </Show>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
