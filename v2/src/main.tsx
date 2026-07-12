import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import RunnerWindow from "./screens/RunnerWindow";
import { initTheme } from "./lib/theme";
import "./index.css";

const queryClient = new QueryClient();
initTheme();

// The compact always-on-top runner opens as a second webview window on the
// same bundle, routed by hash (see RunTests -> openRunnerWindow).
const Root = window.location.hash === "#runner" ? RunnerWindow : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <Root />
    </QueryClientProvider>
  </React.StrictMode>,
);
