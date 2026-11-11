import { mountApp } from "./app";
import { checkForUpdates } from "./updater";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing #app root");
void mountApp(app);
void checkForUpdates();
