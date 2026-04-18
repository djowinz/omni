(function () {
  'use strict';
  const REGISTRY = new Map();
  const ID_INDEX = new Map();
  const TRUSTED = __OMNI_VIEW_TRUSTED__;

  function parsePrecision(raw) {
    const n = parseInt(raw || '0', 10);
    if (!Number.isFinite(n) || n < 0 || n > 6) return 0;
    return n;
  }

  function parseThreshold(raw) {
    if (raw === null || raw === undefined || raw === '') return null;
    const n = parseFloat(raw);
    return Number.isFinite(n) ? n : null;
  }

  function scan() {
    REGISTRY.clear();
    ID_INDEX.clear();
    const sensed = document.querySelectorAll('[data-sensor]');
    for (const el of sensed) {
      const path = el.getAttribute('data-sensor');
      if (!path) continue;
      const entry = {
        el: el,
        target: el.getAttribute('data-sensor-target') || 'text',
        format: el.getAttribute('data-sensor-format') || 'raw',
        precision: parsePrecision(el.getAttribute('data-sensor-precision')),
        warn: parseThreshold(el.getAttribute('data-sensor-threshold-warn')),
        crit: parseThreshold(el.getAttribute('data-sensor-threshold-critical')),
      };
      if (!REGISTRY.has(path)) REGISTRY.set(path, []);
      REGISTRY.get(path).push(entry);
    }
    const ided = document.querySelectorAll('[data-omni-id]');
    for (const el of ided) {
      ID_INDEX.set(el.getAttribute('data-omni-id'), el);
    }
  }

  function formatValue(raw, format, precision) {
    if (typeof raw !== 'number' || !Number.isFinite(raw)) return 'N/A';
    switch (format) {
      case 'percent': {
        const v = raw <= 1 ? raw * 100 : raw;
        return v.toFixed(precision) + '%';
      }
      case 'bytes': {
        const units = ['B', 'KB', 'MB', 'GB', 'TB'];
        let v = raw;
        let i = 0;
        while (v >= 1024 && i < units.length - 1) {
          v /= 1024;
          i++;
        }
        return v.toFixed(precision) + ' ' + units[i];
      }
      case 'temperature':
        return raw.toFixed(precision) + '\u00B0C';
      case 'frequency': {
        if (raw >= 1_000_000_000) return (raw / 1_000_000_000).toFixed(precision) + ' GHz';
        if (raw >= 1_000_000) return (raw / 1_000_000).toFixed(precision) + ' MHz';
        if (raw >= 1_000) return (raw / 1_000).toFixed(precision) + ' kHz';
        return raw.toFixed(precision) + ' Hz';
      }
      case 'raw':
      default:
        return raw.toFixed(precision);
    }
  }

  const CLASS_RE = /^[a-zA-Z0-9_\-\s]*$/;
  const IDENT_RE = /^[a-zA-Z_][a-zA-Z0-9_\-]*$/;

  function applyTarget(el, target, formatted) {
    if (target === 'text') {
      if (el.textContent !== formatted) el.textContent = formatted;
      return;
    }
    if (target === 'class') {
      if (!CLASS_RE.test(formatted)) return;
      if (el.className !== formatted) el.className = formatted;
      return;
    }
    if (target.startsWith('attr:')) {
      const name = target.slice(5);
      if (!IDENT_RE.test(name)) return;
      if (el.getAttribute(name) !== formatted) el.setAttribute(name, formatted);
      return;
    }
    if (target.startsWith('style-var:')) {
      const name = target.slice(10);
      if (!IDENT_RE.test(name)) return;
      el.style.setProperty('--' + name, formatted);
      return;
    }
  }

  function applyThresholds(el, raw, warn, crit) {
    if (typeof raw !== 'number' || !Number.isFinite(raw)) return;
    const critOn = crit !== null && raw >= crit;
    const warnOn = !critOn && warn !== null && raw >= warn;
    el.classList.toggle('sensor-critical', critOn);
    el.classList.toggle('sensor-warn', warnOn);
  }

  window.__omni_update = function (values) {
    if (!values) return;
    for (const [path, entries] of REGISTRY) {
      const raw = values[path];
      if (raw === undefined || raw === null) continue;
      for (const entry of entries) {
        const formatted = formatValue(raw, entry.format, entry.precision);
        applyTarget(entry.el, entry.target, formatted);
        applyThresholds(entry.el, raw, entry.warn, entry.crit);
      }
    }
  };

  window.__omni_set_classes = function (diff) {
    if (!diff) return;
    for (const id in diff) {
      const el = ID_INDEX.get(id);
      if (!el) continue;
      const next = diff[id];
      if (typeof next === 'string' && el.className !== next) el.className = next;
    }
  };

  window.__omni_set_text = function (diff) {
    if (!diff) return;
    for (const id in diff) {
      const el = ID_INDEX.get(id);
      if (!el) continue;
      const text = diff[id];
      for (const n of el.childNodes) {
        if (n.nodeType === 3 && n.textContent !== text) {
          n.textContent = text;
          break;
        }
      }
    }
  };

  window.__omni_set_attrs = function (diff) {
    if (!diff) return;
    for (const id in diff) {
      const el = ID_INDEX.get(id);
      if (!el) continue;
      const attrs = diff[id];
      for (const name in attrs) {
        el.setAttribute(name, attrs[name]);
      }
    }
  };

  window.__omni_set_theme = function (vars) {
    if (!vars) return;
    const root = document.documentElement;
    for (const k in vars) {
      if (!IDENT_RE.test(k)) continue;
      root.style.setProperty('--' + k, String(vars[k]));
    }
  };

  window.__omni_rescan = scan;

  if (!TRUSTED) {
    const NET = [
      'fetch',
      'XMLHttpRequest',
      'WebSocket',
      'EventSource',
      'RTCPeerConnection',
      'Worker',
      'SharedWorker',
    ];
    for (const name of NET) {
      try {
        delete window[name];
      } catch (e) {}
      try {
        Object.defineProperty(window, name, {
          value: undefined,
          configurable: false,
          writable: false,
        });
      } catch (e) {}
    }
    try {
      delete navigator.sendBeacon;
    } catch (e) {}
    try {
      delete navigator.serviceWorker;
    } catch (e) {}
    window.eval = function () {
      throw new Error('eval disabled');
    };
    window.Function = function () {
      throw new Error('Function disabled');
    };
    try {
      Object.freeze(window.location);
    } catch (e) {}
    try {
      delete window.localStorage;
    } catch (e) {}
    try {
      delete window.sessionStorage;
    } catch (e) {}
    try {
      delete window.indexedDB;
    } catch (e) {}
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', scan);
  } else {
    scan();
  }
})();
