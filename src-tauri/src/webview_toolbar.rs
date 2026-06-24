use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use tauri::Manager;

const BROWSER_BRIDGE_SCRIPT: &str = r#"
(function () {
  if (window.__selahBrowserBridgeInstalled) return;
  window.__selahBrowserBridgeInstalled = true;

  function normalizeText(value) {
    return String(value || '')
      .replace(/\u00A0/g, ' ')
      .replace(/[ \t]+\n/g, '\n')
      .replace(/\n[ \t]+/g, '\n')
      .replace(/[ \t]{2,}/g, ' ')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
  }

  function textOf(el) {
    if (!el) return '';
    return normalizeText(el.innerText || el.textContent || '');
  }

  function isVisible(el) {
    if (!el || !el.isConnected) return false;
    if (el.hidden || el.getAttribute('aria-hidden') === 'true') return false;
    try {
      var style = window.getComputedStyle(el);
      if (!style) return true;
      if (style.display === 'none' || style.visibility === 'hidden') return false;
      if (parseFloat(style.opacity || '1') === 0) return false;
    } catch (_) {}
    var rect = typeof el.getBoundingClientRect === 'function' ? el.getBoundingClientRect() : null;
    if (!rect) return true;
    return rect.width > 0 || rect.height > 0;
  }

  function isJunk(el) {
    if (!el || !(el instanceof Element)) return false;
    return !!el.closest(
      'script,style,noscript,template,svg,canvas,iframe,nav,header,footer,aside,' +
      '[role="navigation"],[role="banner"],[role="contentinfo"],[role="dialog"],[aria-modal="true"],' +
      '.cookie,.cookies,.consent,.ads,.ad,.advertisement,.breadcrumb,.sidebar,.drawer,.modal,.popup'
    );
  }

  function isControlJunk(el) {
    if (!el || !(el instanceof Element)) return false;
    return !!el.closest(
      'script,style,noscript,template,iframe,[role="dialog"],[aria-modal="true"],' +
      '.cookie,.cookies,.consent,.ads,.ad,.advertisement,.modal,.popup'
    );
  }

  function pushUnique(items, seen, raw, maxChars) {
    var value = normalizeText(raw);
    if (!value) return;
    if (maxChars && value.length > maxChars) value = value.slice(0, maxChars) + '…';
    var key = value.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    items.push(value);
  }

  function scoreRoot(el) {
    if (!el || !isVisible(el) || isJunk(el)) return -1;
    var text = textOf(el);
    if (text.length < 80) return -1;
    var blocks = el.querySelectorAll('p,li,tr').length;
    var headings = el.querySelectorAll('h1,h2,h3').length;
    var controls = el.querySelectorAll('button,input,textarea,select').length;
    return Math.min(text.length, 7000) + blocks * 40 + headings * 120 + controls * 25;
  }

  function pickContentRoot(doc) {
    var preferred = [
      'main',
      'article',
      '[role="main"]',
      '.main',
      '#main',
      '.content',
      '#content',
      '.article',
      '.post',
      '.entry',
      'form'
    ];
    for (var i = 0; i < preferred.length; i++) {
      var candidate = doc.querySelector(preferred[i]);
      if (!candidate || !isVisible(candidate) || isJunk(candidate)) continue;
      if (textOf(candidate).length >= 120 || candidate.querySelector('input,textarea,select,button')) {
        return candidate;
      }
    }

    var candidates = Array.from(doc.querySelectorAll('main,article,section,form,div')).slice(0, 500);
    var best = doc.body || doc.documentElement;
    var bestScore = scoreRoot(best);
    for (var j = 0; j < candidates.length; j++) {
      var el = candidates[j];
      var score = scoreRoot(el);
      if (score > bestScore) {
        best = el;
        bestScore = score;
      }
    }
    return best || doc.body || doc.documentElement;
  }

  function collectHeadings(root) {
    var out = [];
    var seen = new Set();
    var nodes = root ? root.querySelectorAll('h1,h2,h3,h4,h5,h6') : [];
    for (var i = 0; i < nodes.length && out.length < 10; i++) {
      var el = nodes[i];
      if (!isVisible(el) || isControlJunk(el)) continue;
      pushUnique(out, seen, textOf(el), 140);
    }
    return out;
  }

  function collectLinks(root) {
    var out = [];
    var seen = new Set();
    var nodes = root ? root.querySelectorAll('a[href]') : [];
    for (var i = 0; i < nodes.length && out.length < 30; i++) {
      var el = nodes[i];
      if (!isVisible(el) || isControlJunk(el)) continue;
      var text = candidateText(el);
      var href = normalizeText(el.href || el.getAttribute('href') || '');
      if (!href || href === '#' || href.startsWith('javascript:')) continue;
      var key = (text + '|' + href).toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({
        text: text.length > 120 ? text.slice(0, 120) + '…' : text,
        url: href.length > 240 ? href.slice(0, 240) + '…' : href,
        rect: elementRect(el)
      });
    }
    return out;
  }

  function buttonKind(el) {
    var kind = el.getAttribute('type') || el.getAttribute('role') || el.tagName || '';
    return normalizeText(kind).toLowerCase();
  }

  function collectButtons(root) {
    var out = [];
    var seen = new Set();
    var nodes = root
      ? root.querySelectorAll('button,[role="button"],input[type="button"],input[type="submit"],input[type="reset"]')
      : [];
    for (var i = 0; i < nodes.length && out.length < 10; i++) {
      var el = nodes[i];
      if (!isVisible(el) || isControlJunk(el)) continue;
      var text = candidateText(el);
      if (!text) continue;
      var key = (text + '|' + buttonKind(el)).toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({
        text: text.length > 120 ? text.slice(0, 120) + '…' : text,
        type: buttonKind(el),
        rect: elementRect(el)
      });
    }
    return out;
  }

  function findLabelText(el) {
    if (!el) return '';
    var parentLabel = el.closest('label');
    if (parentLabel) {
      var parentText = textOf(parentLabel);
      if (parentText) return parentText;
    }
    var id = el.id || '';
    if (id && window.CSS && typeof window.CSS.escape === 'function') {
      var label = document.querySelector('label[for="' + window.CSS.escape(id) + '"]');
      if (label) {
        var labelText = textOf(label);
        if (labelText) return labelText;
      }
    }
    return normalizeText(
      el.getAttribute('aria-label') ||
      el.getAttribute('placeholder') ||
      el.getAttribute('name') ||
      ''
    );
  }

  function collectInputs(root) {
    var out = [];
    var seen = new Set();
    var nodes = root ? root.querySelectorAll('input,textarea,select') : [];
    for (var i = 0; i < nodes.length && out.length < 10; i++) {
      var el = nodes[i];
      if (!isVisible(el) || isControlJunk(el)) continue;
      var tag = (el.tagName || '').toLowerCase();
      var type = normalizeText(el.getAttribute('type') || tag).toLowerCase();
      if (type === 'hidden') continue;
      var label = findLabelText(el);
      var name = normalizeText(el.getAttribute('name') || '');
      var placeholder = normalizeText(el.getAttribute('placeholder') || '');
      var value = normalizeText(el.value || '');
      var key = (label + '|' + name + '|' + type).toLowerCase();
      if (!label && !name && !placeholder && !value) continue;
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({
        label: label.length > 120 ? label.slice(0, 120) + '…' : label,
        type: type,
        name: name.length > 80 ? name.slice(0, 80) + '…' : name,
        placeholder: placeholder.length > 120 ? placeholder.slice(0, 120) + '…' : placeholder,
        value: value.length > 120 ? value.slice(0, 120) + '…' : value,
        rect: elementRect(el),
        required: !!el.required,
        disabled: !!el.disabled
      });
    }
    return out;
  }

  function collectContent(root) {
    var blocks = [];
    var seen = new Set();
    var totalChars = 0;
    var nodes = root ? root.querySelectorAll('h1,h2,h3,h4,h5,h6,p,li,blockquote,pre,tr') : [];

    function pushLine(line) {
      var value = normalizeText(line);
      if (!value) return;
      var key = value.toLowerCase();
      if (seen.has(key)) return;
      if (blocks.length > 0 && totalChars + value.length > 12000) return;
      seen.add(key);
      blocks.push(value);
      totalChars += value.length + 1;
    }

    for (var i = 0; i < nodes.length; i++) {
      if (totalChars >= 12000) break;
      var el = nodes[i];
      if (!isVisible(el) || isJunk(el)) continue;
      var tag = (el.tagName || '').toLowerCase();
      if (tag === 'tr') {
        var cells = Array.from(el.querySelectorAll('th,td'))
          .map(function (cell) { return textOf(cell); })
          .filter(Boolean);
        if (cells.length) pushLine(cells.join(' | '));
        continue;
      }
      var text = textOf(el);
      if (!text) continue;
      if (/^h[1-6]$/.test(tag)) {
        var level = Math.min(Math.max(parseInt(tag.slice(1), 10) || 2, 1), 4);
        pushLine(Array(level + 1).join('#') + ' ' + text);
      } else if (tag === 'li') {
        pushLine('- ' + text);
      } else if (tag === 'blockquote') {
        pushLine('> ' + text);
      } else {
        pushLine(text);
      }
    }

    if (!blocks.length) {
      return textOf(root).slice(0, 12000);
    }
    return blocks.join('\n');
  }

  function candidateText(el) {
    if (!el) return '';
    var imgAlt = '';
    try {
      var img = el.querySelector && el.querySelector('img[alt]');
      imgAlt = img ? img.getAttribute('alt') || '' : '';
    } catch (_) {}
    return normalizeText(
      textOf(el) ||
      el.getAttribute('aria-label') ||
      el.getAttribute('title') ||
      el.getAttribute('value') ||
      el.getAttribute('alt') ||
      imgAlt ||
      ''
    );
  }

  function matchScore(query, text) {
    var q = normalizeText(query).toLowerCase();
    var t = normalizeText(text).toLowerCase();
    if (!q || !t) return 0;
    if (t === q) return 1200;
    if (t.startsWith(q)) return 900 - Math.min(240, t.length - q.length);
    if (t.includes(q)) return 700 - Math.min(220, t.length - q.length);
    if (q.includes(t) && t.length >= 2) return 520 - Math.min(180, q.length - t.length);
    return 0;
  }

  function dedupeElements(items) {
    var seen = new Set();
    var out = [];
    for (var i = 0; i < items.length; i++) {
      var el = items[i];
      if (!el || seen.has(el)) continue;
      seen.add(el);
      out.push(el);
    }
    return out;
  }

  function pickByIndex(items, index) {
    if (!items.length) return null;
    var safe = Math.max(0, Math.min(Number(index || 0), items.length - 1));
    return items[safe];
  }

  function elementRect(el) {
    if (!el || typeof el.getBoundingClientRect !== 'function') return null;
    var rect = el.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: Math.round(rect.left),
      y: Math.round(rect.top),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
      centerX: Math.round(rect.left + rect.width / 2),
      centerY: Math.round(rect.top + rect.height / 2)
    };
  }

  function currentPageMeta() {
    return {
      url: String(window.location.href || ''),
      title: normalizeText(document.title || ''),
      viewport: {
        width: Math.round(window.innerWidth || 0),
        height: Math.round(window.innerHeight || 0),
        scrollX: Math.round(window.scrollX || 0),
        scrollY: Math.round(window.scrollY || 0),
        scrollHeight: Math.round(document.documentElement?.scrollHeight || document.body?.scrollHeight || 0)
      }
    };
  }

  function coreInvoke() {
    return window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke || null;
  }

  function openPopupThroughApp(rawUrl, title) {
    var invoke = coreInvoke();
    if (!invoke || !rawUrl) return false;
    var resolved = null;
    try {
      resolved = new URL(String(rawUrl || ''), window.location.href);
    } catch (_) {
      return false;
    }
    if (resolved.protocol !== 'http:' && resolved.protocol !== 'https:') return false;
    invoke('open_external_url', {
      url: resolved.toString(),
      title: normalizeText(title || resolved.host || resolved.toString())
    }).catch(function (error) {
      console.warn('open_external_url failed for popup:', error);
    });
    return true;
  }

  try {
    var originalWindowOpen = typeof window.open === 'function' ? window.open.bind(window) : null;
    window.open = function (rawUrl, target, features) {
      if (rawUrl && openPopupThroughApp(rawUrl, document.title || '')) {
        return null;
      }
      return originalWindowOpen ? originalWindowOpen(rawUrl, target, features) : null;
    };
  } catch (_) {}

  function elementSummary(el) {
    if (!el) return {};
    return {
      tag: String(el.tagName || '').toLowerCase(),
      text: candidateText(el).slice(0, 160),
      name: normalizeText(el.getAttribute('name') || '').slice(0, 80),
      type: normalizeText(el.getAttribute('type') || el.getAttribute('role') || '').slice(0, 40),
      href: normalizeText(el.href || el.getAttribute('href') || '').slice(0, 240)
    };
  }

  function clickableSelector() {
    return 'a[href],button,[role="button"],[role="link"],[role="tab"],' +
      '[role="menuitem"],[role="menuitemcheckbox"],[role="menuitemradio"],' +
      '[role="option"],[role="checkbox"],[role="radio"],[role="switch"],[role="treeitem"],' +
      'summary,input[type="button"],input[type="submit"],input[type="reset"],' +
      'input[type="checkbox"],input[type="radio"],label,[onclick],[tabindex]:not([tabindex="-1"])';
  }

  function isClickable(el) {
    if (!el || !isVisible(el) || isJunk(el)) return false;
    if (el.matches && el.matches(clickableSelector())) return true;
    var tag = String(el.tagName || '').toLowerCase();
    return tag === 'a' || tag === 'button';
  }

  function closestClickable(el) {
    if (!el || !el.closest) return null;
    return el.closest(clickableSelector());
  }

  function findClickable(action) {
    var index = Number(action.index || 0);
    if (action.selector) {
      var direct = Array.from(document.querySelectorAll(String(action.selector)))
        .map(function (el) { return closestClickable(el) || el; })
        .filter(function (el) { return isClickable(el); });
      direct = dedupeElements(direct);
      return { element: pickByIndex(direct, index), matches: direct.length };
    }

    var textQuery = normalizeText(action.text || '');
    var hrefQuery = normalizeText(action.hrefContains || '').toLowerCase();
    var ranked = Array.from(document.querySelectorAll(clickableSelector()))
      .filter(function (el) { return isClickable(el); })
      .map(function (el) {
        var score = 0;
        if (textQuery) score += matchScore(textQuery, candidateText(el));
        if (hrefQuery) {
          var href = normalizeText(el.href || el.getAttribute('href') || '').toLowerCase();
          if (href.includes(hrefQuery)) score += href === hrefQuery ? 1300 : 850;
        }
        return { el: el, score: score };
      })
      .filter(function (item) { return item.score > 0; })
      .sort(function (a, b) { return b.score - a.score; });

    var matches = ranked.map(function (item) { return item.el; });
    return { element: pickByIndex(matches, index), matches: matches.length };
  }

  function fieldSelector() {
    return 'input:not([type="hidden"]),textarea,select,[contenteditable="true"]';
  }

  function isFillable(el) {
    if (!el || !isVisible(el) || isJunk(el)) return false;
    if (el.matches && el.matches(fieldSelector())) return true;
    return !!el.isContentEditable;
  }

  function fieldSummaryText(el) {
    return normalizeText([
      findLabelText(el),
      el.getAttribute('name') || '',
      el.getAttribute('placeholder') || '',
      el.getAttribute('aria-label') || '',
      el.getAttribute('title') || ''
    ].filter(Boolean).join(' | '));
  }

  function findField(action, options) {
    var index = Number(action.index || 0);
    var allowSelectOnly = !!(options && options.selectOnly);
    if (action.selector) {
      var direct = Array.from(document.querySelectorAll(String(action.selector)))
        .filter(function (el) { return isFillable(el); });
      if (allowSelectOnly) {
        direct = direct.filter(function (el) { return String(el.tagName || '').toLowerCase() === 'select'; });
      }
      return { element: pickByIndex(direct, index), matches: direct.length };
    }

    var labelQuery = normalizeText(action.label || '');
    var ranked = Array.from(document.querySelectorAll(fieldSelector()))
      .filter(function (el) { return isFillable(el); })
      .filter(function (el) {
        return !allowSelectOnly || String(el.tagName || '').toLowerCase() === 'select';
      })
      .map(function (el) {
        return { el: el, score: matchScore(labelQuery, fieldSummaryText(el)) };
      })
      .filter(function (item) { return labelQuery ? item.score > 0 : true; })
      .sort(function (a, b) { return b.score - a.score; });

    var matches = ranked.map(function (item) { return item.el; });
    return { element: pickByIndex(matches, index), matches: matches.length };
  }

  function setNativeValue(el, value) {
    var proto = null;
    if (window.HTMLInputElement && el instanceof window.HTMLInputElement) {
      proto = window.HTMLInputElement.prototype;
    } else if (window.HTMLTextAreaElement && el instanceof window.HTMLTextAreaElement) {
      proto = window.HTMLTextAreaElement.prototype;
    }
    var desc = proto && Object.getOwnPropertyDescriptor(proto, 'value');
    if (desc && typeof desc.set === 'function') {
      desc.set.call(el, value);
    } else {
      el.value = value;
    }
  }

  function dispatchInputEvents(el) {
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function setFieldValue(el, rawValue) {
    var value = String(rawValue == null ? '' : rawValue);
    if (el.isContentEditable) {
      el.focus();
      el.textContent = value;
      dispatchInputEvents(el);
      return;
    }

    var tag = String(el.tagName || '').toLowerCase();
    var type = normalizeText(el.getAttribute('type') || '').toLowerCase();
    el.focus();

    if (type === 'checkbox' || type === 'radio') {
      var truthy = ['true', '1', 'yes', 'on', 'checked', '选中', '勾选', 'はい'];
      var shouldCheck = truthy.indexOf(value.trim().toLowerCase()) >= 0;
      el.checked = shouldCheck;
      dispatchInputEvents(el);
      return;
    }

    if (tag === 'select') {
      selectOptionValue(el, value);
      return;
    }

    setNativeValue(el, value);
    dispatchInputEvents(el);
  }

  function selectOptionValue(el, rawValue) {
    var value = normalizeText(rawValue);
    if (!el || String(el.tagName || '').toLowerCase() !== 'select') {
      throw new Error('Target is not a <select> element');
    }
    var options = Array.from(el.options || []);
    var lower = value.toLowerCase();
    var bestIndex = -1;
    var bestScore = 0;
    for (var i = 0; i < options.length; i++) {
      var option = options[i];
      var optionText = normalizeText(option.text || option.label || '');
      var optionValue = normalizeText(option.value || '');
      var score = Math.max(matchScore(value, optionText), matchScore(value, optionValue));
      if (optionValue.toLowerCase() === lower || optionText.toLowerCase() === lower) {
        score += 800;
      }
      if (score > bestScore) {
        bestScore = score;
        bestIndex = i;
      }
    }
    if (bestIndex < 0) {
      throw new Error('No matching option found');
    }
    el.focus();
    el.selectedIndex = bestIndex;
    dispatchInputEvents(el);
  }

  function performClick(el) {
    if (!el) throw new Error('No clickable element found');
    // A target="_blank" link clicked inside the webview opens a detached/blocked
    // window, so the destination never appears and automation looks like it
    // "did nothing". Force same-tab navigation so the page actually loads in the
    // current webview (the agent keeps the same target and the user sees it).
    // JS window.open() calls are still routed to a Copilot tab via the override.
    try {
      var navAnchor = (el.tagName && String(el.tagName).toLowerCase() === 'a')
        ? el
        : (el.closest ? el.closest('a[href]') : null);
      if (navAnchor) {
        var navTarget = String(navAnchor.getAttribute('target') || '').toLowerCase();
        if (navTarget === '_blank' || navTarget === '_new') {
          navAnchor.setAttribute('target', '_self');
        }
      }
    } catch (e) {}
    try { if (typeof el.focus === 'function') el.focus(); } catch (e) {}
    var rect = el.getBoundingClientRect();
    var cx = Math.round(rect.left + rect.width / 2);
    var cy = Math.round(rect.top + rect.height / 2);
    var opts = { bubbles: true, cancelable: true, view: window, clientX: cx, clientY: cy, button: 0, buttons: 1 };
    var PE = window.PointerEvent;
    var pOpts = Object.assign({ pointerId: 1, pointerType: 'mouse', isPrimary: true }, opts);
    function fireP(type) { if (PE) { try { el.dispatchEvent(new PE(type, pOpts)); } catch (e) {} } }
    function fireM(type, extra) {
      try { el.dispatchEvent(new MouseEvent(type, Object.assign({}, opts, extra || {}))); } catch (e) {}
    }
    // Full pointer + mouse sequence so controls that only listen for pointer
    // events (custom buttons, SPA widgets) also fire, not just legacy mouse ones.
    fireP('pointerover'); fireM('mouseover');
    fireP('pointerenter');
    fireP('pointerdown'); fireM('mousedown');
    fireP('pointerup'); fireM('mouseup', { buttons: 0 });
    if (typeof el.click === 'function') { el.click(); } else { fireM('click', { buttons: 0 }); }
  }

  function viewportPoint(action, prefix) {
    var xKey = prefix ? prefix + 'X' : 'x';
    var yKey = prefix ? prefix + 'Y' : 'y';
    var x = Number(action[xKey] ?? action[(prefix || '') + 'x']);
    var y = Number(action[yKey] ?? action[(prefix || '') + 'y']);
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
      throw new Error('Mouse coordinates are required');
    }
    x = Math.max(0, Math.min(Math.round(x), Math.max(0, window.innerWidth || 0)));
    y = Math.max(0, Math.min(Math.round(y), Math.max(0, window.innerHeight || 0)));
    return { x: x, y: y };
  }

  function dispatchMouse(type, point, button) {
    var el = document.elementFromPoint(point.x, point.y) || document.body || document.documentElement;
    if (!el) throw new Error('No element at mouse coordinates');
    el.dispatchEvent(new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      view: window,
      clientX: point.x,
      clientY: point.y,
      button: button || 0,
      buttons: type === 'mouseup' ? 0 : 1
    }));
    return el;
  }

  function dispatchPointer(type, point) {
    if (!window.PointerEvent) return;
    var el = document.elementFromPoint(point.x, point.y) || document.body || document.documentElement;
    if (!el) return;
    try {
      el.dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, view: window,
        clientX: point.x, clientY: point.y, button: 0,
        buttons: type === 'pointerup' ? 0 : 1,
        pointerId: 1, pointerType: 'mouse', isPrimary: true
      }));
    } catch (e) {}
  }

  function performMouseClick(action) {
    var point = viewportPoint(action, '');
    dispatchPointer('pointerover', point);
    var downEl = dispatchMouse('mouseover', point, 0);
    dispatchMouse('mousemove', point, 0);
    dispatchPointer('pointerdown', point);
    dispatchMouse('mousedown', point, 0);
    dispatchPointer('pointerup', point);
    dispatchMouse('mouseup', point, 0);
    dispatchMouse('click', point, 0);
    return { point: point, element: elementSummary(downEl) };
  }

  async function performMouseDrag(action) {
    var from = viewportPoint(action, 'from');
    var to = viewportPoint(action, 'to');
    var steps = Math.max(2, Math.min(Number(action.steps || 8), 24));
    dispatchPointer('pointerover', from);
    var startEl = dispatchMouse('mouseover', from, 0);
    dispatchMouse('mousemove', from, 0);
    dispatchPointer('pointerdown', from);
    dispatchMouse('mousedown', from, 0);
    for (var i = 1; i <= steps; i++) {
      var point = {
        x: Math.round(from.x + (to.x - from.x) * i / steps),
        y: Math.round(from.y + (to.y - from.y) * i / steps)
      };
      dispatchPointer('pointermove', point);
      dispatchMouse('mousemove', point, 0);
      await wait(16);
    }
    dispatchPointer('pointerup', to);
    dispatchMouse('mouseup', to, 0);
    return { from: from, to: to, element: elementSummary(startEl) };
  }

  function normalizeKeyName(rawKey) {
    var key = normalizeText(rawKey);
    var lower = key.toLowerCase();
    var map = {
      enter: 'Enter',
      tab: 'Tab',
      escape: 'Escape',
      esc: 'Escape',
      backspace: 'Backspace',
      delete: 'Delete',
      arrowup: 'ArrowUp',
      arrowdown: 'ArrowDown',
      arrowleft: 'ArrowLeft',
      arrowright: 'ArrowRight',
      space: ' ',
      spacebar: ' ',
      pageup: 'PageUp',
      pagedown: 'PageDown',
      home: 'Home',
      end: 'End'
    };
    return map[lower] || key;
  }

  function performKeyPress(el, rawKey) {
    var key = normalizeKeyName(rawKey);
    var target = el || document.activeElement || document.body;
    if (!target) throw new Error('No target available for key press');
    if (typeof target.focus === 'function') target.focus();

    ['keydown', 'keypress', 'keyup'].forEach(function (type) {
      target.dispatchEvent(new KeyboardEvent(type, {
        key: key,
        bubbles: true,
        cancelable: true
      }));
    });

    if (key === 'Enter' && target.form && String(target.tagName || '').toLowerCase() !== 'textarea') {
      if (typeof target.form.requestSubmit === 'function') target.form.requestSubmit();
      else if (typeof target.form.submit === 'function') target.form.submit();
    }

    if ((key === ' ' || key === 'Enter') && isClickable(target) && typeof target.click === 'function') {
      target.click();
    }

    return key;
  }

  function wait(ms) {
    return new Promise(function (resolve) { setTimeout(resolve, ms); });
  }

  async function waitForCondition(action) {
    var timeoutMs = Math.max(200, Number(action.timeoutMs || 3000));
    var start = Date.now();
    var selector = normalizeText(action.selector || '');
    var text = normalizeText(action.text || '');
    while (Date.now() - start <= timeoutMs) {
      if (selector) {
        var matches = Array.from(document.querySelectorAll(String(action.selector)))
          .filter(function (el) { return isVisible(el) && !isJunk(el); });
        if (matches.length) {
          return {
            ok: true,
            action: 'wait_for',
            waitedMs: Date.now() - start,
            matches: matches.length,
            condition: selector,
            selector: selector,
            element: elementSummary(matches[0])
          };
        }
      }
      if (text) {
        var body = normalizeText((document.body && (document.body.innerText || document.body.textContent)) || '');
        if (body.toLowerCase().includes(text.toLowerCase())) {
          return {
            ok: true,
            action: 'wait_for',
            waitedMs: Date.now() - start,
            matches: 1,
            condition: text,
            textFound: text
          };
        }
      }
      await wait(150);
    }
    throw new Error('Timed out waiting for page condition');
  }

  window.__selahBrowserRunAction = async function (requestId, action) {
    try {
      var invoke = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
      if (!invoke) return;
      var payload = action || {};
      var kind = normalizeText(payload.kind || payload.action).toLowerCase();
      var report = function (result) {
        return invoke('browser_report_action_result', {
          report: {
            requestId: requestId,
            payload: Object.assign({}, currentPageMeta(), result || {})
          }
        });
      };

      if (!kind) {
        await report({ ok: false, error: 'Missing browser action kind' });
        return;
      }

      if (kind === 'click') {
        var clicked = findClickable(payload);
        if (!clicked.element) throw new Error('No matching clickable element found');
        var clickResult = {
          ok: true,
          action: 'click',
          matches: clicked.matches,
          selector: normalizeText(payload.selector || ''),
          textQuery: normalizeText(payload.text || ''),
          hrefContains: normalizeText(payload.hrefContains || ''),
          element: elementSummary(clicked.element)
        };
        var clickReport = report(clickResult);
        performClick(clicked.element);
        await clickReport;
        return;
      }

      if (kind === 'fill') {
        var filled = findField(payload, { selectOnly: false });
        if (!filled.element) throw new Error('No matching field found');
        setFieldValue(filled.element, payload.value || '');
        await report({
          ok: true,
          action: 'fill',
          matches: filled.matches,
          selector: normalizeText(payload.selector || ''),
          labelQuery: normalizeText(payload.label || ''),
          valuePreview: normalizeText(String(payload.value || '')).slice(0, 120),
          element: elementSummary(filled.element)
        });
        return;
      }

      if (kind === 'select_option') {
        var selected = findField(payload, { selectOnly: true });
        if (!selected.element) throw new Error('No matching select field found');
        selectOptionValue(selected.element, payload.value || '');
        await report({
          ok: true,
          action: 'select_option',
          matches: selected.matches,
          selector: normalizeText(payload.selector || ''),
          labelQuery: normalizeText(payload.label || ''),
          valuePreview: normalizeText(String(payload.value || '')).slice(0, 120),
          element: elementSummary(selected.element)
        });
        return;
      }

      if (kind === 'press') {
        var pressTarget = null;
        if (payload.selector) {
          pressTarget = document.querySelector(String(payload.selector));
        }
        if (pressTarget && !isVisible(pressTarget)) pressTarget = null;
        var pressResult = {
          ok: true,
          action: 'press',
          selector: normalizeText(payload.selector || ''),
          key: performKeyPress(pressTarget, payload.key || '')
        };
        if (pressTarget) {
          pressResult.element = elementSummary(pressTarget);
        }
        await report(pressResult);
        return;
      }

      if (kind === 'scroll') {
        var direction = normalizeText(payload.direction || 'down').toLowerCase();
        var amount = Math.max(80, Number(payload.amount || 900));
        if (payload.selector) {
          var scrollTarget = document.querySelector(String(payload.selector));
          if (!scrollTarget || !isVisible(scrollTarget)) throw new Error('No matching element to scroll into view');
          scrollTarget.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'auto' });
          await wait(60);
          await report({
            ok: true,
            action: 'scroll',
            selector: normalizeText(payload.selector || ''),
            direction: direction,
            element: elementSummary(scrollTarget),
            scrollY: Math.round(window.scrollY || 0)
          });
          return;
        }

        if (direction === 'top') window.scrollTo({ top: 0, behavior: 'auto' });
        else if (direction === 'bottom') window.scrollTo({ top: document.body ? document.body.scrollHeight : 999999, behavior: 'auto' });
        else if (direction === 'up') window.scrollBy({ top: -amount, behavior: 'auto' });
        else window.scrollBy({ top: amount, behavior: 'auto' });

        await wait(60);
        await report({
          ok: true,
          action: 'scroll',
          direction: direction,
          amount: amount,
          scrollY: Math.round(window.scrollY || 0)
        });
        return;
      }

      if (kind === 'mouse_click') {
        var mouseClick = performMouseClick(payload);
        await wait(80);
        await report({
          ok: true,
          action: 'mouse_click',
          x: mouseClick.point.x,
          y: mouseClick.point.y,
          element: mouseClick.element
        });
        return;
      }

      if (kind === 'mouse_drag') {
        var drag = await performMouseDrag(payload);
        await wait(80);
        await report({
          ok: true,
          action: 'mouse_drag',
          from: drag.from,
          to: drag.to,
          element: drag.element
        });
        return;
      }

      if (kind === 'wait_for') {
        var waited = await waitForCondition(payload);
        await report(waited);
        return;
      }

      throw new Error('Unsupported browser action: ' + kind);
    } catch (error) {
      try {
        var invoke = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
        if (!invoke) return;
        await invoke('browser_report_action_result', {
          report: {
            requestId: requestId,
            payload: Object.assign({}, currentPageMeta(), {
              ok: false,
              action: normalizeText(action && (action.kind || action.action) || '').toLowerCase(),
              error: normalizeText(error && (error.message || String(error)) || 'Browser action failed')
            })
          }
        });
      } catch (_) {}
    }
  };

  window.__selahBrowserExtractText = async function (requestId) {
    try {
      var invoke = window.__TAURI__?.core?.invoke || window.__TAURI_INTERNALS__?.invoke;
      if (!invoke) return;
      var doc = document;
      var title = (doc.title || '').trim();
      var root = pickContentRoot(doc);
      var bodyText = collectContent(root);
      if (!bodyText && doc.body) {
        bodyText = textOf(doc.body);
      }
      await invoke('browser_report_page_text', {
        report: {
          requestId: requestId,
          payload: {
            title: title,
            url: String(window.location.href || ''),
            viewport: currentPageMeta().viewport,
            text: bodyText,
            headings: collectHeadings(root),
            links: collectLinks(doc),
            buttons: collectButtons(doc),
            inputs: collectInputs(doc),
            contentSource: root && root.tagName ? String(root.tagName).toLowerCase() : 'document'
          }
        }
      });
    } catch (_) {}
  };
})();
"#;

