import { For, Show, createSignal } from "solid-js";
import type { SshKeyInfo } from "../lib/tauri";

interface KeyTableProps {
  keys: SshKeyInfo[];
}

export function KeyTable(props: KeyTableProps) {
  const [expandedIdx, setExpandedIdx] = createSignal<number | null>(null);

  const toggle = (idx: number) => {
    setExpandedIdx((prev) => (prev === idx ? null : idx));
  };

  return (
    <div class="overflow-hidden rounded-lg border border-gray-200 shadow-sm">
      <table class="w-full table-fixed divide-y divide-gray-200">
        <thead class="bg-gray-50">
          <tr>
            <th
              scope="col"
              class="w-[45%] px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
            >
              Name
            </th>
            <th
              scope="col"
              class="w-[15%] px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
            >
              Type
            </th>
            <th
              scope="col"
              class="w-[40%] px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
            >
              Fingerprint
            </th>
          </tr>
        </thead>
        <tbody class="bg-white divide-y divide-gray-200">
          <Show
            when={props.keys.length > 0}
            fallback={
              <tr>
                <td colspan="3" class="px-4 py-4 text-center text-sm text-gray-500">
                  No SSH keys found
                </td>
              </tr>
            }
          >
            <For each={props.keys}>
              {(key, index) => {
                const isExpanded = () => expandedIdx() === index();

                return (
                  <>
                    <tr
                      class={`cursor-pointer transition-colors hover:bg-blue-50 ${
                        index() % 2 === 0 ? "bg-white" : "bg-gray-50"
                      }`}
                      onClick={() => toggle(index())}
                    >
                      <td class="px-4 py-3 text-sm font-medium text-gray-900">
                        <div class="truncate">{key.name}</div>
                      </td>
                      <td class="px-4 py-3 text-sm text-gray-500">
                        {key.key_type}
                      </td>
                      <td class="px-4 py-3 text-sm text-gray-500">
                        <div class="font-mono truncate">{key.fingerprint}</div>
                      </td>
                    </tr>
                    <Show when={isExpanded()}>
                      <tr class="bg-gray-50">
                        <td colspan="3" class="px-4 py-3">
                          <div class="space-y-2 text-sm">
                            <div>
                              <span class="font-medium text-gray-500">Name: </span>
                              <span class="text-gray-900 break-all">{key.name}</span>
                            </div>
                            <div>
                              <span class="font-medium text-gray-500">Type: </span>
                              <span class="text-gray-900">{key.key_type}</span>
                            </div>
                            <div>
                              <span class="font-medium text-gray-500">Fingerprint: </span>
                              <span class="font-mono text-gray-900 break-all">{key.fingerprint}</span>
                            </div>
                          </div>
                        </td>
                      </tr>
                    </Show>
                  </>
                );
              }}
            </For>
          </Show>
        </tbody>
      </table>
    </div>
  );
}
