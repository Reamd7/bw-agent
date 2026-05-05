import { For, Index, Show, createSignal, onMount } from "solid-js";
import type { SshKeyInfo, CustomFieldInput } from "../lib/tauri";
import { getGitSignProgramPath, updateKeyFields } from "../lib/tauri";
import { KeyEditModal } from "./KeyEditModal";

const isWindows = () =>
  navigator.platform.toLowerCase().includes("win") ||
  navigator.userAgent.toLowerCase().includes("windows");

const generateGitConfigCommands = (
  signProgramPath: string,
  publicKey: string,
): string => {
  const signPath = isWindows()
    ? signProgramPath.replace(/\\/g, "/")
    : signProgramPath;
  return [
    `git config commit.gpgsign true`,
    `git config tag.gpgsign true`,
    `git config gpg.format ssh`,
    `git config gpg.ssh.program "${signPath}"`,
    `git config user.signingkey "${publicKey}"`,
  ].join("\n");
};

/** Convert CustomFieldInput[] to optimistic CustomFieldInfo[] for immediate UI */
const toOptimisticFields = (fields: CustomFieldInput[]): CustomFieldInfo[] =>
  fields.map((f) => ({ name: f.name, value: f.value, field_type: f.field_type }));

/** Derive field operations that go through updateKeyFields */
const toggleGitSign = (key: SshKeyInfo) => {
  const gitSignIdx = key.custom_fields.findIndex(
    (f) => f.name === "git-sign",
  );
  let fields: CustomFieldInput[];
  if (gitSignIdx >= 0) {
    // Toggle the existing value
    fields = key.custom_fields.map((f, i) =>
      i === gitSignIdx
        ? { ...f, value: f.value === "true" ? "false" : "true" }
        : { name: f.name, value: f.value, field_type: f.field_type },
    );
  } else {
    // Add git-sign field
    fields = [
      ...key.custom_fields.map((f) => ({
        name: f.name,
        value: f.value,
        field_type: f.field_type,
      })),
      { name: "git-sign", value: "true", field_type: 2 },
    ];
  }
  return fields;
};

const addMatchPattern = (key: SshKeyInfo, pattern: string) => {
  const fields: CustomFieldInput[] = [
    ...key.custom_fields.map((f) => ({
      name: f.name,
      value: f.value,
      field_type: f.field_type,
    })),
    { name: "gh-match", value: pattern, field_type: 0 },
  ];
  return fields;
};

const removeMatchPattern = (key: SshKeyInfo, patternIndex: number) => {
  // Count only gh-match fields to find the right one
  let matchCount = 0;
  const fields: CustomFieldInput[] = [];
  for (const f of key.custom_fields) {
    if (f.name === "gh-match") {
      if (matchCount === patternIndex) {
        matchCount++;
        continue; // skip this one
      }
      matchCount++;
    }
    fields.push({ name: f.name, value: f.value, field_type: f.field_type });
  }
  return fields;
};

interface KeyTableProps {
  keys: SshKeyInfo[];
  onRefresh?: () => void;
  onKeyUpdated?: (entryId: string, updatedFields: CustomFieldInfo[]) => void;
}