static PAGE_TEXT_WAITERS: LazyLock<
    Mutex<HashMap<String, tokio::sync::oneshot::Sender<PageTextPayload>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_ACTION_WAITERS: LazyLock<
    Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(debug_assertions)]
static BROWSER_MOUSE_SELFTEST_WAITERS: LazyLock<
    Mutex<HashMap<String, tokio::sync::oneshot::Sender<BrowserMouseSelftestReport>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_WINDOW_LABELS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static BROWSER_WINDOW_TARGETS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_WINDOW_OWNERS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_WINDOW_TITLES: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BROWSER_WINDOW_KINDS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
// WKWebView.URL can briefly be nil while navigating or closing, and Wry 0.54
// unwraps it internally. Keep event-driven snapshots instead of calling url().
static BROWSER_WINDOW_URLS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct BrowserWindowInfo {
    pub label: String,
    pub target: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "type")]
    pub kind: String,
}

pub fn browser_bridge_script() -> &'static str {
    BROWSER_BRIDGE_SCRIPT
}

pub fn browser_window_label_from_target(target: &str) -> String {
    target
        .strip_suffix("-ct")
        .or_else(|| target.strip_suffix("-tb"))
        .unwrap_or(target)
        .to_string()
}

pub fn emit_browser_agent_status(app: &tauri::AppHandle, target: &str, active: bool, action: &str) {
    crate::document_tabs::emit_agent_status(app, target, active, action);
}

