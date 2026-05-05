import { Index, Show, createSignal } from "solid-js";
import { updateKeyFields } from "../lib/tauri";
import type { SshKeyInfo, CustomFieldInput, CustomFieldInfo } from "../lib/tauri";

interface KeyEditModalProps {
  keyInfo: SshKeyInfo | null;
  onClose: () => void;
  onSaved: (updatedFields: CustomFieldInfo[]) => void;
}

export function KeyEditModal(props: KeyEditModalProps) {
  return (
    <Show when={props.keyInfo}>
      {(keyInfo) => {
        const [fields, setFields] = createSignal<CustomFieldInput[]>(
          keyInfo().custom_fields.map((f) => ({ ...f }))
        );
        const [saving, setSaving] = createSignal(false);
        const [error, setError] = createSignal("");

        const updateField = (index: number, key: keyof CustomFieldInput, value: string | number) => {
          setFields((prev) => {
            const next = [...prev];
            next[index] = { ...next[index], [key]: value };
            // Auto-set boolean defaults
            if (key === "field_type" && value === 2 && next[index].value !== "true" && next[index].value !== "false") {
              next[index] = { ...next[index], value: "false" };
            }
            return next;
          });
        };

        const removeField = (index: number) => {
          setFields((prev) => {
            const next = [...prev];
            next.splice(index, 1);
            return next;
          });
        };

        const addField = () => {
          setFields((prev) => [...prev, { name: "", value: "", field_type: 0 }]);
        };

        const handleSave = async () => {
          setSaving(true);
          setError("");
          const optimisticFields: CustomFieldInfo[] = fields().map((f) => ({
            name: f.name,
            value: f.value,
            field_type: f.field_type,
          }));
          // Optimistic update + close immediately
          props.onSaved(optimisticFields);
          props.onClose();
          try {
            await updateKeyFields(keyInfo().entry_id, fields());
          } catch (e: any) {
            console.error("Background field update failed:", e);
          }
        };

        return (
          <div class="overlay">
            <div class="modal" style={{ "max-width": "540px" }} onClick={(e) => e.stopPropagation()}>
              {/* Header */}
              <div class="flex items-center justify-between px-6 pt-6 pb-0">
                <div class="flex items-center gap-3">
                  <div class="flex h-10 w-10 items-center justify-center rounded-xl" style={`background: var(--brand-50)`}>
                    <svg class="h-5 w-5" style={`color: var(--brand-500)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M16.862 4.487l1.687-1.688a1.875 1.875 0 112.652 2.652L10.582 16.07a4.5 4.5 0 01-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 011.13-1.897l8.932-8.931zm0 0L19.5 7.125M18 14v4.75A2.25 2.25 0 0115.75 21H5.25A2.25 2.25 0 013 18.75V8.25A2.25 2.25 0 015.25 6H10" />
                    </svg>
                  </div>
                  <div>
                    <h3 class="text-base font-semibold" style={`color: var(--text-primary)`}>Edit Custom Fields</h3>
                    <p class="text-xs mt-0.5 truncate" style={{ color: "var(--text-tertiary)", "max-width": "280px" }}>{keyInfo().name}</p>
                  </div>
                </div>
                <button class="btn-ghost" style={{ "border-radius": "var(--radius-md)", padding: "6px" }} onClick={props.onClose}>
                  <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>

              {/* Content */}
              <div class="px-6 py-4 space-y-3" style={{ "max-height": "400px", "overflow-y": "auto" }}>
                <Index each={fields()}>
                  {(field, idx) => {
                    const ft = () => field().field_type;
                    const fname = () => field().name;
                    const fvalue = () => field().value;

                    return (
                      <div class="rounded-lg p-3" style={`background: var(--bg-secondary)`}>
                        <div class="flex items-center gap-2 mb-2">
                          <select
                            class="input text-xs"
                            style={{ width: "90px", padding: "4px 8px" }}
                            value={ft()}
                            onChange={(e) => updateField(idx, "field_type", parseInt(e.currentTarget.value))}
                          >
                            <option value={0}>Text</option>
                            <option value={1}>Hidden</option>
                            <option value={2}>Boolean</option>
                          </select>
                          <button
                            class="btn-ghost"
                            style={{
                              padding: "4px",
                              "border-radius": "var(--radius-sm)",
                              color: "var(--danger)",
                              "margin-left": "auto",
                            }}
                            onClick={() => removeField(idx)}
                          >
                            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                            </svg>
                          </button>
                        </div>
                        <div class="flex gap-2">
                          <div style={{ width: "120px", "flex-shrink": "0" }}>
                            <label class="text-xs font-medium mb-1.5 block" style={`color: var(--text-tertiary)`}>Name</label>
                            <input
                              class="input text-sm"
                              style={{ width: "100%", padding: "6px 10px" }}
                              value={fname()}
                              placeholder="e.g. git-sign"
                              onInput={(e) => updateField(idx, "name", e.currentTarget.value)}
                            />
                          </div>
                          <div class="flex-1 min-w-0">
                            <label class="text-xs font-medium mb-1.5 block" style={`color: var(--text-tertiary)`}>Value</label>
                            <Show
                              when={ft() === 2}
                              fallback={
                                <input
                                  class="input text-sm"
                                  style={{ width: "100%", padding: "6px 10px" }}
                                  type={ft() === 1 ? "password" : "text"}
                                  value={fvalue()}
                                  placeholder="value"
                                  onInput={(e) => updateField(idx, "value", e.currentTarget.value)}
                                />
                              }
                            >
                              <select
                                class="input text-sm"
                                style={{ width: "100%", padding: "6px 10px" }}
                                value={fvalue()}
                                onChange={(e) => updateField(idx, "value", e.currentTarget.value)}
                              >
                                <option value="true">true</option>
                                <option value="false">false</option>
                              </select>
                            </Show>
                          </div>
                        </div>
                      </div>
                    );
                  }}
                </Index>
                <Show when={fields().length === 0}>
                  <div class="text-xs italic py-2" style="color: var(--text-tertiary)">No custom fields yet.</div>
                </Show>
              </div>

              <div class="px-6">
                <button
                  class="btn btn-secondary text-xs w-full"
                  onClick={addField}
                >
                  + Add Field
                </button>
              </div>

              <Show when={error()}>
                <div class="mx-6 text-xs rounded-md p-2.5" style="background: var(--danger-bg); color: var(--danger);">
                  {error()}
                </div>
              </Show>

              {/* Actions */}
              <div class="flex gap-2.5 px-6 pb-6 pt-2">
                <button class="btn btn-secondary flex-1" onClick={props.onClose} disabled={saving()}>
                  Cancel
                </button>
                <button class="btn btn-primary flex-1" onClick={handleSave} disabled={saving()}>
                  {saving() ? "Saving..." : "Save Fields"}
                </button>
              </div>
            </div>
          </div>
        );
      }}
    </Show>
  );
}
