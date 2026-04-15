import { HashRouter } from "@solidjs/router";
import { lazy, onMount } from "solid-js";
import { initStoreListeners } from "./lib/store";

const routes = [
  { path: "/", component: lazy(() => import("./pages/LoginPage")) },
  { path: "/dashboard", component: lazy(() => import("./pages/DashboardPage")) },
  { path: "/settings", component: lazy(() => import("./pages/SettingsPage")) },
];

export default function App() {
  onMount(() => {
    initStoreListeners();
  });

  return <HashRouter>{routes}</HashRouter>;
}