pub fn register_readable_child(
    app: &tauri::AppHandle,
    owner_label: &str,
    label: &str,
    target: &str,
    title: &str,
    kind: &str,
) {
    if app.get_window(owner_label).is_none() || app.get_webview(target).is_none() {
        return;
    }
    BROWSER_WINDOW_LABELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string());
    BROWSER_WINDOW_TARGETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), target.to_string());
    BROWSER_WINDOW_OWNERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), owner_label.to_string());
    BROWSER_WINDOW_TITLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), title.to_string());
    BROWSER_WINDOW_KINDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), kind.to_string());
}

pub fn unregister_readable_label(label: &str) {
    BROWSER_WINDOW_LABELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    let target = BROWSER_WINDOW_TARGETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    if let Some(target) = target {
        BROWSER_WINDOW_URLS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&target);
    }
    BROWSER_WINDOW_OWNERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    BROWSER_WINDOW_TITLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    BROWSER_WINDOW_KINDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
}

pub fn set_readable_url(target: &str, url: &str) {
    BROWSER_WINDOW_URLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(target.to_string(), url.to_string());
}

fn readable_url(target: &str) -> String {
    BROWSER_WINDOW_URLS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(target)
        .cloned()
        .unwrap_or_default()
}

pub fn set_owner_active_target(owner_label: &str, target: &str, title: &str, kind: &str) {
    BROWSER_WINDOW_TARGETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(owner_label.to_string(), target.to_string());
    BROWSER_WINDOW_TITLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(owner_label.to_string(), title.to_string());
    BROWSER_WINDOW_KINDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(owner_label.to_string(), kind.to_string());
}

