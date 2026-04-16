import { HashRouter, Route, Navigate } from "@solidjs/router";
import { lazy, onMount, Show } from "solid-js";
import { initStoreListeners, store } from "./lib/store";

const LoginPage = lazy(() => import("./pages/LoginPage"));
const DashboardPage = lazy(() => import("./pages/DashboardPage"));
const SettingsPage = lazy(() => import("./pages/SettingsPage"));

function Protected(props: { children: any }) {
  return (
    <Show when={!store.locked} fallback={<Navigate href="/" />}>
      {props.children}
    </Show>
  );
}

export default function App() {
  onMount(() => {
    initStoreListeners();
  });

  return (
    <HashRouter>
      <Route path="/" component={LoginPage} />
      <Route path="/dashboard" component={() => <Protected><DashboardPage /></Protected>} />
      <Route path="/settings" component={SettingsPage} />
    </HashRouter>
  );
}
