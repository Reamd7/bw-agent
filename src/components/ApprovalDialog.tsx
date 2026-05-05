import { For, Show } from "solid-js";
import type { ApprovalRequest } from "../lib/tauri";

interface ApprovalDialogProps {
  request: ApprovalRequest | null;
  onRespond: (requestId: string, approved: boolean) => void;
  onDismiss: () => void;
}

export function ApprovalDialog(props: ApprovalDialogProps) {
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
                Approve
              </button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
