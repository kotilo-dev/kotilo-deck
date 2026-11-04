/* Kotilo Deck widget SDK, plugin API v1 (docs/plugin-api.md). Served by the deck at
 * <scheme>/_sdk/kotilo-widget.js; plugins load it with
 * <script src="../_sdk/kotilo-widget.js"></script>. Plain ES2020, no build step. */
(function () {
	'use strict';
	if (window.kotilo) return;

	var host = window.parent;
	var handlers = {}; // event type -> [fn]
	var topics = {}; // topic -> [fn]
	var ready = false; // true after the first `init`
	var lastInit = null;
	var applied = []; // --k-* names set by the last theme
	var TOKEN = /^[a-z0-9-]{1,40}$/;

	function send(msg) {
		if (!host || host === window) return;
		msg.kotilo = 1;
		// The iframe is sandboxed (opaque origin) and sends nothing secret, so '*' is fine;
		// the deck checks `event.source` on its side.
		host.postMessage(msg, '*');
	}

	function call(list, value) {
		if (!list) return;
		list.slice().forEach(function (fn) {
			try {
				fn(value);
			} catch (e) {
				console.error('[kotilo]', e);
			}
		});
	}

	function applyTheme(theme) {
		if (!theme || typeof theme !== 'object') return;
		var style = document.documentElement.style;
		var next = [];
		var tokens = theme.tokens && typeof theme.tokens === 'object' ? theme.tokens : {};
		Object.keys(tokens).forEach(function (key) {
			var value = tokens[key];
			if (!TOKEN.test(key) || typeof value !== 'string') return;
			style.setProperty('--k-' + key, value);
			next.push('--k-' + key);
		});
		applied.forEach(function (name) {
			if (next.indexOf(name) < 0) style.removeProperty(name);
		});
		applied = next;
		if (theme.variant === 'dark' || theme.variant === 'light') {
			style.setProperty('color-scheme', theme.variant);
		}
		kotilo.theme = theme;
	}

	function sendSubscribe(topic) {
		send({ type: 'subscribe', topic: topic });
	}

	var kotilo = {
		apiVersion: 1,
		/** Last known "WxH". */
		size: null,
		/** Last known instance settings (defaults merged by the deck). */
		settings: {},
		theme: null,
		locale: null,
		editing: false,
		available: true,
		visible: true,

		/** Listens for 'init' | 'size' | 'settings' | 'theme' | 'editing' | 'visibility'.
		 *  Returns a function that removes the listener. A late 'init' listener is called
		 *  with the init message right away. */
		on: function (type, fn) {
			if (typeof fn !== 'function') return function () {};
			(handlers[type] = handlers[type] || []).push(fn);
			if (type === 'init' && lastInit) {
				var init = lastInit;
				setTimeout(function () {
					fn(init);
				}, 0);
			}
			return function () {
				kotilo.off(type, fn);
			};
		},
		off: function (type, fn) {
			var list = handlers[type];
			if (list) handlers[type] = list.filter(function (f) { return f !== fn; });
		},

		/** Receives `data` payloads for a topic declared in manifest.json `topics`. */
		subscribe: function (topic, fn) {
			if (typeof topic !== 'string' || typeof fn !== 'function') return function () {};
			var first = !topics[topic] || topics[topic].length === 0;
			(topics[topic] = topics[topic] || []).push(fn);
			if (first && ready) sendSubscribe(topic);
			return function () {
				kotilo.unsubscribe(topic, fn);
			};
		},
		/** Removes `fn` (or every listener when omitted); the last one stops the pushes. */
		unsubscribe: function (topic, fn) {
			var list = topics[topic];
			if (!list || list.length === 0) return;
			topics[topic] = fn ? list.filter(function (f) { return f !== fn; }) : [];
			if (topics[topic].length === 0 && ready) send({ type: 'unsubscribe', topic: topic });
		}
	};

	window.addEventListener('message', function (e) {
		if (e.source !== host || host === window) return;
		var d = e.data;
		if (!d || typeof d !== 'object' || d.kotilo !== 1 || typeof d.type !== 'string') return;
		switch (d.type) {
			case 'init':
				if (typeof d.size === 'string') kotilo.size = d.size;
				if (d.settings && typeof d.settings === 'object') kotilo.settings = d.settings;
				if (typeof d.locale === 'string') kotilo.locale = d.locale;
				kotilo.editing = d.editing === true;
				kotilo.available = d.available !== false;
				applyTheme(d.theme);
				lastInit = d;
				if (!ready) {
					ready = true;
					// Subscriptions made before the deck answered go out now.
					Object.keys(topics).forEach(function (t) {
						if (topics[t].length) sendSubscribe(t);
					});
				}
				call(handlers.init, d);
				break;
			case 'size':
				if (typeof d.size !== 'string') return;
				kotilo.size = d.size;
				call(handlers.size, d.size);
				break;
			case 'settings':
				if (!d.settings || typeof d.settings !== 'object') return;
				kotilo.settings = d.settings;
				call(handlers.settings, d.settings);
				break;
			case 'theme':
				applyTheme(d.theme);
				call(handlers.theme, d.theme);
				break;
			case 'editing':
				kotilo.editing = d.editing === true;
				call(handlers.editing, kotilo.editing);
				break;
			case 'visibility':
				kotilo.visible = d.visible !== false;
				call(handlers.visibility, kotilo.visible);
				break;
			case 'data':
				if (typeof d.topic === 'string') call(topics[d.topic], d.payload);
				break;
			default:
			// Unknown types are ignored (additive versioning).
		}
	});

	window.kotilo = kotilo;
	send({ type: 'ready' });
})();
