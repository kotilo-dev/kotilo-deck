/**
 * Text file save/open. Inside Tauri the backend shows native dialogs (`file_save_as` / `file_open_text`;
 * WebView2 blob downloads are unreliable). In a plain browser: Blob download and a hidden file input.
 */
import { fileOpenText, fileSaveAs, isTauri } from '#lib/ipc/index.ts';

/** Resolves false when the user cancelled. */
export async function saveTextFile(defaultName: string, contents: string): Promise<boolean> {
	if (isTauri()) return fileSaveAs(defaultName, contents);
	const url = URL.createObjectURL(new Blob([contents], { type: 'application/json' }));
	const a = document.createElement('a');
	a.href = url;
	a.download = defaultName;
	document.body.append(a);
	a.click();
	a.remove();
	setTimeout(() => URL.revokeObjectURL(url), 1000);
	return true;
}

/** UTF-8 contents of a picked file, or null when cancelled. `extensions` without dots, e.g. ["json"]. */
export async function openTextFile(extensions: string[]): Promise<string | null> {
	if (isTauri()) return fileOpenText(extensions);
	return new Promise((resolve) => {
		const input = document.createElement('input');
		input.type = 'file';
		input.accept = extensions.map((e) => `.${e}`).join(',');
		input.style.display = 'none';
		let done = false;
		const finish = (v: string | null) => {
			if (done) return;
			done = true;
			input.remove();
			resolve(v);
		};
		input.addEventListener('change', async () => {
			const file = input.files?.[0];
			finish(file ? await file.text() : null);
		});
		input.addEventListener('cancel', () => finish(null));
		document.body.append(input);
		input.click();
	});
}
