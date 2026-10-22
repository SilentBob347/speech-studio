import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import "./lib/i18n";
import App from "./App.tsx";
import TalkWidget from "./components/talk/TalkWidget";
import { startBridge } from "./lib/mcpBridge";
import { applyTheme, savedTheme } from "./lib/theme";
import { installMasterVolume } from "./lib/masterVolume";

applyTheme(savedTheme());
installMasterVolume();
// the widget is a second window of the same studio, not one the agent drives
const params = new URLSearchParams(window.location.search);
const widget = params.has("widget");
// a browser's widget popup asked to close itself; the desktop shell closes its window before the page loads
if (widget && params.has("close")) window.close();
if (widget) document.documentElement.classList.add("talk-widget");
else startBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {widget ? <TalkWidget /> : <App />}
  </StrictMode>,
);
