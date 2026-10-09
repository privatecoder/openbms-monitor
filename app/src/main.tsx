import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/barlow/400.css";
import "@fontsource/barlow/500.css";
import "@fontsource/barlow/600.css";
import "@fontsource/barlow-semi-condensed/500.css";
import "@fontsource/barlow-semi-condensed/600.css";
import "./index.css";
import "./i18n";
import App from "./App";

const urlTheme = new URLSearchParams(location.search).get("theme");
const stored = urlTheme ?? (() => { try { return localStorage.getItem("theme"); } catch { return null; } })();
if (stored ? stored === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches) document.documentElement.classList.add("dark");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
