import { For, Show } from "solid-js";
import type { ApprovalRequest } from "../lib/tauri";

interface ApprovalDialogProps {
  request: ApprovalRequest | null;
  onRespond: (requestId: string, approved: boolean) => void;
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
        <div class="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto overflow-x-hidden bg-gray-900/50 p-4">
          <div class="relative w-full max-w-md rounded-xl bg-white p-6 shadow-2xl">
            <div class="mb-6 text-center">
              <div class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-blue-100">
                <svg
                  class="h-6 w-6 text-blue-600"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                  xmlns="http://www.w3.org/2000/svg"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z"
                  ></path>
                </svg>
              </div>
              <h3 class="text-xl font-bold text-gray-900">SSH Key Access Request</h3>
              <p class="mt-2 text-sm text-gray-500">
                An application is requesting access to your SSH key.
              </p>
            </div>

            <div class="mb-6 space-y-3 rounded-lg bg-gray-50 p-4 text-sm">
              <div class="flex justify-between">
                <span class="font-medium text-gray-500">Key Name:</span>
                <span class="font-semibold text-gray-900">{req().key_name}</span>
              </div>
              <div class="flex justify-between">
                <span class="font-medium text-gray-500">Fingerprint:</span>
                <span class="font-mono text-xs text-gray-900 truncate max-w-[200px]" title={req().key_fingerprint}>
                  {req().key_fingerprint}
                </span>
              </div>
              <div class="flex justify-between items-start">
                <span class="font-medium text-gray-500">Process:</span>
                <div class="text-right">
                  <Show
                    when={req().process_chain.length > 0}
                    fallback={
                      <span class="font-semibold text-gray-900" title={req().client_exe}>
                        {extractExeName(req().client_exe)} (PID: {req().client_pid})
                      </span>
                    }
                  >
                    <div class="flex items-center gap-1 flex-wrap justify-end">
                      <For each={req().process_chain}>
                        {(proc, index) => (
                          <>
                            <Show when={index() > 0}>
                              <span class="text-gray-400 text-xs">→</span>
                            </Show>
                            <span
                              class="font-semibold text-gray-900 cursor-default"
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
                <div class="flex justify-between items-start">
                  <span class="font-medium text-gray-500">Target:</span>
                  <span
                    class="font-mono text-xs text-gray-900 truncate max-w-[200px]"
                    title={req().process_chain[req().process_chain.length - 1].cmdline}
                  >
                    {req().process_chain[req().process_chain.length - 1].cmdline}
                  </span>
                </div>
              </Show>
              <div class="flex justify-between">
                <span class="font-medium text-gray-500">Time:</span>
                <span class="text-gray-900">{formatTime(req().timestamp)}</span>
              </div>
            </div>

            <div class="flex gap-3">
              <button
                type="button"
                class="flex-1 rounded-lg bg-red-600 px-4 py-2.5 text-center text-sm font-medium text-white hover:bg-red-700 focus:outline-none focus:ring-4 focus:ring-red-300"
                onClick={() => props.onRespond(req().id, false)}
              >
                Deny
              </button>
              <button
                type="button"
                class="flex-1 rounded-lg bg-green-600 px-4 py-2.5 text-center text-sm font-medium text-white hover:bg-green-700 focus:outline-none focus:ring-4 focus:ring-green-300"
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
