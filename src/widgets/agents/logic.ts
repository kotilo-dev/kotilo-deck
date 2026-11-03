/**
 * Agent Status view logic (DESIGN §5.10, protocol.md `agents.state` / `usage.today`). Pure functions.
 */
import type { Agent, AgentStatus, Approval, CompanionSnapshot, CompanionStatus, Usage } from '#lib/ipc/index.ts';

/** protocol.md: unknown status values are shown as idle. */
export function normalizeStatus(status: unknown): AgentStatus {
	return status === 'working' || status === 'waiting' ? status : 'idle';
}

const RANK: Record<AgentStatus, number> = { waiting: 0, working: 1, idle: 2 };

/**
 * Waiting (needs the user) first, then working, then idle. Within a status the oldest `since` comes first
 * (longest wait / longest run), then repo · name for a stable order.
 */
export function sortAgents(agents: readonly Agent[]): Agent[] {
	return agents
		.map((a) => ({ ...a, status: normalizeStatus(a.status) }))
		.sort((a, b) => {
			const r = RANK[a.status] - RANK[b.status];
			if (r !== 0) return r;
			const sa = a.since ?? Number.MAX_SAFE_INTEGER;
			const sb = b.since ?? Number.MAX_SAFE_INTEGER;
			if (sa !== sb) return sa - sb;
			return `${a.repo}·${a.name}`.localeCompare(`${b.repo}·${b.name}`);
		});
}

/** "Running" = not idle (working or waiting for approval). */
export const runningCount = (agents: readonly Agent[]): number => agents.filter((a) => normalizeStatus(a.status) !== 'idle').length;

/** Oldest request first, minus the ones answered locally and still in flight. */
export function pendingApprovals(approvals: readonly Approval[], hidden: ReadonlySet<string>): Approval[] {
	return approvals.filter((a) => !hidden.has(a.id)).sort((a, b) => a.requestedAt - b.requestedAt);
}

/**
 * Optimistic ids that the next snapshot no longer lists are settled (the desktop resolved them); the rest
 * stay hidden until their `companionRespond` call settles.
 */
export function settleHidden(hidden: ReadonlySet<string>, approvals: readonly Approval[]): Set<string> {
	const live = new Set(approvals.map((a) => a.id));
	return new Set([...hidden].filter((id) => live.has(id)));
}

/** Disconnected = companion off, server not listening, or no desktop client. */
export const isConnected = (status: CompanionStatus | null): boolean => !!status && status.enabled && status.listening && status.connected;

/** "kotilo-deck · claude" */
export const agentLabel = (agent: Pick<Agent, 'repo' | 'name'>): string => [agent.repo, agent.name].filter(Boolean).join(' · ');

/** Status dot token (DESIGN §5.10): idle = border, working = accent, waiting = warning. */
export const statusTone = (status: AgentStatus): 'border' | 'accent' | 'warning' =>
	status === 'waiting' ? 'warning' : status === 'working' ? 'accent' : 'border';

const currency = new Map<string, Intl.NumberFormat>();

/** "$4.12" in the OS locale's shape ("$4,12" in tr). */
export function formatCost(usd: number, locale: string): string {
	let f = currency.get(locale);
	if (!f) {
		f = new Intl.NumberFormat(locale, { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 });
		currency.set(locale, f);
	}
	return f.format(Number.isFinite(usd) ? Math.max(0, usd) : 0);
}

/** 1 840 000 → "1.8M" (compact, locale-aware). */
export function formatTokens(tokens: number, locale: string): string {
	const n = Number.isFinite(tokens) ? Math.max(0, tokens) : 0;
	return new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(n);
}

export interface UsageBar {
	/** 0–1 of the budget; null without a budget (tokens/cost text only). */
	fraction: number | null;
	/** At or above 90 % of the budget. */
	warn: boolean;
}

export function usageBar(usage: Usage | null): UsageBar {
	if (!usage || usage.budgetUsd === null || usage.budgetUsd === undefined || !(usage.budgetUsd > 0)) return { fraction: null, warn: false };
	const fraction = Math.min(1, Math.max(0, usage.costUsd / usage.budgetUsd));
	return { fraction, warn: usage.costUsd / usage.budgetUsd >= 0.9 };
}

/** Whole minutes an agent has been in its status, null without `since`. */
export const minutesIn = (since: number | null, now: number): number | null =>
	since === null ? null : Math.max(0, Math.floor((now - since) / 60_000));

/** Snapshot with agents normalised and sorted; safe defaults for a malformed payload. */
export function viewOf(snapshot: CompanionSnapshot | null): { agents: Agent[]; approvals: Approval[]; usage: Usage | null } {
	if (!snapshot) return { agents: [], approvals: [], usage: null };
	return {
		agents: sortAgents(Array.isArray(snapshot.agents) ? snapshot.agents : []),
		approvals: Array.isArray(snapshot.approvals) ? snapshot.approvals : [],
		usage: snapshot.usage ?? null
	};
}
