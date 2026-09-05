import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ZoomLayer } from "./components/bits";
import { installErrorReporting } from "./errorReport";

// Before the tree mounts, so a failure during the first render is caught too.
installErrorReporting();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
    <ZoomLayer />
  </React.StrictMode>,
);
