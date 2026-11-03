import { describe, expect, it } from 'vitest';
import type { Agent, Approval, CompanionStatus } from '#lib/ipc/index.ts';
import {
	agentLabel,
	formatCost,
	formatTokens,
	isConnected,
	minutesIn,
	normalizeStatus,
	pendingApprovals,
	runningCount,
	settleHidden,
	sortAgents,
	statusTone,
	usageBar,
	viewOf
} from './logic';

const agent = (id: string, status: Agent['status'] | string, since: number | null = null, repo = 'deck'): Agent => ({
	id,
	repo,
	name: 'claude',
	status: status as Agent['status'],
	task: null,
	since
});

const approval = (id: string, requestedAt: number): Approval => ({ id, agentId: 'a1', summary: `Run ${id}`, detail: null, requestedAt });

const status = (over: Partial<CompanionStatus> = {}): CompanionStatus => ({
	enabled: true,
	listening: true,
	connected: true,
	client: '1.4.0',
	port: 47831,
	error: null,
	...over
});

describe('agent sorting', () => {
	it('puts waiting first, then working, then idle', () => {
		const sorted = sortAgents([agent('i', 'idle'), agent('w', 'working'), agent('q', 'waiting')]);
		expect(sorted.map((a) => a.id)).toEqual(['q', 'w', 'i']);
	});

	it('orders by the oldest since within a status, then by label', () => {
		const sorted = sortAgents([
			agent('late', 'waiting', 300),
			agent('early', 'waiting', 100),
			agent('b', 'working', null, 'b-repo'),
			agent('a', 'working', null, 'a-repo')
		]);
		expect(sorted.map((a) => a.id)).toEqual(['early', 'late', 'a', 'b']);
	});

	it('shows unknown statuses as idle and does not mutate the input', () => {
		const input = [agent('x', 'thinking'), agent('w', 'working')];
		const sorted = sortAgents(input);
		expect(sorted.map((a) => [a.id, a.status])).toEqual([
			['w', 'working'],
			['x', 'idle']
		]);
		expect(input[0].status).toBe('thinking');
		expect(normalizeStatus(undefined)).toBe('idle');
	});

	it('counts running (non-idle) agents and maps dot tones', () => {
		expect(runningCount([agent('a', 'idle'), agent('b', 'working'), agent('c', 'waiting'), agent('d', 'bogus')])).toBe(2);
		expect(statusTone('idle')).toBe('border');
		expect(statusTone('working')).toBe('accent');
		expect(statusTone('waiting')).toBe('warning');
		expect(agentLabel({ repo: 'kotilo-deck', name: 'rust-platform' })).toBe('kotilo-deck · rust-platform');
		expect(agentLabel({ repo: '', name: 'claude' })).toBe('claude');
	});
});

describe('approval queue', () => {
	it('is oldest first and hides optimistic removals', () => {
		const list = [approval('b', 20), approval('a', 10), approval('c', 30)];
		expect(pendingApprovals(list, new Set(['c'])).map((a) => a.id)).toEqual(['a', 'b']);
	});

	it('drops settled optimistic ids once the snapshot no longer lists them', () => {
		const hidden = settleHidden(new Set(['a', 'gone']), [approval('a', 1)]);
		expect([...hidden]).toEqual(['a']);
	});
});

describe('connection', () => {
	it('is connected only when enabled, listening and a client is attached', () => {
		expect(isConnected(null)).toBe(false);
		expect(isConnected(status())).toBe(true);
		expect(isConnected(status({ connected: false }))).toBe(false);
		expect(isConnected(status({ enabled: false }))).toBe(false);
		expect(isConnected(status({ listening: false }))).toBe(false);
	});

	it('normalises a malformed snapshot', () => {
		expect(viewOf(null)).toEqual({ agents: [], approvals: [], usage: null });
		expect(viewOf({ agents: null as never, approvals: undefined as never, usage: null, updatedAt: null })).toEqual({
			agents: [],
			approvals: [],
			usage: null
		});
	});
});

describe('usage', () => {
	it('formats cost as USD in the locale', () => {
		expect(formatCost(4.12, 'en-US')).toBe('$4.12');
		expect(formatCost(4, 'en-US')).toBe('$4.00');
		expect(formatCost(-1, 'en-US')).toBe('$0.00');
		expect(formatCost(4.12, 'tr-TR')).toMatch(/4,12/);
	});

	it('formats tokens compactly', () => {
		expect(formatTokens(1_840_000, 'en-US')).toBe('1.8M');
		expect(formatTokens(950, 'en-US')).toBe('950');
		expect(formatTokens(NaN, 'en-US')).toBe('0');
	});

	it('builds the budget bar only with a budget', () => {
		expect(usageBar(null)).toEqual({ fraction: null, warn: false });
		expect(usageBar({ tokens: 1, costUsd: 4.12, budgetUsd: null })).toEqual({ fraction: null, warn: false });
		expect(usageBar({ tokens: 1, costUsd: 4, budgetUsd: 10 })).toEqual({ fraction: 0.4, warn: false });
		expect(usageBar({ tokens: 1, costUsd: 9.5, budgetUsd: 10 })).toEqual({ fraction: 0.95, warn: true });
		expect(usageBar({ tokens: 1, costUsd: 30, budgetUsd: 10 })).toEqual({ fraction: 1, warn: true });
		expect(usageBar({ tokens: 1, costUsd: 3, budgetUsd: 0 }).fraction).toBeNull();
	});

	it('computes minutes in status', () => {
		expect(minutesIn(null, 1e6)).toBeNull();
		expect(minutesIn(0, 12 * 60_000 + 59_000)).toBe(12);
		expect(minutesIn(1e6, 0)).toBe(0);
	});
});
