import { For, Show, createSignal } from "solid-js";import type { SshKeyInfo } from "../lib/tauri";

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
                            <Show when={key.match_patterns.length > 0}>
                              <div class="mt-1 pt-2 border-t border-gray-200">
                                <div class="flex items-start">
                                  <span class="font-medium text-blue-600 shrink-0">密钥路由规则: </span>
                                  <span class="ml-1 text-gray-600 text-xs leading-5">根据 Git 仓库地址自动选择此密钥</span>
                                </div>
                                <div class="mt-1 flex flex-wrap gap-1.5">
                                  <For each={key.match_patterns}>
                                    {(pattern) => (
                                      <span class="inline-flex items-center rounded-md bg-blue-50 px-2.5 py-0.5 text-xs font-medium font-mono text-blue-700 ring-1 ring-inset ring-blue-600/20">
                                        {pattern}
                                      </span>
                                    )}
                                  </For>
                                </div>
                              </div>
                            </Show>
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
