/**
 * Quick-add parser (SPEC §4.4 Todo, §9): pulls a due date/time out of free text in English or
 * Turkish and returns the remaining text. Pure; `now` is injected so tests are deterministic.
 *
 *   "buy milk tomorrow 9am"  → { text: "buy milk", due: tomorrow 09:00 }
 *   "yarın 9'da süt al"      → { text: "süt al",   due: tomorrow 09:00 }
 *   "standup in 30 min"      → { text: "standup",  due: now + 30 min }
 *
 * Rules (DECISIONS):
 * - A bare number is never a time ("buy 2 milk"). Times need a colon (`14:30`), am/pm, a lead-in
 *   (`at 9`, `saat 9`, `@9`), a Turkish locative suffix (`9'da`, `15'te`) or a day-part word
 *   (`akşam 6`).
 * - Time only, already past today → tomorrow. Plain weekday that is today with a past time → next week.
 *   `next <weekday>` / `haftaya <weekday>` never means today.
 * - Date without a time → `defaultHour` (09:00); if that is already past (e.g. "today" at 16:20) → the
 *   next whole hour. "tonight" / "bu akşam" → 20:00, and `tonight 9` means 21:00.
 * - Relative minutes/hours (`in 30 min`, `2 saat sonra`) are an instant and win over any other phrase.
 * - The first match of each kind counts; later ones stay in the text.
 */

export interface ParseResult {
	/** Input with the parsed phrases removed and whitespace collapsed. */
	text: string;
	/** Local Unix ms, or null when nothing was recognised. */
	due: number | null;
	/** True when the phrase carried an explicit time (or a relative offset). */
	hasTime: boolean;
	/** Removed `[start, end)` ranges of the input, ascending. */
	spans: [number, number][];
}

export interface ParseOptions {
	/** Hour used for a date without a time. Default 9. */
	defaultHour?: number;
}

const MINUTE = 60_000;

// Letter/digit boundaries that understand Turkish letters (`\b` is ASCII-only).
const B = '(?<![\\p{L}\\p{N}])';
const E = '(?![\\p{L}\\p{N}])';
/** Straight or typographic apostrophe before a Turkish suffix. */
const APOS = "['’]";
/** Turkish case suffixes after an apostrophe: 9'da, cuma'ya, 5 kasım'da. */
const SUFFIX = `(?:${APOS}(?:da|de|ta|te|ya|ye|a|e))?`;

/**
 * Lower-cases one UTF-16 unit at a time so indices match the input exactly. `I` and `İ` both map to
 * `i`; patterns write the Turkish dotless `ı` as `[ıi]` so "YARIN" still matches.
 */
function normalise(input: string): string {
	let out = '';
	for (const ch of input) {
		if (ch === 'İ' || ch === 'I') out += 'i';
		else {
			const lower = ch.toLowerCase();
			out += lower.length === ch.length ? lower : ch;
		}
	}
	return out;
}

const re = (body: string) => new RegExp(body, 'gu');

const WEEKDAYS: [string, number][] = [
	// Longest first so "cumartesi" beats "cuma" and "pazartesi" beats "pazar".
	['cumartesi', 6],
	['pazartesi', 1],
	['çarşamba', 3],
	['perşembe', 4],
	['wednesday', 3],
	['thursday', 4],
	['saturday', 6],
	['tuesday', 2],
	['monday', 1],
	['friday', 5],
	['sunday', 0],
	['thurs', 4],
	['tues', 2],
	['thur', 4],
	['pazar', 0],
	['sal[ıi]', 2],
	['cuma', 5],
	['mon', 1],
	['tue', 2],
	['wed', 3],
	['thu', 4],
	['fri', 5]
];

const MONTHS: [string, number][] = [
	['january', 1],
	['february', 2],
	['march', 3],
	['april', 4],
	['june', 6],
	['july', 7],
	['august', 8],
	['september', 9],
	['sept', 9],
	['october', 10],
	['november', 11],
	['december', 12],
	['jan', 1],
	['feb', 2],
	['mar', 3],
	['apr', 4],
	['may', 5],
	['jun', 6],
	['jul', 7],
	['aug', 8],
	['sep', 9],
	['oct', 10],
	['nov', 11],
	['dec', 12],
	['ocak', 1],
	['şubat', 2],
	['mart', 3],
	['nisan', 4],
	['may[ıi]s', 5],
	['haziran', 6],
	['temmuz', 7],
	['ağustos', 8],
	['eylül', 9],
	['ekim', 10],
	['kas[ıi]m', 11],
	['aral[ıi]k', 12]
];

