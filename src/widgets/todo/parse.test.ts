import { describe, expect, it } from 'vitest';
import { parseQuickAdd } from './parse';

/** Tuesday 3 November 2026, 10:00 local. */
const NOW = new Date(2026, 10, 3, 10, 0, 0, 0);
const local = (mo: number, d: number, h: number, m = 0, y = 2026) => new Date(y, mo - 1, d, h, m).getTime();
const p = (input: string, now = NOW) => parseQuickAdd(input, now);

describe('parseQuickAdd: spec examples', () => {
	it.each([
		['buy milk tomorrow 9am', 'buy milk', local(11, 4, 9)],
		['deploy at 14:30', 'deploy', local(11, 3, 14, 30)],
		['call mom friday 6pm', 'call mom', local(11, 6, 18)],
		['standup in 30 min', 'standup', local(11, 3, 10, 30)],
		["yarın 9'da süt al", 'süt al', local(11, 4, 9)],
		['cuma 18:00 ara', 'ara', local(11, 6, 18)],
		['bugün 15:00', '', local(11, 3, 15)]
	])('%s', (input, text, due) => {
		const r = p(input);
		expect(r.text).toBe(text);
		expect(r.due).toBe(due);
		expect(r.hasTime).toBe(true);
	});
});

describe('parseQuickAdd: no date', () => {
	it('returns the trimmed text and null due', () => {
		expect(p('  buy   milk ')).toEqual({ text: 'buy milk', due: null, hasTime: false, spans: [] });
		expect(p('')).toEqual({ text: '', due: null, hasTime: false, spans: [] });
	});

	it.each(['buy 2 milk', 'read chapter 12', 'pay 2.50 for coffee', 'fix bug 404', 'ring room 3'])(
		'never treats a bare number as a time: %s',
		(input) => expect(p(input).due).toBeNull()
	);

	it('does not match weekday or month words inside other words', () => {
		expect(p('update monitoring').due).toBeNull();
		expect(p('fridays are fun').due).toBeNull();
		expect(p('marketing plan').due).toBeNull();
		expect(p('cumhuriyet bayramı').due).toBeNull();
	});
});

describe('parseQuickAdd: English times', () => {
	it.each([
		['wake 7am', local(11, 4, 7)], // past today → tomorrow
		['lunch 12pm', local(11, 3, 12)],
		['midnight snack 12am', local(11, 4, 0)],
		['call 9:45 pm', local(11, 3, 21, 45)],
		['call at 9', local(11, 4, 9)],
		['call at 11', local(11, 3, 11)],
		['meet @ 16:00', local(11, 3, 16)],
		['meet @4pm', local(11, 3, 16)],
		['lunch at noon', local(11, 3, 12)],
		['backup at midnight', local(11, 4, 0)],
		['review 10:00', local(11, 4, 10)], // exactly now is not the future
		['ping 5 p.m.', local(11, 3, 17)]
	])('%s', (input, due) => expect(p(input).due).toBe(due));

	it('rejects impossible times', () => {
		expect(p('meet 25:00').due).toBeNull();
		expect(p('meet 10:75').due).toBeNull();
		expect(p('meet 13pm').due).toBeNull();
	});
});

