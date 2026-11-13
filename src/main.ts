import "./app.css";
import { installConsoleInterceptor } from "./lib/utils/log-buffer.js";
import { installExternalLinkHandler } from "./lib/utils/openExternal.js";
import App from "./App.svelte";
import { mount } from "svelte";

// Install before mounting so startup logs are captured.
installConsoleInterceptor();
installExternalLinkHandler();

const app = mount(App, { target: document.getElementById("app")! });

export default app;
