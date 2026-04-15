import { For, Show } from "solid-js";
import type { SshKeyInfo } from "../lib/tauri";

interface KeyTableProps {
  keys: SshKeyInfo[];
}

export function KeyTable(props: KeyTableProps) {
  return (
    <div class="overflow-x-auto rounded-lg border border-gray-200 shadow-sm">
      <table class="min-w-full divide-y divide-gray-200">
        <thead class="bg-gray-50">
          <tr>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Name
            </th>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Type
            </th>
            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
              Fingerprint
            </th>
          </tr>
        </thead>
        <tbody class="bg-white divide-y divide-gray-200">
          <Show
            when={props.keys.length > 0}
            fallback={
              <tr>
                <td colspan="3" class="px-6 py-4 text-center text-sm text-gray-500">
                  No SSH keys found
                </td>
              </tr>
            }
          >
            <For each={props.keys}>
              {(key, index) => (
                <tr class={index() % 2 === 0 ? "bg-white" : "bg-gray-50"}>
                  <td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                    {key.name}
                  </td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    {key.key_type}
                  </td>
                  <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                    <div class="font-mono truncate max-w-xs" title={key.fingerprint}>
                      {key.fingerprint}
                    </div>
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
