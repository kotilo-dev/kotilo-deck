import { describe, expect, it } from 'vitest';
import { base64Utf8, canInstall, maskUrl, portProblem, svgDataUrl, updateBusy } from './lan';

describe('LAN port validation', () => {
	it('accepts 1024–65535 except the companion port', () => {
		expect(portProblem('47832')).toBeNull();
		expect(portProblem(' 1024 ')).toBeNull();
		expect(portProblem('65535')).toBeNull();
		expect(portProblem('1023')).toBe('range');
		expect(portProblem('65536')).toBe('range');
		expect(portProblem('47831')).toBe('companion');
		expect(portProblem('')).toBe('number');
		expect(portProblem('80a')).toBe('number');
		expect(portProblem('-5')).toBe('number');
		expect(portProblem('4.5')).toBe('number');
	});
});

describe('maskUrl', () => {
	it('hides the token and keeps the address', () => {
		expect(maskUrl('http://192.168.1.5:47832/?token=s3cr3t')).toBe('http://192.168.1.5:47832/?token=••••••••');
		expect(maskUrl('http://192.168.1.5:47832/?token=s3cr3t')).not.toContain('s3cr3t');
	});
	it('leaves token-less URLs and fully masks garbage', () => {
		expect(maskUrl('http://10.0.0.2:47832/')).toBe('http://10.0.0.2:47832/');
		expect(maskUrl('not a url ?token=abc')).toBe('••••••••');
	});
});

describe('QR data URL', () => {
	it('base64-encodes UTF-8 SVG markup for an <img>', () => {
		const svg = '<svg xmlns="http://www.w3.org/2000/svg"><title>ö</title></svg>';
		const url = svgDataUrl(svg);
		expect(url.startsWith('data:image/svg+xml;base64,')).toBe(true);
		const decoded = new TextDecoder().decode(Uint8Array.from(atob(url.split(',')[1]), (c) => c.charCodeAt(0)));
		expect(decoded).toBe(svg);
		expect(base64Utf8('abc')).toBe('YWJj');
	});
});

describe('update states', () => {
	it('install only when available/ready; busy while checking/downloading', () => {
		expect(canInstall('available')).toBe(true);
		expect(canInstall('ready')).toBe(true);
		expect(canInstall('idle')).toBe(false);
		expect(canInstall('unsupported')).toBe(false);
		expect(updateBusy('checking')).toBe(true);
		expect(updateBusy('downloading')).toBe(true);
		expect(updateBusy('error')).toBe(false);
	});
});