describe('parseQuickAdd: English days', () => {
	it.each([
		['pay rent today 5pm', 'pay rent', local(11, 3, 17)],
		['pay rent tomorrow', 'pay rent', local(11, 4, 9)],
		['dentist tmrw 8:15', 'dentist', local(11, 4, 8, 15)],
		['ship day after tomorrow', 'ship', local(11, 5, 9)],
		['gym on monday', 'gym', local(11, 9, 9)],
		['gym mon 7am', 'gym', local(11, 9, 7)],
		['retro on Fri at 3pm', 'retro', local(11, 6, 15)],
		['plan next wednesday', 'plan', local(11, 4, 9)],
		['party Saturday 8pm', 'party', local(11, 7, 20)],
		['call grandma sunday', 'call grandma', local(11, 8, 9)],
		['movie tonight', 'movie', local(11, 3, 20)],
		['movie tonight 9', 'movie', local(11, 3, 21)],
		['movie tonight at 21:30', 'movie', local(11, 3, 21, 30)]
	])('%s', (input, text, due) => {
		const r = p(input);
		expect(r.text).toBe(text);
		expect(r.due).toBe(due);
	});

	it('reads a bare hour only right after the day phrase', () => {
		expect(p('tomorrow 9 standup')).toMatchObject({ text: 'standup', due: local(11, 4, 9) });
		expect(p('tomorrow buy 2 apples')).toMatchObject({ text: 'buy 2 apples', due: local(11, 4, 9) });
	});

	it('is case-insensitive', () => {
		expect(p('Buy Milk TOMORROW 9AM')).toMatchObject({ text: 'Buy Milk', due: local(11, 4, 9) });
	});

	it('same weekday: today if still ahead, else next week; "next" always next week', () => {
		expect(p('standup tuesday 11:00').due).toBe(local(11, 3, 11));
		expect(p('standup tuesday 9:00').due).toBe(local(11, 10, 9));
		expect(p('standup next tuesday 11:00').due).toBe(local(11, 10, 11));
		expect(p('standup tuesday').due).toBe(local(11, 10, 9)); // 09:00 already past
	});

	it('date without time uses the default hour, or the next whole hour for today', () => {
		expect(p('file taxes today').due).toBe(local(11, 3, 11));
		expect(parseQuickAdd('file taxes today', new Date(2026, 10, 3, 7, 30)).due).toBe(local(11, 3, 9));
		expect(parseQuickAdd('x tomorrow', NOW, { defaultHour: 8 }).due).toBe(local(11, 4, 8));
		expect(p('file taxes today').hasTime).toBe(false);
	});

	it('"today" late in the evening rolls the next whole hour into tomorrow', () => {
		expect(parseQuickAdd('x today', new Date(2026, 10, 3, 23, 30)).due).toBe(local(11, 4, 0));
	});
});

describe('parseQuickAdd: relative', () => {
	it.each([
		['tea in 5 minutes', 'tea', local(11, 3, 10, 5)],
		['tea in 5m', 'tea', local(11, 3, 10, 5)],
		['deploy in 2h', 'deploy', local(11, 3, 12)],
		['deploy in an hour', 'deploy', local(11, 3, 11)],
		['stretch in half an hour', 'stretch', local(11, 3, 10, 30)],
		['renew in 3 days', 'renew', local(11, 6, 9)],
		['renew in 2 weeks at 4pm', 'renew', local(11, 17, 16)],
		['çay 10 dk sonra', 'çay', local(11, 3, 10, 10)],
		['toplantı 2 saat sonra', 'toplantı', local(11, 3, 12)],
		['mola yarım saat sonra', 'mola', local(11, 3, 10, 30)],
		['fatura 3 gün sonra', 'fatura', local(11, 6, 9)],
		['rapor 1 hafta sonra 14:00', 'rapor', local(11, 10, 14)],
		['ara 15 dakika içinde', 'ara', local(11, 3, 10, 15)]
	])('%s', (input, text, due) => {
		const r = p(input);
		expect(r.text).toBe(text);
		expect(r.due).toBe(due);
	});

	it('a relative instant wins and leaves other phrases in the text', () => {
		expect(p('call in 30 min not tomorrow')).toMatchObject({ text: 'call not tomorrow', due: local(11, 3, 10, 30) });
	});

	it('rounds to the minute', () => {
		expect(parseQuickAdd('x in 1 min', new Date(2026, 10, 3, 10, 0, 40)).due).toBe(local(11, 3, 10, 2));
	});
});