const alt = (list: [string, number][]) => list.map(([w]) => w).join('|');
const lookup = (list: [string, number][], word: string): number => {
	for (const [w, n] of list) if (new RegExp(`^(?:${w})$`, 'u').test(word)) return n;
	return -1;
};

const ISO = re(`${B}(?:(?:on|by)\\s+)?(\\d{4})-(\\d{2})-(\\d{2})(?:[t ](\\d{1,2}):(\\d{2}))?${E}`);

const REL_EN = re(
	`${B}in\\s+(\\d+|an?|half\\s+an?)\\s*(minutes|minute|mins|min|m|hours|hour|hrs|hr|h|days|day|d|weeks|week|wks|wk|w)${E}`
);
const REL_TR = re(
	`${B}(\\d+|yar[ıi]m|bir)\\s*(dakika|dak|dk|saat|sa|gün|hafta)\\s+(?:sonra|i[çc]inde)${E}`
);

const DAYWORD = re(
	`${B}(?:(?:on|by)\\s+)?(?:(next|this|gelecek|haftaya|önümüzdeki|bu)\\s+)?(day\\s+after\\s+tomorrow|today|tonight|tomorrow|tmrw|tmr|bugün|yar[ıi]n|öbür\\s*gün|ertesi\\s*gün|ak[şs]am|${alt(WEEKDAYS)})(?:\\s+günü)?${SUFFIX}${E}`
);

const MONTH_DM = re(`${B}(?:(?:on|by)\\s+)?(\\d{1,2})(?:st|nd|rd|th|\\.)?\\s+(${alt(MONTHS)})${SUFFIX}${E}`);
const MONTH_MD = re(`${B}(?:(?:on|by)\\s+)?(${alt(MONTHS)})\\s+(\\d{1,2})(?:st|nd|rd|th)?${E}`);

const DAYPART = 'sabah|ak[şs]am|öğleden\\s+sonra|gece|morning|evening|afternoon';
const TIME = re(
	`${B}(?:(${DAYPART})\\s+)?(?:(at|saat)\\s+|(@)\\s*)?(\\d{1,2})(?:([:.])(\\d{2}))?(?:\\s*(am|pm|a\\.m\\.|p\\.m\\.)|${APOS}(?:da|de|ta|te))?${E}`
);
const NAMED_TIME = re(`${B}(?:(?:at|saat)\\s+)?(noon|midnight|öğlen|öğle|gece\\s+yar[ıi]s[ıi])(?:${APOS}(?:da|de|ta|te))?${E}`);

type Span = [number, number];

const overlaps = (a: Span, taken: Span[]) => taken.some(([s, e]) => a[0] < e && s < a[1]);

/** First match of `pattern` that does not overlap `taken` and passes `accept`. */
function first(
	s: string,
	pattern: RegExp,
	taken: Span[],
	accept: (m: RegExpExecArray) => boolean = () => true
): RegExpExecArray | null {
	pattern.lastIndex = 0;
	for (let m = pattern.exec(s); m; m = pattern.exec(s)) {
		const span: Span = [m.index, m.index + m[0].length];
		if (!overlaps(span, taken) && accept(m)) return m;
		// Retry from the next character: a rejected long match may hide a valid shorter one inside it.
		pattern.lastIndex = m.index + 1;
	}
	return null;
}

const spanOf = (m: RegExpExecArray): Span => [m.index, m.index + m[0].length];

interface DayMatch {
	/** Local calendar day. */
	y: number;
	mo: number;
	d: number;
	/** Plain weekday: may roll a week if the time is already past. */
	weekdayToday?: boolean;
	evening?: boolean;
}

function addDays(now: Date, n: number): { y: number; mo: number; d: number } {
	const t = new Date(now.getFullYear(), now.getMonth(), now.getDate() + n);
	return { y: t.getFullYear(), mo: t.getMonth() + 1, d: t.getDate() };
}

function dayFromWord(word: string, mod: string | undefined, now: Date): DayMatch | null {
	const w = word.replace(/\s+/g, ' ');
	if (w === 'today' || w === 'bugün') return addDays(now, 0);
	if (w === 'tonight' || ((w === 'akşam' || w === 'aksam') && mod === 'bu')) return { ...addDays(now, 0), evening: true };
	if (w === 'akşam' || w === 'aksam') return null; // a day part, handled with the time
	if (w === 'tomorrow' || w === 'tmrw' || w === 'tmr' || /^yar[ıi]n$/u.test(w)) return addDays(now, 1);
	if (w === 'day after tomorrow' || /^(öbür|ertesi) ?gün$/u.test(w)) return addDays(now, 2);
	const wd = lookup(WEEKDAYS, w);
	if (wd < 0) return null;
	const strict = mod === 'next' || mod === 'gelecek' || mod === 'haftaya' || mod === 'önümüzdeki';
	let diff = (wd - now.getDay() + 7) % 7;
	if (diff === 0 && strict) diff = 7;
	return { ...addDays(now, diff), weekdayToday: diff === 0 };
}

