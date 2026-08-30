import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ZoomLayer } from "./components/bits";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
    <ZoomLayer />
  </React.StrictMode>,
);