export function KeyTable(props: KeyTableProps) {
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  const [signProgramPath, setSignProgramPath] = createSignal<string>("");
  const [editingKey, setEditingKey] = createSignal<SshKeyInfo | null>(null);

  onMount(async () => {
    try {
      const path = await getGitSignProgramPath();
      setSignProgramPath(path);
    } catch (e) {
      console.error("Failed to get git sign program path:", e);
    }
  });

  const toggle = (entryId: string) => {
    setExpandedId((prev) => (prev === entryId ? null : entryId));
  };

  const handleFieldAction = (
    entryId: string,
    optimisticFields: CustomFieldInfo[],
    action: () => Promise<void>,
  ) => {
    // 1. Optimistic update — instant UI
    props.onKeyUpdated?.(entryId, optimisticFields);
    // 2. Background sync — no UI blocking
    action().catch((e) => {
      console.error("Field update failed, reverting:", e);
      // Revert on failure — full refresh to get authoritative state
      props.onRefresh?.();
    });
  };

  return (
    <>
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
          <Index each={props.keys}>
            {(key, idx) => {
              const isExpanded = () => expandedId() === key().entry_id;
              const [copied, setCopied] = createSignal(false);
              const [newPattern, setNewPattern] = createSignal("");
              const [patternError, setPatternError] = createSignal("");

              const hasGitSign = () =>
                key().custom_fields.some(
                  (f) => f.name === "git-sign" && f.value === "true",
                );

              const otherFields = () =>
                key().custom_fields.filter(
                  (f) => f.name !== "git-sign" && f.name !== "gh-match",
                );

              const handleCopyGitConfig = async () => {
                const progPath = signProgramPath();
                if (!progPath) return;
                try {
                  const commands = generateGitConfigCommands(
                    progPath,
                    key().public_key,
                  );
                  await navigator.clipboard.writeText(commands);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                } catch (err) {
                  console.error("Failed to copy:", err);
                }
              };

              const handleAddPattern = () => {
                const k = key();
                const p = newPattern().trim();
                if (!p) return;
                if (k.match_patterns.includes(p)) {
                  setPatternError("Already exists");
                  return;
                }
                setPatternError("");
                handleFieldAction(
                  k.entry_id,
                  toOptimisticFields(addMatchPattern(k, p)),
                  () => updateKeyFields(k.entry_id, addMatchPattern(k, p)).then(() => setNewPattern("")),
                );
              };

              return (
                <div class="card" style={{ padding: "14px 20px" }}>
                  {/* Summary row */}
                  <div
                    class="flex items-center gap-4 cursor-pointer"
                    onClick={() => toggle(key().entry_id)}
                  >
                    <div
                      class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                      style={`background: var(--brand-50)`}
                    >
                      <svg
                        class="h-4.5 w-4.5"
                        style={`color: var(--brand-500)`}
                        fill="none"
                        viewBox="0 0 24 24"
                        stroke="currentColor"
                        stroke-width="1.5"
                      >
                        <path
                          stroke-linecap="round"
                          stroke-linejoin="round"
                          d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z"
                        />
                      </svg>
                    </div>

                    <div class="min-w-0 flex-1">
                      <div
                        class="text-sm font-medium truncate"
                        style={`color: var(--text-primary)`}
                      >
                        {key().name}
                      </div>
                      <div
                        class="text-xs mt-0.5 truncate font-mono"
                        style={`color: var(--text-tertiary)`}
                      >
                        {key().fingerprint}
                      </div>
                    </div>

                    <span class="badge badge-brand text-xs shrink-0">
                      {key().key_type}
                    </span>

                    <Show when={hasGitSign()}>
                      <span class="badge badge-success text-xs shrink-0 flex items-center gap-1">
                        <svg
                          class="w-3 h-3"
                          fill="none"
                          viewBox="0 0 24 24"
                          stroke="currentColor"
                          stroke-width="2"
                        >
                          <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            d="M5 13l4 4L19 7"
                          />
                        </svg>
                        sign
                      </span>
                    </Show>

                    <Show when={key().match_patterns.length > 0}>
                      <span class="badge text-xs shrink-0 flex items-center gap-1" style="background: var(--bg-tertiary); color: var(--text-secondary)">
                        {key().match_patterns.length} route{key().match_patterns.length > 1 ? "s" : ""}
                      </span>
                    </Show>

                    {/* Expand chevron */}
                    <svg
                      class="w-4 h-4 shrink-0 transition-transform"
                      style={{
                        color: "var(--text-tertiary)",
                        transform: isExpanded()
                          ? "rotate(180deg)"
                          : "rotate(0deg)",
                      }}
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      stroke-width="2"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        d="M19.5 8.25l-7.5 7.5-7.5-7.5"
                      />
                    </svg>
                  </div>

                  {/* Expanded details */}
                  <Show when={isExpanded()}>
                    <div
                      class="mt-3 pt-3"
                      style={`border-top: 1px solid var(--border-primary)`}
                    >
                      <div class="space-y-3 text-sm">
                        {/* Basic info */}
                        <div class="flex items-center gap-2">
                          <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Name</span>
                          <span style={`color: var(--text-primary)`} class="break-all">{key().name}</span>
                        </div>
                        <div class="flex items-center gap-2">
                          <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Type</span>
                          <span style={`color: var(--text-primary)`}>{key().key_type}</span>
                        </div>
                        <div class="flex items-start gap-2">
                          <span class="text-xs font-medium shrink-0" style={`color: var(--text-tertiary); width: 90px`}>Fingerprint</span>
                          <span class="font-mono text-xs break-all" style={`color: var(--text-primary)`}>{key().fingerprint}</span>
                        </div>

                        {/* Git Signing — toggle switch */}
                        <div class="flex items-center justify-between">
                          <div class="flex items-center gap-2">
                            <span
                              class="text-xs font-medium"
                              style={`color: var(--text-tertiary); width: 90px`}
                            >
                              Git Signing
                            </span>
                            <span
                              class="text-xs"
                              style={`color: var(--text-secondary)`}
                            >
                              Use this key for commit signing
                            </span>
                          </div>
                          <div class="flex items-center gap-2">
                            <Show when={hasGitSign() && signProgramPath().length > 0}>
                              <button
                                class="text-xs flex items-center gap-1.5 px-2 py-1 rounded-md transition-colors"
                                style={{
                                  color: copied() ? "var(--success)" : "var(--brand-500)",
                                  background: copied() ? "var(--success-bg)" : "var(--brand-50)",
                                  cursor: "pointer",
                                }}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleCopyGitConfig();
                                }}
                              >
                                <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                                  <Show
                                    when={copied()}
                                    fallback={
                                      <path stroke-linecap="round" stroke-linejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
                                    }
                                  >
                                    <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                                  </Show>
                                </svg>
                                {copied() ? "Copied!" : "Copy git config"}
                              </button>
                            </Show>
                            <button
                              role="switch"
                              aria-checked={hasGitSign()}
                              onClick={(e) => {
                                e.stopPropagation();
                                const k = key();
                                handleFieldAction(
                                  k.entry_id,
                                  toOptimisticFields(toggleGitSign(k)),
                                  () => updateKeyFields(k.entry_id, toggleGitSign(k)),
                                );
                              }}
                              class="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full transition-colors"
                              style={{
                                "background-color": hasGitSign()
                                  ? "var(--brand-500)"
                                  : "var(--border-primary)",
                              }}
                            >
                              <span
                                class="inline-block h-4 w-4 rounded-full bg-white transition-transform"
                                style={{
                                  "margin-top": "2px",
                                  transform: hasGitSign()
                                    ? "translateX(18px)"
                                    : "translateX(2px)",
                                }}
                              />
                            </button>
                          </div>
                        </div>

                        {/* Routing — inline editable pattern list */}
                        <div class="flex items-start gap-2">
                          <span
                            class="text-xs font-medium shrink-0"
                            style={`color: var(--text-tertiary); width: 90px`}
                          >
                            Routing
                          </span>
                          <div class="flex-1 min-w-0">
                            <Show
                              when={key().match_patterns.length > 0}
                              fallback={
                                <span
                                  class="text-xs"
                                  style="color: var(--text-tertiary)"
                                >
                                  No routing rules — matches all repos
                                </span>
                              }
                            >
                              <div class="flex flex-wrap gap-1.5">
                                <Index each={key().match_patterns}>
                                  {(pattern, pIdx) => (
                                    <span
                                      class="badge badge-brand text-xs font-mono flex items-center gap-1"
                                      style={{ padding: "2px 8px" }}
                                    >
                                      {pattern()}
                                      <button
                                        class="ml-0.5 hover:opacity-70 transition-opacity"
                                        style={{ cursor: "pointer" }}
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          const k = key();
                                          handleFieldAction(
                                            k.entry_id,
                                            toOptimisticFields(removeMatchPattern(k, pIdx)),
                                            () => updateKeyFields(k.entry_id, removeMatchPattern(k, pIdx)),
                                          );
                                        }}
                                      >
                                        <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                                          <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                                        </svg>
                                      </button>
                                    </span>
                                  )}
                                </Index>
                              </div>
                            </Show>
                            <div class="flex items-center gap-1.5 mt-2">
                              <input
                                class="input text-xs font-mono"
                                style={{ padding: "4px 8px", width: "200px" }}
                                placeholder="e.g. github.com/org/*"
                                value={newPattern()}
                                onInput={(e) => {
                                  setNewPattern(e.currentTarget.value);
                                  setPatternError("");
                                }}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") {
                                    e.preventDefault();
                                    handleAddPattern();
                                  }
                                }}
                                onClick={(e) => e.stopPropagation()}
                              />
                              <button
                                class="text-xs px-2 py-1 rounded-md transition-colors"
                                disabled={!newPattern().trim()}
                                style={{
                                  color: "var(--brand-500)",
                                  background: "var(--brand-50)",
                                  cursor:
                                    !newPattern().trim()
                                      ? "not-allowed"
                                      : "pointer",
                                  opacity:
                                    !newPattern().trim() ? "0.5" : "1",
                                }}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleAddPattern();
                                }}
                              >
                                Add
                              </button>
                              <Show when={patternError()}>
                                <span class="text-xs" style="color: var(--danger)">
                                  {patternError()}
                                </span>
                              </Show>
                            </div>
                          </div>
                        </div>

                        {/* Other custom fields — compact read-only list */}
                        <Show when={otherFields().length > 0}>
                          <div class="flex items-start gap-2">
                            <span
                              class="text-xs font-medium shrink-0"
                              style={`color: var(--text-tertiary); width: 90px`}
                            >
                              Fields
                            </span>
                            <div class="flex flex-col gap-1">
                              <For each={otherFields()}>
                                {(field) => (
                                  <div class="flex items-center gap-2 text-xs">
                                    <span
                                      class="font-medium"
                                      style="color: var(--text-secondary)"
                                    >
                                      {field.name}
                                    </span>
                                    <span style="color: var(--text-tertiary)">
                                      =
                                    </span>
                                    <span
                                      class="font-mono"
                                      style="color: var(--text-primary)"
                                    >
                                      {field.field_type === 1
                                        ? "********"
                                        : field.field_type === 2
                                          ? field.value === "true"
                                            ? "yes"
                                            : "no"
                                          : field.value}
                                    </span>
                                  </div>
                                )}
                              </For>
                            </div>
                          </div>
                        </Show>

                        {/* Advanced edit button */}
                        <div class="flex items-center gap-2 pt-1">
                          <span class="shrink-0" style={`width: 90px`}></span>
                          <button
                            class="text-xs flex items-center gap-1 px-2 py-1 rounded-md transition-colors hover:opacity-70"
                            style={{
                              color: "var(--text-tertiary)",
                              cursor: "pointer",
                            }}
                            onClick={(e) => {
                              e.stopPropagation();
                              setEditingKey(key());
                            }}
                          >
                            <svg
                              class="h-3 w-3"
                              fill="none"
                              viewBox="0 0 24 24"
                              stroke="currentColor"
                              stroke-width="2"
                            >
                              <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 010 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 010-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28z"
                              />
                              <path
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
                              />
                            </svg>
                            Advanced
                          </button>
                        </div>
                      </div>
                    </div>
                  </Show>
                </div>
              );
            }}
          </Index>
        </div>
      </Show>

      <KeyEditModal
        keyInfo={editingKey()}
        onClose={() => setEditingKey(null)}
        onSaved={(updatedFields) => {
          const key = editingKey();
          if (key) props.onKeyUpdated?.(key.entry_id, updatedFields);
        }}
      />
    </>
  );
}
