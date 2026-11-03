<script lang="ts">
	/**
	 * Workspace auto-switch rules (SPEC §4.1): trigger → target workspace, enable toggle, add / delete.
	 * The deck evaluates them (`#lib/workspace/autoswitch.ts`); this only edits `settings.autoSwitch`.
	 */
	import type { AutoSwitchRule, AutoSwitchTrigger } from '#lib/ipc/index.ts';
	import { locale, t } from '#lib/i18n/index.ts';
	import { parseAt, ruleId } from '#lib/workspace/index.ts';
	import Icon from '#lib/ui/Icon.svelte';
	import { getSettingsContext } from './context.svelte';
	import Switch from './Switch.svelte';

	const { store, ui, save } = getSettingsContext();
	const rules = $derived(store.settings.autoSwitch);
	const feeds = $derived(store.settings.calendarFeeds);

	type TriggerType = AutoSwitchTrigger['type'];
	const TYPES: TriggerType[] = ['time', 'pomodoro-start', 'pomodoro-end', 'calendar-start'];
	/** Monday first; values are `Date.getDay()` (0 = Sunday). */
	const WEEK = [1, 2, 3, 4, 5, 6, 0];
	const WEEKDAYS = [1, 2, 3, 4, 5];
	// 2026-11-01 is a Sunday, so day n of that week has getDay() === n.
	const dayName = (d: number, style: 'short' | 'long') =>
		new Intl.DateTimeFormat(locale, { weekday: style }).format(new Date(2026, 10, 1 + d));

	const defaultTrigger = (type: TriggerType): AutoSwitchTrigger =>
		type === 'time'
			? { type, at: '09:00', days: [...WEEKDAYS] }
			: type === 'calendar-start'
				? { type, feedId: null }
				: { type };

	const write = (next: AutoSwitchRule[]) => void save({ autoSwitch: next });
	const update = (id: string, patch: Partial<AutoSwitchRule>) => write(rules.map((r) => (r.id === id ? { ...r, ...patch } : r)));

	function add() {
		const target = store.active?.id ?? store.workspaces[0]?.id ?? '';
		const rule: AutoSwitchRule = { id: ruleId(rules.map((r) => r.id)), enabled: true, trigger: defaultTrigger('time'), workspace: target };
		write([...rules, rule]);
		requestAnimationFrame(() => document.getElementById(`k-rule-${rule.id}-type`)?.focus());
	}

	function remove(rule: AutoSwitchRule) {
		const index = rules.findIndex((r) => r.id === rule.id);
		write(rules.filter((r) => r.id !== rule.id));
		ui.say(t('settings.rules.removed'), 'info', 5000, {
			label: t('shell.edit.undo'),
			run: () => {
				if (rules.some((r) => r.id === rule.id)) return;
				const next = [...rules];
				next.splice(Math.min(index, next.length), 0, rule);
				write(next);
			}
		});
	}

	function setTime(rule: AutoSwitchRule, at: string) {
		if (rule.trigger.type !== 'time' || parseAt(at) === null) return;
		update(rule.id, { trigger: { ...rule.trigger, at } });
	}

	/** `days: []` means every day; the UI shows that as all seven pressed. The last day can't be turned off. */
	function toggleDay(rule: AutoSwitchRule, day: number) {
		if (rule.trigger.type !== 'time') return;
		const current = new Set(rule.trigger.days.length ? rule.trigger.days : WEEK);
		if (current.has(day)) {
			if (current.size === 1) return;
			current.delete(day);
		} else current.add(day);
		const days = current.size === 7 ? [] : WEEK.filter((d) => current.has(d));
		update(rule.id, { trigger: { ...rule.trigger, days } });
	}

	const dayOn = (rule: AutoSwitchRule, day: number) =>
		rule.trigger.type === 'time' && (rule.trigger.days.length === 0 || rule.trigger.days.includes(day));

	function problem(rule: AutoSwitchRule): string | null {
		if (!store.workspaces.some((w) => w.id === rule.workspace)) return t('settings.rules.missingWorkspace');
		if (rule.trigger.type === 'calendar-start' && rule.trigger.feedId !== null && !feeds.some((f) => f.id === (rule.trigger as { feedId: string }).feedId))
			return t('settings.rules.missingFeed');
		return null;
	}
</script>

