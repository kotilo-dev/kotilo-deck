import { describe, expect, it } from 'vitest';
import { describeError, fromEvent, installErrorHooks } from './index.ts';

const flush = () => new Promise((r) => setTimeout(r, 0));

function errorEvent(init: { message?: string; error?: unknown; filename?: string; lineno?: number; colno?: number }): Event {
	const e = new Event('error');
	return Object.assign(e, { message: '', filename: '', lineno: 0, colno: 0, ...init });
}

function rejection(reason: unknown): Event {
	return Object.assign(new Event('unhandledrejection'), { reason });
}

describe('describeError', () => {
	it('uses name, message and stack of an Error', () => {
		const err = new TypeError('x is undefined');
		const d = describeError(err);
		expect(d.message).toBe('TypeError: x is undefined');
		expect(d.stack).toContain('x is undefined');
	});

	it('handles strings, objects and nothing', () => {
		expect(describeError('boom')).toEqual({ message: 'boom', stack: null });
		expect(describeError({ code: 1 })).toEqual({ message: '{"code":1}', stack: null });
		expect(describeError(undefined, 'fallback')).toEqual({ message: 'fallback', stack: null });
	});

	it('clips long messages to 500 chars', () => {
		expect(describeError('a'.repeat(900)).message).toHaveLength(500);
	});
});

describe('fromEvent', () => {
	it('ignores resource load errors without a message', () => {
		expect(fromEvent(errorEvent({}))).toBeNull();
	});

	it('script errors without an Error object keep the location', () => {
		expect(fromEvent(errorEvent({ message: 'Script error.', filename: 'app.js', lineno: 3, colno: 7 }))?.message).toBe(
			'Script error. (app.js:3:7)'
		);
	});

	it('rejections use the reason', () => {
		expect(fromEvent(rejection(new Error('nope')))?.message).toBe('Error: nope');
	});
});

describe('installErrorHooks', () => {
	it('logs error and unhandledrejection events, and uninstalls', async () => {
		const target = new EventTarget();
		const calls: [string, string | null][] = [];
		const off = installErrorHooks(target, async (m, s) => void calls.push([m, s]));
		target.dispatchEvent(errorEvent({ message: 'Uncaught', error: new Error('one') }));
		target.dispatchEvent(rejection('two'));
		await flush();
		expect(calls.map((c) => c[0])).toEqual(['Error: one', 'two']);
		off();
		target.dispatchEvent(rejection('three'));
		await flush();
		expect(calls).toHaveLength(2);
	});

	it('swallows logger failures (sync throw and rejection) without re-reporting them', async () => {
		const target = new EventTarget();
		let n = 0;
		installErrorHooks(target, () => {
			n++;
			if (n === 1) throw new Error('sync');
			return Promise.reject(new Error('async'));
		});
		target.dispatchEvent(rejection('a'));
		target.dispatchEvent(rejection('b'));
		await flush();
		expect(n).toBe(2);
	});

	it('drops repeats of the same message and caps the rate', async () => {
		const target = new EventTarget();
		let clock = 0;
		const seen: string[] = [];
		installErrorHooks(target, async (m) => void seen.push(m), { now: () => clock, perMinute: 3, repeatMs: 1000 });
		target.dispatchEvent(rejection('same'));
		target.dispatchEvent(rejection('same'));
		clock = 1500;
		target.dispatchEvent(rejection('same'));
		target.dispatchEvent(rejection('other'));
		target.dispatchEvent(rejection('third'));
		await flush();
		expect(seen).toEqual(['same', 'same', 'other']);
		clock = 70_000;
		target.dispatchEvent(rejection('later'));
		await flush();
		expect(seen).toEqual(['same', 'same', 'other', 'later']);
	});
});
