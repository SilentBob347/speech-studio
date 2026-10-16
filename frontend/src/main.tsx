import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./lib/i18n";
import App from "./App.tsx";
import TalkWidget from "./components/talk/TalkWidget";
import { startBridge } from "./lib/mcpBridge";
import { applyTheme, savedTheme } from "./lib/theme";

applyTheme(savedTheme());
// the widget is a second window of the same studio, not one the agent drives
const widget = new URLSearchParams(window.location.search).has("widget");
if (widget) document.documentElement.classList.add("talk-widget");
else startBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {widget ? <TalkWidget /> : <App />}
  </StrictMode>,
);