#[cfg(debug_assertions)]
fn browser_popup_title(url: &url::Url) -> String {
    url.host_str()
        .filter(|host| !host.trim().is_empty())
        .unwrap_or_else(|| url.as_str())
        .to_string()
}

#[cfg(debug_assertions)]
fn open_browser_popup_window(
    app: &tauri::AppHandle,
    url: url::Url,
) -> Result<BrowserWindowInfo, String> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported popup URL scheme: {}", scheme));
    }
    let title = browser_popup_title(&url);
    crate::document_tabs::open_external_tab(app, url.to_string(), Some(title))
}

pub struct BrowserAgentStatusGuard {
    app: tauri::AppHandle,
    target: String,
    active: bool,
}

impl BrowserAgentStatusGuard {
    pub fn start(app: &tauri::AppHandle, target: &str, action: &str) -> Self {
        emit_browser_agent_status(app, target, true, action);
        Self {
            app: app.clone(),
            target: target.to_string(),
            active: true,
        }
    }

    pub fn finish(mut self) {
        self.clear();
    }

    fn clear(&mut self) {
        if self.active {
            emit_browser_agent_status(&self.app, &self.target, false, "");
            self.active = false;
        }
    }
}

impl Drop for BrowserAgentStatusGuard {
    fn drop(&mut self) {
        self.clear();
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserElementRect {
    #[serde(default)]
    pub x: i64,
    #[serde(default)]
    pub y: i64,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub center_x: i64,
    #[serde(default)]
    pub center_y: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserViewportPayload {
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub scroll_x: i64,
    #[serde(default)]
    pub scroll_y: i64,
    #[serde(default)]
    pub scroll_height: i64,
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLinkPayload {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub rect: Option<BrowserElementRect>,
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserButtonPayload {
    #[serde(default)]
    pub text: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub rect: Option<BrowserElementRect>,
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInputPayload {
    #[serde(default)]
    pub label: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub rect: Option<BrowserElementRect>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextPayload {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub viewport: Option<BrowserViewportPayload>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub headings: Vec<String>,
    #[serde(default)]
    pub links: Vec<BrowserLinkPayload>,
    #[serde(default)]
    pub buttons: Vec<BrowserButtonPayload>,
    #[serde(default)]
    pub inputs: Vec<BrowserInputPayload>,
    #[serde(default)]
    pub content_source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPageTextReport {
    request_id: String,
    payload: PageTextPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserActionReport {
    request_id: String,
    payload: Value,
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMouseSelftestReport {
    request_id: String,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    href: String,
}

/// Create a standalone content window used only by the browser mouse selftest.
#[cfg(debug_assertions)]
pub fn create_browser_window(
    app: &tauri::AppHandle,
    label: &str,
    url: tauri::WebviewUrl,
    title: &str,
    width: f64,
    height: f64,
    init_scripts: &[&str],
) -> Result<BrowserWindowInfo, String> {
    let content_label = format!("{}-ct", label);

    let builder = tauri::window::WindowBuilder::new(app, label)
        .title(title)
        .inner_size(width, height)
        .resizable(true);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    let window = builder
        .build()
        .map_err(|e| format!("ウィンドウ作成失敗: {}", e))?;
    BROWSER_WINDOW_LABELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string());
    BROWSER_WINDOW_TARGETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), content_label.clone());
    BROWSER_WINDOW_OWNERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_string(), label.to_string());

    let target_for_load = content_label.clone();
    let mut content_builder = tauri::webview::WebviewBuilder::new(&content_label, url)
        .initialization_script(BROWSER_BRIDGE_SCRIPT)
        .on_page_load(move |_webview, payload| {
            set_readable_url(&target_for_load, payload.url().as_str());
        });
    for script in init_scripts {
        content_builder = content_builder.initialization_script(*script);
    }

    let app_for_new_window = app.clone();
    content_builder = content_builder.on_new_window(move |popup_url, _features| {
        let app_for_open = app_for_new_window.clone();
        let log_url = popup_url.to_string();
        let _ = app_for_new_window.run_on_main_thread(move || {
            if let Err(err) = open_browser_popup_window(&app_for_open, popup_url) {
                log::warn!(
                    "[browser] failed to open requested popup window url={}: {}",
                    log_url,
                    err
                );
            }
        });
        tauri::webview::NewWindowResponse::Deny
    });

    window
        .add_child(
            content_builder,
            tauri::Position::Logical(tauri::LogicalPosition::new(0.0, 0.0)),
            tauri::Size::Logical(tauri::LogicalSize::new(width, height)),
        )
        .map_err(|e| format!("コンテンツ作成失敗: {}", e))?;

    Ok(BrowserWindowInfo {
        label: label.to_string(),
        target: content_label,
        url: String::new(),
        title: title.to_string(),
        kind: "browser".to_string(),
    })
}

// ============ Browser Control Commands ============

#[tauri::command]
pub async fn browser_go_back(app: tauri::AppHandle, target: String) -> Result<(), String> {
    let wv = app.get_webview(&target).ok_or("Webview not found")?;
    wv.eval("history.back()").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn browser_go_forward(app: tauri::AppHandle, target: String) -> Result<(), String> {
    let wv = app.get_webview(&target).ok_or("Webview not found")?;
    wv.eval("history.forward()").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn browser_reload(app: tauri::AppHandle, target: String) -> Result<(), String> {
    let wv = app.get_webview(&target).ok_or("Webview not found")?;
    wv.eval("location.reload()").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn browser_get_url(app: tauri::AppHandle, target: String) -> Result<String, String> {
    if app.get_webview(&target).is_none() {
        return Err("Webview not found".into());
    }
    Ok(readable_url(&target))
}

#[tauri::command]
pub async fn browser_navigate(
    app: tauri::AppHandle,
    target: String,
    url: String,
) -> Result<(), String> {
    let parsed: url::Url = url.parse().map_err(|e| format!("URL parse error: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported URL scheme: {}", scheme));
    }
    let wv = app.get_webview(&target).ok_or("Webview not found")?;
    wv.navigate(parsed.clone()).map_err(|e| e.to_string())?;
    set_readable_url(&target, parsed.as_str());
    Ok(())
}

/// Close the browser window that owns `target` (which may be either the window
/// label, the `-ct` content webview label, or the `-tb` toolbar webview label).
/// Removes the label from the registry so subsequent `list_browser_windows`
/// calls reflect reality even before Tauri finishes destroying the window.
pub async fn browser_close(app: tauri::AppHandle, target: String) -> Result<String, String> {
    let label = browser_window_label_from_target(&target);
    let window = app
        .get_window(&label)
        .ok_or_else(|| format!("ウィンドウが見つかりません: {}", label))?;
    unregister_readable_label(&label);
    window
        .close()
        .map_err(|e| format!("ウィンドウを閉じられませんでした: {}", e))?;
    Ok(label)
}

#[tauri::command]
pub async fn browser_report_page_text(report: BrowserPageTextReport) -> Result<(), String> {
    let tx = PAGE_TEXT_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&report.request_id)
        .ok_or_else(|| "No pending browser text request".to_string())?;
    let _ = tx.send(report.payload);
    Ok(())
}

#[tauri::command]
pub async fn browser_report_action_result(report: BrowserActionReport) -> Result<(), String> {
    let tx = BROWSER_ACTION_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&report.request_id)
        .ok_or_else(|| "No pending browser action request".to_string())?;
    let _ = tx.send(report.payload);
    Ok(())
}

#[cfg(debug_assertions)]
#[tauri::command]
pub async fn debug_browser_mouse_selftest_report(
    report: BrowserMouseSelftestReport,
) -> Result<(), String> {
    if report.count == 0 {
        eprintln!("SELAH_BROWSER_MOUSE_SELFTEST_READY {}", report.href);
        return Ok(());
    }
    let tx = BROWSER_MOUSE_SELFTEST_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&report.request_id)
        .ok_or_else(|| "No pending browser mouse selftest request".to_string())?;
    let _ = tx.send(report);
    Ok(())
}

#[cfg(not(debug_assertions))]
#[tauri::command]
pub async fn debug_browser_mouse_selftest_report() -> Result<(), String> {
    Err("debug commands are not available in release builds".into())
}

pub fn list_browser_windows(app: &tauri::AppHandle) -> Vec<BrowserWindowInfo> {
    let labels: Vec<String> = BROWSER_WINDOW_LABELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    let mut items: Vec<BrowserWindowInfo> = labels
        .into_iter()
        .filter_map(|label| {
            let target = BROWSER_WINDOW_TARGETS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&label)
                .cloned()
                .unwrap_or_else(|| format!("{}-ct", &label));
            let owner = BROWSER_WINDOW_OWNERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&label)
                .cloned()
                .unwrap_or_else(|| label.clone());
            app.get_window(&owner)?;
            app.get_webview(&target)?;
            let url = readable_url(&target);
            let title = BROWSER_WINDOW_TITLES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&label)
                .cloned()
                .unwrap_or_default();
            let kind = BROWSER_WINDOW_KINDS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&label)
                .cloned()
                .unwrap_or_else(|| {
                    if label.contains("detail") || target.contains("detail") {
                        "detail".to_string()
                    } else {
                        "browser".to_string()
                    }
                });
            Some(BrowserWindowInfo {
                label,
                target,
                url,
                title,
                kind,
            })
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

pub fn resolve_browser_target(
    app: &tauri::AppHandle,
    requested: Option<&str>,
) -> Result<String, String> {
    if let Some(target) = requested {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err("browser target is empty".into());
        }
        if app.get_webview(trimmed).is_some() {
            return Ok(trimmed.to_string());
        }
        let content = format!("{}-ct", trimmed);
        if app.get_webview(&content).is_some() {
            return Ok(content);
        }
        return Err(format!("Browser target not found: {}", trimmed));
    }
    let items = list_browser_windows(app);
    match items.as_slice() {
        [] => Err("No browser window is open".into()),
        [only] => Ok(only.target.clone()),
        _ => Err("Multiple browser windows are open; list_browser_windows first".into()),
    }
}

pub async fn extract_page_text(
    app: &tauri::AppHandle,
    target: &str,
) -> Result<PageTextPayload, String> {
    let wv = app.get_webview(target).ok_or("Webview not found")?;

    for attempt in 0..5 {
        let request_id = format!("browser-text-{}", uuid::Uuid::new_v4());
        let (tx, rx) = tokio::sync::oneshot::channel();
        PAGE_TEXT_WAITERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(request_id.clone(), tx);

        let js = format!(
            "(function(){{ if (window.__selahBrowserExtractText) window.__selahBrowserExtractText({}); }})();",
            serde_json::to_string(&request_id).unwrap_or_else(|_| "\"\"".into())
        );

        if let Err(e) = wv.eval(&js) {
            PAGE_TEXT_WAITERS
                .lock()
                .unwrap_or_else(|pe| pe.into_inner())
                .remove(&request_id);
            return Err(e.to_string());
        }

        match tokio::time::timeout(std::time::Duration::from_millis(1200), rx).await {
            Ok(Ok(payload))
                if !payload.url.is_empty()
                    && payload.url != "about:blank"
                    && (!payload.text.trim().is_empty() || attempt >= 2) =>
            {
                return Ok(payload);
            }
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {
                PAGE_TEXT_WAITERS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request_id);
                if attempt < 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    continue;
                }
            }
        }
    }
    Err("Timed out while extracting page text".into())
}

pub async fn run_browser_action(
    app: &tauri::AppHandle,
    target: &str,
    action: &Value,
    timeout_ms: u64,
) -> Result<Value, String> {
    let wv = app.get_webview(target).ok_or("Webview not found")?;
    let request_id = format!("browser-action-{}", uuid::Uuid::new_v4());
    let (tx, rx) = tokio::sync::oneshot::channel();
    BROWSER_ACTION_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(request_id.clone(), tx);

    let js = format!(
        "(function(){{ if (window.__selahBrowserRunAction) window.__selahBrowserRunAction({}, {}); else window.__TAURI__?.core?.invoke?.('browser_report_action_result', {{ report: {{ requestId: {}, payload: {{ ok: false, error: 'Browser action bridge unavailable' }} }} }}); }})();",
        serde_json::to_string(&request_id).unwrap_or_else(|_| "\"\"".into()),
        serde_json::to_string(action).unwrap_or_else(|_| "{}".into()),
        serde_json::to_string(&request_id).unwrap_or_else(|_| "\"\"".into()),
    );

    if let Err(e) = wv.eval(&js) {
        BROWSER_ACTION_WAITERS
            .lock()
            .unwrap_or_else(|pe| pe.into_inner())
            .remove(&request_id);
        return Err(e.to_string());
    }

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms.max(300)), rx).await {
        Ok(Ok(payload)) => Ok(payload),
        Ok(Err(_)) => Err("Browser action channel closed".into()),
        Err(_) => {
            BROWSER_ACTION_WAITERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&request_id);
            Err(format!(
                "Timed out while waiting for browser action after {} ms",
                timeout_ms.max(300)
            ))
        }
    }
}

