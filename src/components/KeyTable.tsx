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
    <Show
      when={props.keys.length > 0}
      fallback={
        <div class="card empty-state">
          <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
          </svg>
          <h3>No SSH keys found</h3>
          <p>Keys will appear after syncing your vault.</p>
        </div>
      }
    >
      <div class="space-y-2">
        <For each={props.keys}>
          {(key, index) => {
            const isExpanded = () => expandedIdx() === index();
            return (
              <div
                class="card cursor-pointer"
                style={{ padding: "14px 20px" }}
                onClick={() => toggle(index())}
              >
                {/* Summary row */}
                <div class="flex items-center gap-4">
                  {/* Key type icon */}
                  <div
                    class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                    style={`background: var(--brand-50)`}
                  >
                    <svg class="h-4.5 w-4.5" style={`color: var(--brand-500)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
                    </svg>
                  </div>

                  <div class="min-w-0 flex-1">
                    <div class="text-sm font-medium truncate" style={`color: var(--text-primary)`}>{key.name}</div>
                    <div class="text-xs mt-0.5 truncate font-mono" style={`color: var(--text-tertiary)`}>{key.fingerprint}</div>
                  </div>

                  <span class="badge badge-brand text-xs shrink-0">{key.key_type}</span>

                  {/* Expand chevron */}
                  <svg
                    class="w-4 h-4 shrink-0 transition-transform"
                    style={{
                      color: "var(--text-tertiary)",
                      transform: isExpanded() ? "rotate(180deg)" : "rotate(0deg)",
                    }}
                    fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"
                  >
                    <path stroke-linecap="round" stroke-linejoin="round" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
                  </svg>
                </div>

                {/* Expanded details */}
                <Show when={isExpanded()}>
                  <div class="mt-3 pt-3" style={`border-top: 1px solid var(--border-primary)`}>
                    <div class="space-y-2 text-sm">
                      <div class="flex items-center gap-2">
                        <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Name</span>
                        <span style={`color: var(--text-primary)`} class="break-all">{key.name}</span>
                      </div>
                      <div class="flex items-center gap-2">
                        <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Type</span>
                        <span style={`color: var(--text-primary)`}>{key.key_type}</span>
                      </div>
                      <div class="flex items-start gap-2">
                        <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Fingerprint</span>
                        <span class="font-mono text-xs break-all" style={`color: var(--text-primary)`}>{key.fingerprint}</span>
                      </div>
                      <Show when={key.match_patterns.length > 0}>
                        <div class="flex items-start gap-2">
                          <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Routing</span>
                          <div class="flex flex-wrap gap-1.5">
                            <For each={key.match_patterns}>
                              {(pattern) => (
                                <span class="badge badge-brand text-xs font-mono" style={{ padding: "2px 8px" }}>
                                  {pattern}
                                </span>
                              )}
                            </For>
                          </div>
                        </div>
                      </Show>
                    </div>
                  </div>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
    </Show>
  );
}