interface TimeMatch {
	h: number;
	m: number;
	/** No am/pm and no 24 h hint: "tonight 9" may move it to the evening. */
	ambiguous: boolean;
}

/** `afterDay`: the number directly follows a day phrase ("tomorrow 9", "tonight 9"), so it is a time. */
function timeFromMatch(m: RegExpExecArray, afterDay = false): TimeMatch | null {
	const [, partRaw, lead, atSign, hRaw, sep, mRaw, apRaw] = m;
	const suffix = new RegExp(`${APOS}(?:da|de|ta|te)$`, 'u').test(m[0]);
	const part = partRaw?.replace(/\s+/g, ' ');
	const ap = apRaw?.replace(/\./g, '');
	// Context that makes a number a time; a bare number never is.
	if (!sep && !ap && !lead && !atSign && !suffix && !part && !afterDay) return null;
	// "2.50" is a price, "saat 18.00" / "18.00'de" a time.
	if (sep === '.' && !lead && !atSign && !suffix && !part) return null;
	let h = Number(hRaw);
	const min = mRaw ? Number(mRaw) : 0;
	if (min > 59) return null;
	if (ap) {
		if (h < 1 || h > 12) return null;
		h = (h % 12) + (ap === 'pm' ? 12 : 0);
		return { h, m: min, ambiguous: false };
	}
	if (h > 23) return null;
	if (part && h < 12) {
		if (part === 'akşam' || part === 'aksam' || part === 'öğleden sonra' || part === 'evening' || part === 'afternoon') h += 12;
		else if (part === 'gece' && h >= 6) h += 12;
	}
	return { h, m: min, ambiguous: !part && h < 12 && !(hRaw.length === 2 && hRaw.startsWith('0')) };
}

function cleanText(input: string, spans: Span[]): string {
	let out = '';
	let pos = 0;
	for (const [s, e] of spans) {
		out += `${input.slice(pos, s)} `;
		pos = e;
	}
	out += input.slice(pos);
	return out
		.replace(/\s+/g, ' ')
		.replace(/\s+([,.;:!?])/g, '$1')
		.replace(/^[\s,;:–—-]+|[\s,;:–—-]+$/g, '')
		.trim();
}

const at = (y: number, mo: number, d: number, h: number, m: number) => new Date(y, mo - 1, d, h, m, 0, 0).getTime();

const validDay = (y: number, mo: number, d: number) => {
	if (mo < 1 || mo > 12 || d < 1 || d > 31) return false;
	const t = new Date(y, mo - 1, d);
	return t.getMonth() === mo - 1 && t.getDate() === d;
};