<div class="s-group">
	<div class="s-inline between">
		<h2 class="s-eyebrow">{t('settings.rules.title', { count: rules.length })}</h2>
		<button type="button" class="k-btn s-btn-sm" onclick={add} disabled={store.workspaces.length === 0}>
			<Icon name="plus" size={16} stroke={2.4} />{t('settings.rules.add')}
		</button>
	</div>

	{#if rules.length === 0}
		<p class="s-card s-note">{t('settings.rules.empty')}</p>
	{:else}
		<ul class="s-list">
			{#each rules as rule, i (rule.id)}
				{@const issue = problem(rule)}
				{@const tr = rule.trigger}
				<li class="s-card rule" aria-label={t('settings.rules.ruleN', { n: i + 1 })}>
					<div class="line">
						<Switch label={t('settings.rules.enableN', { n: i + 1 })} checked={rule.enabled} onchange={(v) => update(rule.id, { enabled: v })} />
						<label class="field">
							<span class="k-sr-only">{t('settings.rules.when')}</span>
							<select
								id="k-rule-{rule.id}-type"
								class="k-select"
								value={tr.type}
								onchange={(e) => update(rule.id, { trigger: defaultTrigger(e.currentTarget.value as TriggerType) })}
							>
								{#each TYPES as type (type)}<option value={type}>{t(`settings.rules.types.${type}`)}</option>{/each}
							</select>
						</label>
						<span class="arrow" aria-hidden="true">→</span>
						<label class="field">
							<span class="k-sr-only">{t('settings.rules.target')}</span>
							<select class="k-select" value={rule.workspace} onchange={(e) => update(rule.id, { workspace: e.currentTarget.value })}>
								{#if !store.workspaces.some((w) => w.id === rule.workspace)}
									<option value={rule.workspace}>{t('settings.rules.missingOption', { id: rule.workspace })}</option>
								{/if}
								{#each store.workspaces as ws (ws.id)}<option value={ws.id}>{ws.name}</option>{/each}
							</select>
						</label>
						<button type="button" class="k-btn k-btn-icon danger" aria-label={t('settings.rules.deleteN', { n: i + 1 })} title={t('settings.rules.delete')} onclick={() => remove(rule)}>
							<Icon name="trash" size={16} />
						</button>
					</div>

					{#if tr.type === 'time'}
						<div class="line detail" class:s-disabled={!rule.enabled}>
							<label class="s-inline">
								<span class="s-help">{t('settings.rules.at')}</span>
								<input type="time" class="k-input s-time" value={tr.at} onchange={(e) => setTime(rule, e.currentTarget.value)} />
							</label>
							<div class="days" role="group" aria-label={t('settings.rules.days')}>
								{#each WEEK as d (d)}
									<button
										type="button"
										class="day"
										aria-pressed={dayOn(rule, d)}
										aria-label={dayName(d, 'long')}
										onclick={() => toggleDay(rule, d)}
									>
										{dayName(d, 'short')}
									</button>
								{/each}
							</div>
						</div>
					{:else if tr.type === 'calendar-start'}
						<div class="line detail" class:s-disabled={!rule.enabled}>
							<label class="s-inline">
								<span class="s-help">{t('settings.rules.feed')}</span>
								<select
									class="k-select s-select-sm"
									value={tr.feedId ?? ''}
									onchange={(e) => update(rule.id, { trigger: { type: 'calendar-start', feedId: e.currentTarget.value || null } })}
								>
									<option value="">{t('settings.rules.anyFeed')}</option>
									{#if tr.feedId !== null && !feeds.some((f) => f.id === tr.feedId)}
										<option value={tr.feedId}>{t('settings.rules.missingOption', { id: tr.feedId })}</option>
									{/if}
									{#each feeds as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
								</select>
							</label>
							<span class="s-help">{t('settings.rules.calendarHelp')}</span>
						</div>
					{/if}
					{#if issue}<p class="s-help warn issue"><Icon name="alert" size={14} />{issue}</p>{/if}
				</li>
			{/each}
		</ul>
	{/if}
	<p class="s-note small">{t('settings.rules.help')}</p>
</div>

<style>
	.between {
		justify-content: space-between;
	}
	.small {
		font-size: 12px;
	}
	.rule {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 8px 8px 8px 14px;
	}
	.line {
		display: flex;
		align-items: center;
		gap: 10px;
		min-width: 0;
	}
	.detail {
		flex-wrap: wrap;
		padding-left: 54px;
	}
	.field {
		flex: 1;
		min-width: 0;
	}
	.arrow {
		color: var(--k-text-muted);
		font-size: 14px;
	}
	.danger {
		border-color: transparent;
	}
	.danger:hover {
		color: var(--k-danger);
	}
	.days {
		display: flex;
		gap: 4px;
		flex-wrap: wrap;
	}
	.day {
		min-width: 44px;
		min-height: 44px;
		padding: 0 6px;
		border-radius: 10px;
		border: 1px solid var(--k-border);
		background: transparent;
		color: var(--k-text-muted);
		font-size: 12px;
		font-weight: 500;
	}
	.day[aria-pressed='true'] {
		border-color: var(--k-accent);
		background: color-mix(in srgb, var(--k-accent) 16%, transparent);
		color: var(--k-text);
		font-weight: 600;
	}
	.issue {
		margin: 0;
		display: flex;
		align-items: center;
		gap: 6px;
		padding-left: 54px;
	}
</style>
