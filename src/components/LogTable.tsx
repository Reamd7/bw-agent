import { For, Show } from "solid-js";
import type { AccessLogEntry } from "../lib/tauri";

interface LogTableProps {
  logs: AccessLogEntry[];
}

export function LogTable(props: LogTableProps) {
  const formatTime = (timestamp: string) => {
    try {
      return new Date(timestamp).toLocaleString();
    } catch (e) {
      return timestamp;
    }
  };

  const extractExeName = (fullPath: string) => {
    // Handle both Windows and Unix paths
    const parts = fullPath.split(/[/\\]/);
    return parts[parts.length - 1] || fullPath;
  };

  return (
    <div class="overflow-x-auto rounded-lg border border-gray-200 shadow-sm">
      <table class="min-w-full divide-y divide-gray-200">
        <thead class="bg-gray-50">
          <tr>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Time
            </th>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Client
            </th>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Key
            </th>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Status
            </th>
          </tr>
        </thead>
        <tbody class="bg-white divide-y divide-gray-200">
          <Show
            when={props.logs.length > 0}
            fallback={
              <tr>
                <td colspan="4" class="px-6 py-4 text-center text-sm text-gray-500">
                  No access logs
                </td>
              </tr>
            }
          >
            <For each={props.logs}>
              {(log, index) => (
                <tr class={index() % 2 === 0 ? "bg-white" : "bg-gray-50"}>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {formatTime(log.timestamp)}
                  </td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                    <Show
                      when={log.process_chain && log.process_chain.length > 0}
                      fallback={<div title={log.client_exe}>{extractExeName(log.client_exe)}</div>}
                    >
                      <div class="flex items-center gap-1" title={log.process_chain.map(p => `${p.exe} (${p.cmdline})`).join('\n')}>
                        <For each={log.process_chain}>
                          {(proc, index) => (
                            <>
                              <Show when={index() > 0}>
                                <span class="text-gray-400 text-xs">→</span>
                              </Show>
                              <span>{extractExeName(proc.exe)}</span>
                            </>
                          )}
                        </For>
                      </div>
                    </Show>
                  </td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    <div title={log.key_fingerprint}>{log.key_name}</div>
                  </td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm">
                    <span
                      class={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
                        log.approved
                          ? "bg-green-100 text-green-800"
                          : "bg-red-100 text-red-800"
                      }`}
                    >
                      {log.approved ? "Approved" : "Denied"}
                    </span>
                  </td>
                </tr>
              )}
            </For>
          </Show>
        </tbody>
      </table>
    </div>
  );
}
