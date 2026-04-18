import { createStore } from "solid-js/store";
import { listen } from "@tauri-apps/api/event";
import type { ApprovalRequest } from "./tauri";

interface AppStore {
  locked: boolean;
  pendingApprovals: ApprovalRequest[];
  email: string;
  isSetupComplete: boolean;
}

export const [store, setStore] = createStore<AppStore>({
  locked: true,
  pendingApprovals: [],
  email: "",
  isSetupComplete: true,
});

// Initialize event listeners
export function initStoreListeners() {
  listen<{ locked: boolean }>("lock-state-changed", (event) => {
    setStore("locked", event.payload.locked);
  });

  listen<ApprovalRequest>("approval-requested", (event) => {
    setStore("pendingApprovals", (prev) => {
      // Avoid duplicates
      if (prev.some((req) => req.id === event.payload.id)) return prev;
      return [...prev, event.payload];
    });
  });
}
