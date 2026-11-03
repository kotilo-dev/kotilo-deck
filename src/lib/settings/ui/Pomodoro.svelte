<script lang="ts">
	/** Pomodoro (SPEC §4.4): one shared timer for every pomodoro widget; saved as `settings.pomodoro`. */
	import type { PomodoroSettings } from '#lib/ipc/index.ts';
	import { t } from '#lib/i18n/index.ts';
	import { getSettingsContext } from './context.svelte';
	import Head from './Head.svelte';
	import Stepper from './Stepper.svelte';
	import Switch from './Switch.svelte';

	const { store, save } = getSettingsContext();
	const p = $derived(store.settings.pomodoro);

	type NumKey = Exclude<keyof PomodoroSettings, 'autoStartNext'>;
	/** Ranges from the IPC contract (PomodoroSettings). */
	const FIELDS: { key: NumKey; min: number; max: number; unit: boolean }[] = [
		{ key: 'focus', min: 1, max: 180, unit: true },
		{ key: 'shortBreak', min: 1, max: 60, unit: true },
		{ key: 'longBreak', min: 1, max: 120, unit: true },
		{ key: 'sessionsPerCycle', min: 1, max: 12, unit: false }
	];

	const set = (patch: Partial<PomodoroSettings>) => void save({ pomodoro: { ...p, ...patch } });
</script>

<section class="s-page">
	<Head title={t('settings.pomodoro.title')} text={t('settings.pomodoro.text')} />

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.pomodoro.timer')}</h2>
		{#each FIELDS as f (f.key)}
			<div class="s-card s-row">
				<label class="s-label" for="k-pm-{f.key}">
					<span class="s-title">{t(`settings.pomodoro.${f.key}`)}</span>
					<span class="s-help" id="k-pm-{f.key}-help">{t(`settings.pomodoro.${f.key}Help`, { min: f.min, max: f.max })}</span>
				</label>
				<Stepper
					id="k-pm-{f.key}"
					value={p[f.key]}
					min={f.min}
					max={f.max}
					label={t(`settings.pomodoro.${f.key}`)}
					unit={f.unit ? t('settings.pomodoro.min') : undefined}
					describedby="k-pm-{f.key}-help"
					onchange={(v) => set({ [f.key]: v })}
				/>
			</div>
		{/each}
		<div class="s-card s-row">
			<div class="s-label">
				<span class="s-title">{t('settings.pomodoro.autoStart')}</span>
				<span class="s-help" id="k-pm-auto">{t('settings.pomodoro.autoStartHelp')}</span>
			</div>
			<Switch label={t('settings.pomodoro.autoStart')} describedby="k-pm-auto" checked={p.autoStartNext} onchange={(v) => set({ autoStartNext: v })} />
		</div>
	</div>

	<div class="s-group">
		<h2 class="s-eyebrow">{t('settings.pomodoro.today')}</h2>
		<div class="s-card s-row">
			<label class="s-label" for="k-pm-goal">
				<span class="s-title">{t('settings.pomodoro.dailyGoal')}</span>
				<span class="s-help" id="k-pm-goal-help">{t('settings.pomodoro.dailyGoalHelp')}</span>
			</label>
			<Stepper
				id="k-pm-goal"
				value={p.dailyGoal}
				min={1}
				max={24}
				label={t('settings.pomodoro.dailyGoal')}
				describedby="k-pm-goal-help"
				onchange={(v) => set({ dailyGoal: v })}
			/>
		</div>
		<p class="s-note small">{t('settings.pomodoro.note')}</p>
	</div>
</section>

<style>
	.small {
		font-size: 12px;
	}
</style>