#[cfg(debug_assertions)]
pub async fn debug_browser_mouse_click_selftest(app: tauri::AppHandle) -> Result<Value, String> {
    let owner_label = "ext-browser-mouse-selftest";
    if let Some(window) = app.get_window(owner_label) {
        let _ = window.close();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let request_id = format!("browser-mouse-selftest-{}", uuid::Uuid::new_v4());
    let (tx, rx) = tokio::sync::oneshot::channel();
    BROWSER_MOUSE_SELFTEST_WAITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(request_id.clone(), tx);
    let page_url = format!(
        "index.html?surface=browser-mouse-selftest&request={}",
        urlencoding::encode(&request_id)
    );

    let info = create_browser_window(
        &app,
        owner_label,
        tauri::WebviewUrl::App(page_url.into()),
        "Browser Mouse Selftest",
        760.0,
        520.0,
        &[],
    )?;

    for (label, window) in app.windows() {
        if label != owner_label {
            let _ = window.hide();
        }
    }
    if let Some(window) = app.get_window(owner_label) {
        let _ = window.set_always_on_top(true);
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            80, 80,
        )));
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(webview) = app.get_webview(&info.target) {
        let _ = webview.set_focus();
    }

    tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
    for (label, window) in app.windows() {
        if label != owner_label {
            let _ = window.hide();
        }
    }
    if let Some(window) = app.get_window(owner_label) {
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(webview) = app.get_webview(&info.target) {
        let _ = webview.set_focus();
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let click_x = 230.0;
    let click_y = 136.0;

    let click_result = crate::computer_control::mouse_click(
        &app,
        Some(&info.target),
        click_x,
        click_y,
        Some("webview"),
    )
    .await?;
    let click_result_for_error = click_result.clone();

    let report = match tokio::time::timeout(std::time::Duration::from_millis(4_000), rx).await {
        Ok(Ok(report)) => report,
        Ok(Err(_)) => return Err("browser mouse selftest report channel closed".into()),
        Err(_) => {
            BROWSER_MOUSE_SELFTEST_WAITERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&request_id);
            let screenshot = write_selftest_screenshot(&app, &info.target).await;
            return Err(format!(
                "mouse click did not reach WebView button; clicked at viewport ({:.0}, {:.0}); click result: {}; screenshot: {}",
                click_x,
                click_y,
                click_result_for_error,
                screenshot.unwrap_or_else(|| "unavailable".into())
            ));
        }
    };
    if report.count == 0 {
        return Err("browser mouse selftest report returned count=0".into());
    }

    if let Some(window) = app.get_window(owner_label) {
        let _ = window.close();
    }

    Ok(serde_json::json!({
        "status": "passed",
        "label": info.label,
        "target": info.target,
        "button": {
            "text": "Click target",
            "rect": {
                "x": 120,
                "y": 112,
                "width": 220,
                "height": 48,
                "centerX": click_x,
                "centerY": click_y,
            },
        },
        "click": click_result,
        "report": report,
    }))
}

#[cfg(debug_assertions)]
async fn write_selftest_screenshot(app: &tauri::AppHandle, target: &str) -> Option<String> {
    use base64::Engine;

    let value = crate::computer_control::screenshot(app, Some(target))
        .await
        .ok()?;
    let data = value
        .get("image")
        .and_then(|image| image.get("data_base64"))
        .and_then(|data| data.as_str())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let rect = value.get("screen_rect").cloned().unwrap_or(Value::Null);
    let path = std::env::temp_dir().join(format!(
        "selah-browser-mouse-selftest-{}.png",
        std::process::id()
    ));
    std::fs::write(&path, bytes).ok()?;
    Some(format!("{} screen_rect={}", path.display(), rect))
}
