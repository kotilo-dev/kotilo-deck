/** Pure helpers for Settings ▸ Tablet & browser and the About ▸ Updates block (unit-tested). */
import type { UpdateState } from '#lib/ipc/index.ts';

export const LAN_DEFAULT_PORT = 47832;
/** protocol.md companion port; the LAN server may never take it. */
export const COMPANION_PORT = 47831;
export const PORT_MIN = 1024;
export const PORT_MAX = 65535;

export type PortProblem = 'number' | 'range' | 'companion';

/** null = valid. Accepts the raw text of the port field. */
export function portProblem(raw: string): PortProblem | null {
	const text = raw.trim();
	if (!/^\d+$/.test(text)) return 'number';
	const n = Number(text);
	if (n < PORT_MIN || n > PORT_MAX) return 'range';
	if (n === COMPANION_PORT) return 'companion';
	return null;
}

/** `http://10.0.0.5:47832/?token=abc` → `http://10.0.0.5:47832/?token=••••••••`. Non-URLs are fully masked. */
export function maskUrl(url: string): string {
	const dots = '••••••••';
	try {
		const u = new URL(url);
		if (!u.searchParams.has('token')) return url;
		return `${u.origin}${u.pathname}?token=${dots}`;
	} catch {
		return dots;
	}
}

/** UTF-8 safe base64 (btoa only takes Latin-1). */
export function base64Utf8(text: string): string {
	const bytes = new TextEncoder().encode(text);
	let bin = '';
	for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
	return btoa(bin);
}

/** `<img src>` for the backend's QR SVG markup; never injected as HTML. */
export const svgDataUrl = (svg: string): string => `data:image/svg+xml;base64,${base64Utf8(svg)}`;

/** Is the update button "Install" available in this state? */
export const canInstall = (state: UpdateState): boolean => state === 'available' || state === 'ready';
/** Is a check/install currently running (buttons disabled)? */
export const updateBusy = (state: UpdateState): boolean => state === 'checking' || state === 'downloading';
