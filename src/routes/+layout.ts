// Desktop app: no SSR, everything renders in the Tauri webview (or a LAN tablet/browser, Phase 4).
import { init } from '#lib/ipc/transport.ts';

export const ssr = false;
export const prerender = true;

/** Decide tauri / remote / local once before any page mounts: `hasBackend()` and `isRemote()` are sync after this. */
export const load = async () => {
	await init();
	return {};
};