describe('parseQuickAdd: Turkish', () => {
	it.each([
		["yarın saat 9'da süt al", 'süt al', local(11, 4, 9)],
		['YARIN 10:30 toplantı', 'toplantı', local(11, 4, 10, 30)],
		['Yarın toplantı', 'toplantı', local(11, 4, 9)],
		["cumartesi 11'de kahvaltı", 'kahvaltı', local(11, 7, 11)],
		['pazartesi rapor', 'rapor', local(11, 9, 9)],
		['pazar günü aile', 'aile', local(11, 8, 9)],
		['çarşamba 14:00 sunum', 'sunum', local(11, 4, 14)],
		['perşembe saat 16.30 dişçi', 'dişçi', local(11, 5, 16, 30)],
		["salı'ya not", 'not', local(11, 10, 9)],
		["akşam 6'da yemek", 'yemek', local(11, 3, 18)],
		['sabah 8 koşu', 'koşu', local(11, 4, 8)],
		['öğleden sonra 3 toplantı', 'toplantı', local(11, 3, 15)],
		['gece 11 yedek al', 'yedek al', local(11, 3, 23)],
		['bu akşam sinema', 'sinema', local(11, 3, 20)],
		["bu akşam 9'da sinema", 'sinema', local(11, 3, 21)],
		['haftaya salı 10:30 retro', 'retro', local(11, 10, 10, 30)],
		['öbür gün kargo', 'kargo', local(11, 5, 9)],
		['öğlen yemek', 'yemek', local(11, 3, 12)],
		["15'te ara", 'ara', local(11, 3, 15)],
		["18:00'de çık", 'çık', local(11, 3, 18)]
	])('%s', (input, text, due) => {
		const r = p(input);
		expect(r.text).toBe(text);
		expect(r.due).toBe(due);
	});
});

describe('parseQuickAdd: absolute dates', () => {
	it.each([
		['release 2026-11-20', 'release', local(11, 20, 9)],
		['release 2026-11-20 14:30', 'release', local(11, 20, 14, 30)],
		['release 2026-11-20T08:05', 'release', local(11, 20, 8, 5)],
		['release on 2027-01-02', 'release', local(1, 2, 9, 0, 2027)],
		['party 20 nov', 'party', local(11, 20, 9)],
		['party nov 20th 7pm', 'party', local(11, 20, 19)],
		['party 5 December', 'party', local(12, 5, 9)],
		['doğum günü 12 ocak', 'doğum günü', local(1, 12, 9, 0, 2027)], // past this year → next year
		["vize 5 kasım'da 10:00", 'vize', local(11, 5, 10)],
		['tatil 1 Mayıs', 'tatil', local(5, 1, 9, 0, 2027)]
	])('%s', (input, text, due) => {
		const r = p(input);
		expect(r.text).toBe(text);
		expect(r.due).toBe(due);
	});

	it('rejects impossible calendar dates', () => {
		expect(p('x 2026-02-30').due).toBeNull();
		expect(p('x 2026-13-01').due).toBeNull();
		expect(p('x 31 nov').due).toBeNull();
	});
});

describe('parseQuickAdd: spans and cleanup', () => {
	it('reports removed spans in input order', () => {
		const r = p('buy milk tomorrow 9am');
		expect(r.spans).toEqual([
			[9, 17],
			[18, 21]
		]);
	});

	it('keeps typographic apostrophes and tidies punctuation', () => {
		expect(p('yarın 9’da süt al').text).toBe('süt al');
		expect(p('email Ann, tomorrow 9am').text).toBe('email Ann');
		expect(p('tomorrow - pick up keys').text).toBe('pick up keys');
	});

	it('removes the lead-in prepositions with the phrase', () => {
		expect(p('call Bob at 3pm').text).toBe('call Bob');
		expect(p('sign on monday').text).toBe('sign');
		expect(p('submit by friday').text).toBe('submit');
	});

	it('handles astral characters without shifting spans', () => {
		expect(p('🎉 party tomorrow 8pm')).toMatchObject({ text: '🎉 party', due: local(11, 4, 20) });
	});
});