export function parseQuickAdd(input: string, now: Date = new Date(), opts: ParseOptions = {}): ParseResult {
	const defaultHour = opts.defaultHour ?? 9;
	const s = normalise(input);
	const none: ParseResult = { text: input.trim().replace(/\s+/g, ' '), due: null, hasTime: false, spans: [] };
	if (!s.trim()) return { ...none, text: '' };

	const taken: Span[] = [];
	const finish = (due: number, hasTime: boolean): ParseResult => {
		const spans = [...taken].sort((a, b) => a[0] - b[0]);
		return { text: cleanText(input, spans), due, hasTime, spans };
	};

	// 1. Relative instant / relative day offset.
	let dayOffset: number | null = null;
	const rel = first(s, REL_EN, taken) ?? first(s, REL_TR, taken);
	if (rel) {
		const qRaw = rel[1].replace(/\s+/g, ' ');
		const unit = rel[2];
		let q = /^\d+$/.test(qRaw) ? Number(qRaw) : qRaw.startsWith('half') || /^yar/u.test(qRaw) ? 0.5 : 1;
		const minutes =
			/^(m|min|mins|minute|minutes|dk|dak|dakika)$/.test(unit) ? q
			: /^(h|hr|hrs|hour|hours|sa|saat)$/.test(unit) ? q * 60
			: null;
		if (minutes !== null) {
			if (minutes > 0) {
				taken.push(spanOf(rel));
				return finish(Math.round((now.getTime() + minutes * MINUTE) / MINUTE) * MINUTE, true);
			}
		} else {
			if (/^(w|wk|wks|week|weeks|hafta)$/.test(unit)) q *= 7;
			if (Number.isInteger(q) && q > 0) {
				dayOffset = q;
				taken.push(spanOf(rel));
			}
		}
	}

	// 2. Calendar day.
	let day: DayMatch | null = dayOffset !== null ? addDays(now, dayOffset) : null;
	let isoTime: TimeMatch | null = null;
	if (!day) {
		const iso = first(s, ISO, taken, (m) => validDay(Number(m[1]), Number(m[2]), Number(m[3])));
		if (iso) {
			day = { y: Number(iso[1]), mo: Number(iso[2]), d: Number(iso[3]) };
			if (iso[4] !== undefined) {
				const h = Number(iso[4]);
				const m = Number(iso[5]);
				if (h <= 23 && m <= 59) isoTime = { h, m, ambiguous: false };
			}
			taken.push(spanOf(iso));
		}
	}
	if (!day) {
		const w = first(s, DAYWORD, taken, (m) => dayFromWord(m[2], m[1], now) !== null);
		if (w) {
			day = dayFromWord(w[2], w[1], now);
			taken.push(spanOf(w));
		}
	}
	if (!day) {
		const dm = first(s, MONTH_DM, taken, (m) => lookup(MONTHS, m[2]) > 0 && Number(m[1]) >= 1 && Number(m[1]) <= 31);
		const md = dm ? null : first(s, MONTH_MD, taken, (m) => lookup(MONTHS, m[1]) > 0 && Number(m[2]) >= 1 && Number(m[2]) <= 31);
		const m = dm ?? md;
		if (m) {
			const mo = dm ? lookup(MONTHS, dm[2]) : lookup(MONTHS, md![1]);
			const d = dm ? Number(dm[1]) : Number(md![2]);
			let y = now.getFullYear();
			// Already gone this year → next year.
			if (at(y, mo, d, 23, 59) < now.getTime()) y += 1;
			if (validDay(y, mo, d)) {
				day = { y, mo, d };
				taken.push(spanOf(m));
			}
		}
	}

	// 3. Time of day. A bare hour counts only right after the day phrase.
	const dayEnd = day && taken.length ? taken[taken.length - 1][1] : null;
	const follows = (m: RegExpExecArray) => dayEnd !== null && /^\s+$/.test(s.slice(dayEnd, m.index));
	let time: TimeMatch | null = isoTime;
	if (!time) {
		const named = first(s, NAMED_TIME, taken);
		const tm = first(s, TIME, taken, (m) => timeFromMatch(m, follows(m)) !== null);
		// Leftmost wins when both appear.
		const pick = named && (!tm || named.index < tm.index) ? named : tm;
		if (pick === named && named) {
			time = { h: named[1] === 'midnight' || named[1].startsWith('gece') ? 0 : 12, m: 0, ambiguous: false };
			taken.push(spanOf(named));
		} else if (tm) {
			time = timeFromMatch(tm, follows(tm));
			taken.push(spanOf(tm));
		}
	}

	if (!day && !time) return none;

	const nowMs = now.getTime();
	if (!day) {
		// Time only: today, or tomorrow if that moment has passed.
		const t = time!;
		let due = at(now.getFullYear(), now.getMonth() + 1, now.getDate(), t.h, t.m);
		if (due <= nowMs) {
			const n = addDays(now, 1);
			due = at(n.y, n.mo, n.d, t.h, t.m);
		}
		return finish(due, true);
	}

	if (time) {
		let { h } = time;
		if (day.evening && time.ambiguous && h < 12) h += 12;
		let due = at(day.y, day.mo, day.d, h, time.m);
		if (day.weekdayToday && due <= nowMs) {
			const n = addDays(new Date(day.y, day.mo - 1, day.d), 7);
			due = at(n.y, n.mo, n.d, h, time.m);
		}
		return finish(due, true);
	}

	// Date without a time.
	let due = at(day.y, day.mo, day.d, day.evening ? 20 : defaultHour, 0);
	if (due <= nowMs) {
		const isToday = day.y === now.getFullYear() && day.mo === now.getMonth() + 1 && day.d === now.getDate();
		if (day.weekdayToday) {
			const n = addDays(now, 7);
			due = at(n.y, n.mo, n.d, defaultHour, 0);
		} else if (isToday) {
			// Next whole local hour (Date rolls 24:00 over to tomorrow).
			due = at(day.y, day.mo, day.d, now.getHours() + 1, 0);
		}
	}
	return finish(due, false);
}
