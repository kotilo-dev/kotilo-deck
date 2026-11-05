import { describe, expect, it, vi } from 'vitest';
import { createDemoBackend, DEMO_MONITOR, demoSample } from './demo';
import type {
	CalendarUpcoming,
	CompanionSnapshot,
	DeckSettings,
	MediaState,
	PomodoroState,
	SystemSample,
	Todo,
	WeatherReport,
	Workspace
} from './index';

const NOW = new Date(2026, 10, 5, 10, 42).getTime();

function make(search = '?demo') {
	return createDemoBackend({ params: new URLSearchParams(search), now: () => NOW, expose: false });
}

describe('demo backend', () => {
	it('takes the theme and active workspace from the URL', async () => {
		const d = make('?demo&theme=nord&workspace=ops');
		const s = await d.invoke<DeckSettings>('settings_get');
		expect(s.theme).toBe('nord');
		expect(s.activeWorkspace).toEqual({ [DEMO_MONITOR]: 'ops' });
		expect(s.burnIn.shiftMinutes).toBe(0);
		expect(s.onboarding.completed).toBe(true);
		expect(s.calendarFeeds.length).toBeGreaterThan(0);
	});

	it('serves the four presets with a weather location', async () => {
		const ws = await make().invoke<Workspace[]>('workspace_list');
		expect(ws.map((w) => w.id)).toEqual(['focus', 'ops', 'agents', 'full']);
		const weather = ws[0].widgets.find((w) => w.widget === 'weather');
		expect(weather?.settings?.location).toMatchObject({ latitude: 41.01, longitude: 28.95 });
	});

	it('is deterministic for a fixed clock', async () => {
		const [a, b] = [make(), make()];
		for (const cmd of ['weather_get', 'calendar_upcoming', 'companion_snapshot', 'todo_list', 'pomodoro_state', 'media_state']) {
			expect(await a.invoke(cmd, { unit: 'c' })).toEqual(await b.invoke(cmd, { unit: 'c' }));
		}
		expect(demoSample(5, 0)).toEqual(demoSample(5, 0));
	});

	it('returns realistic data for the widgets', async () => {
		const d = make();
		const w = await d.invoke<WeatherReport>('weather_get', { unit: 'c' });
		expect(w.hourly).toHaveLength(6);
		expect(w.daily).toHaveLength(4);
		expect(w.daily[0].date).toBe('2026-11-05');
		const cal = await d.invoke<CalendarUpcoming>('calendar_upcoming', { limit: 20, days: 7 });
		expect(cal.events.some((e) => !e.allDay && e.start > NOW && e.start < NOW + 12 * 3600_000)).toBe(true);
		expect(cal.events.every((e) => e.end > NOW)).toBe(true);
		const snap = await d.invoke<CompanionSnapshot>('companion_snapshot');
		expect(snap.agents).toHaveLength(2);
		expect(snap.approvals).toHaveLength(1);
		const p = await d.invoke<PomodoroState>('pomodoro_state');
		expect(p).toMatchObject({ phase: 'focus', running: true, remaining: 14 * 60 + 32 });
		const m = await d.invoke<MediaState>('media_state');
		expect(m.session?.artwork).toMatch(/^data:image\/svg\+xml/);
		const sample = demoSample(0, NOW);
		expect(sample.cpu).toBeGreaterThan(0);
		expect(sample.cpu).toBeLessThan(100);
		expect(sample.perCore).toHaveLength(8);
	});

	it('writes mutate memory and emit the matching events', async () => {
		const d = make();
		const settings = vi.fn();
		const todos = vi.fn();
		await d.listen('settings://changed', settings);
		await d.listen('todo://changed', todos);
		await d.invoke('settings_set', { patch: { theme: 'dracula' } });
		expect(settings).toHaveBeenCalledTimes(1);
		expect((await d.invoke<DeckSettings>('settings_get')).theme).toBe('dracula');
		const added = await d.invoke<Todo>('todo_add', { text: 'New', due: null, remindAt: null });
		expect((await d.invoke<Todo[]>('todo_list')).at(-1)?.id).toBe(added.id);
		expect(todos).toHaveBeenCalled();
		// A fresh demo starts over.
		expect((await make().invoke<DeckSettings>('settings_get')).theme).toBeNull();
	});

	it('samples system://sample once a second while subscribed', async () => {
		vi.useFakeTimers();
		const intervals: (() => void)[] = [];
		const d = createDemoBackend({
			now: () => NOW,
			expose: false,
			setInterval: (fn) => intervals.push(fn),
			clearInterval: () => intervals.pop()
		});
		const got: SystemSample[] = [];
		await d.listen<SystemSample>('system://sample', (e) => got.push(e.payload));
		await d.invoke('system_subscribe');
		await vi.advanceTimersByTimeAsync(0);
		expect(got).toHaveLength(60); // backfill
		intervals[0]();
		expect(got).toHaveLength(61);
		await d.invoke('system_unsubscribe');
		expect(intervals).toHaveLength(0);
		vi.useRealTimers();
	});

	it('rejects unknown commands with a string', async () => {
		await expect(make().invoke('nope')).rejects.toBe('demo: unknown command nope');
	});
});
