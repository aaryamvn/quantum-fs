import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
// Bundled rather than fetched: the app must render fully offline, so Inter ships
// with the build instead of coming from a font CDN.
import "@fontsource-variable/inter";
import "./styles/globals.css";

const container = document.getElementById("root");
if (!container) throw new Error("QuantumFS: #root not found");

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
