'use strict';
// Stegosaur desktop UI: three tabs (Hide, Reveal, Analyze), lists you fill with the system's file windows,
// and a panel on the right. Every number comes from Rust; nothing is modelled here.

const $ = (s, r) => (r || document).querySelector(s);
const $$ = (s, r) => [...(r || document).querySelectorAll(s)];
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmt = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + ' MB' : n >= 1024 ? Math.round(n / 1024) + ' KB' : n + ' B');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const svg = (d) => '<svg viewBox="0 0 24 24" aria-hidden="true">' + d + '</svg>';
const ICON = {
  image: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M21 16l-5-5-8 8"/>'),
  audio: svg('<path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>'),
  text: svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h6"/>'),
  other: svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>'),
  lossy: svg('<circle cx="12" cy="12" r="9"/><path d="M5.6 5.6l12.8 12.8"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  check: svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
  plus: svg('<path d="M12 5v14M5 12h14"/>'),
  lock: svg('<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>'),
  arrowUp: svg('<path d="M12 19V5"/><path d="M6 11l6-6 6 6"/>'),
  arrowDown: svg('<path d="M12 5v14"/><path d="M6 13l6 6 6-6"/>'),
  chevR: svg('<path d="M9 5l7 7-7 7"/>'),
  chevD: svg('<path d="M5 9l7 7 7-7"/>'),
  eye: svg('<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>'),
  eyeOff: svg('<path d="M3 3l18 18"/><path d="M10.6 6.2A9.8 9.8 0 0 1 12 6c6 0 10 6 10 6a17 17 0 0 1-3.2 3.7M6.3 7.6A17 17 0 0 0 2 12s4 7 10 7a9.6 9.6 0 0 0 4.2-1"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/>'),
  copy: svg('<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>'),
  paste: svg('<rect x="5" y="5" width="14" height="16" rx="2"/><path d="M9 5V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v1"/><path d="M9 12h6M9 16h4"/>'),
  trash: svg('<path d="M4 7h16"/><path d="M10 11v6M14 11v6"/><path d="M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12"/><path d="M9 7V4h6v3"/>'),
  play: svg('<path d="M7 4l13 8-13 8z"/>'),
  stop: svg('<rect x="6" y="6" width="12" height="12" rx="2.5"/>'),
  folderPlus: svg('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 11v5M9.5 13.5h5"/>'),
  filePlus: svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M12 11v6M9 14h6"/>'),
  key: svg('<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9"/><path d="M16 7l3 3"/>'),
  warn: svg('<path d="M12 3l10 17H2z"/><path d="M12 10v4M12 17.5v.01"/>'),
  grip: svg('<circle cx="9" cy="6" r="1"/><circle cx="15" cy="6" r="1"/><circle cx="9" cy="12" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="9" cy="18" r="1"/><circle cx="15" cy="18" r="1"/>'),
  download: svg('<path d="M12 4v11"/><path d="M7 11l5 5 5-5"/><path d="M5 20h14"/>'),
  layers: svg('<path d="M12 3l9 5-9 5-9-5z"/><path d="M3 13l9 5 9-5"/>'),
  gauge: svg('<path d="M4 17a8 8 0 1 1 16 0"/><path d="M12 17l4-5"/><circle cx="12" cy="17" r="1.2"/>'),
  sort: svg('<path d="M8 5v14M5 8l3-3 3 3"/><path d="M16 19V5M13 16l3 3 3-3"/>'),
  sliders: svg('<path d="M4 7h10M18 7h2M4 17h2M10 17h10"/><circle cx="16" cy="7" r="2"/><circle cx="8" cy="17" r="2"/>'),
};
const CHEV = ICON.chevR.replace('<svg ', '<svg class="chev" ');

// ---------------------------------------------------------------- state
const IMG_EXT = ['png', 'bmp', 'tif', 'tiff', 'webp', 'ppm', 'jpg', 'jpeg'];
const AUD_EXT = ['wav', 'flac'];
const TXT_EXT = ['txt', 'log'];
const S = {
  tab: 'hide',
  hide: { payloads: [], covers: [], text: false, message: '', second: null, same: false, pass: '', confirm: '', confirm2: '', k: 2, layout: 'auto' },
  reveal: { files: [], passes: [], draft: '' },
  analyze: { files: [] },
  opts: { mode: 'match', parity: 16, rate: 10, kdf: 'standard', naming: 'keyed', pattern: '{name}_{n}', outDir: '', overwrite: false, extra: true, style: 'syslog', order: 4, recursive: false, namesOnly: false },
  styles: [],
  prev: null, prevError: null, previewing: false,
  result: null, busy: false, progress: '',
};
let focusZone = 'payloads', sel = { list: 'payloads', i: -1 }, sortBy = {}, showPass = false, typing = false, typedDraft = '', typedCount = 0;
const openSec = {};
const secOpen = (k, def) => ((k in openSec ? openSec[k] : def) ? 'open' : '');
document.addEventListener('toggle', (e) => { const d = e.target; if (d.dataset && d.dataset.sec) openSec[d.dataset.sec] = d.open; }, true);

const layoutName = { auto: 'Auto', split: 'Split', bulk: 'Bulk' };
const layoutLabel = (k) => ({ single: 'One file', same: 'One file', two: 'Two secrets', split: 'Split', bulk: 'Bulk', text: 'Text' }[k] || k);
const isText = (n) => TXT_EXT.includes(String(n).split('.').pop().toLowerCase());
const baseName = (p) => String(p).split(/[\\/]/).pop();
const bytesOf = (s) => new TextEncoder().encode(s).length;

function toItem(e) {
  const kind = e.kind === 'carrier' ? (AUD_EXT.includes(e.carrier) ? 'audio' : 'image') : e.kind === 'unusable' ? 'lossy' : isText(e.name) ? 'text' : 'other';
  return { path: e.path, name: e.name, size: e.size, kind, carrier: e.carrier, summary: e.summary, samples: e.samples, note: e.note };
}
const accepts = (it, acc) => acc === 'any' || (acc === 'cover' && (it.kind === 'image' || it.kind === 'audio')) || (acc === 'image' && it.kind === 'image') || (acc === 'reveal' && (it.kind === 'image' || it.kind === 'audio' || it.kind === 'text'));
const filtersFor = (acc) => (acc === 'cover' ? [{ name: 'Images and audio', extensions: IMG_EXT.concat(AUD_EXT) }] : acc === 'image' ? [{ name: 'Images', extensions: IMG_EXT }] : acc === 'reveal' ? [{ name: 'Images, audio and text', extensions: IMG_EXT.concat(AUD_EXT, TXT_EXT) }] : undefined);
const LISTS = { payloads: ['hide', 'payloads', 'any'], covers: ['hide', 'covers', 'cover'], reveal: ['reveal', 'files', 'reveal'], analyze: ['analyze', 'files', 'image'] };
const listOf = (key) => S[LISTS[key][0]][LISTS[key][1]];

function toast(m, o) {
  o = o || {};
  const t = $('#toast');
  t.innerHTML = (o.err ? ICON.warn : ICON.check) + '<span></span>';
  t.lastChild.textContent = m;
  t.classList.toggle('err', !!o.err);
  t.classList.add('on');
  clearTimeout(toast.h);
  toast.h = setTimeout(() => t.classList.remove('on'), o.ms || 2400);
}
function celebrate(m) {
  const b = $('#logo');
  b.classList.remove('wig'); void b.offsetWidth; b.classList.add('wig');
  toast(m);
}
const msgOf = (e) => (typeof e === 'string' ? e : (e && e.message) || String(e));

// ---------------------------------------------------------------- adding files
async function addPaths(key, paths) {
  if (!paths || !paths.length) return;
  const acc = LISTS[key][2];
  try {
    const entries = await api.describe(paths, acc, S.opts.recursive && key === 'reveal');
    const list = listOf(key), refused = [];
    for (const e of entries) {
      const it = toItem(e);
      if (!accepts(it, acc)) { refused.push(it.name + (it.note ? ' (' + it.note + ')' : it.kind === 'lossy' ? ' (lossy format)' : '')); continue; }
      if (!list.some((x) => x.path === it.path)) list.push(it);
    }
    if (refused.length) toast('Not used: ' + refused.slice(0, 3).join(', ') + (refused.length > 3 ? ' and ' + (refused.length - 3) + ' more' : ''), { err: true, ms: 5000 });
    S.result = null;
    changed();
  } catch (e) { toast(msgOf(e), { err: true, ms: 6000 }); }
}
async function pickInto(key, folder) {
  try {
    const paths = folder ? await api.pickFolders('Choose a folder') : await api.pickFiles(key === 'payloads' ? 'Choose the files to hide' : 'Choose files', filtersFor(LISTS[key][2]));
    await addPaths(key, paths);
  } catch (e) { toast(msgOf(e), { err: true }); }
}
async function chooseSecond() {
  try {
    const paths = await api.pickFiles('Choose the second file', undefined);
    if (!paths.length) return;
    const entries = await api.describe([paths[0]], 'any', false);
    if (entries[0]) { S.hide.second.file = toItem(entries[0]); S.result = null; changed(); }
  } catch (e) { toast(msgOf(e), { err: true }); }
}
// A small window that collects lines one at a time (type or paste, Enter adds the line). Pasting several lines at once adds each.
function pasteModal(onLines, o) {
  o = o || {};
  const lines = [];
  const back = document.createElement('div');
  back.className = 'modalb';
  const draw = () => {
    $('#pl', back).innerHTML = lines.map((l, i) => '<div class="prow"><span class="dim">' + (i + 1) + '</span><span class="pt" title="' + (o.secret ? '' : esc(l)) + '">' + (o.secret ? stars(l) : esc(l)) + '</span><button class="ico del" data-rm="' + i + '" title="Remove">' + ICON.close + '</button></div>').join('') || '<div class="prow dim">' + esc(o.empty || 'Nothing added yet.') + '</div>';
    $('#pc', back).textContent = lines.length + (lines.length === 1 ? ' line' : ' lines');
    const l = $('#pl', back); l.scrollTop = l.scrollHeight;
  };
  const addText = (t) => { for (const x of String(t).split(/\r?\n/)) { const v = o.secret ? x : x.trim().replace(/^"|"$/g, ''); if (v && !lines.includes(v)) lines.push(v); } };
  back.innerHTML = '<div class="pk"><div class="pk-top"><b>' + esc(o.title || 'Paste paths') + '</b><button class="pk-x" data-x title="Cancel">' + ICON.close + '</button></div><div class="plist" id="pl"></div><div style="padding:8px 16px 14px"><div class="pwrow"><input class="in" id="pi" type="' + (o.secret ? 'password' : 'text') + '" autocomplete="off" spellcheck="false" placeholder="' + esc(o.placeholder || 'a full path, then Enter') + '"><button data-add title="Add">' + ICON.plus + '</button></div></div><div class="pk-foot"><span class="dim" id="pc"></span><span class="sp"></span><button class="btn ghost sm" data-x>Cancel</button><button class="btn sm" data-ok>Add all</button></div></div>';
  document.body.appendChild(back);
  draw();
  const inp = $('#pi', back);
  inp.focus();
  const take = () => { if (inp.value) { addText(inp.value); inp.value = ''; draw(); } };
  inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); take(); } });
  inp.addEventListener('paste', (e) => {
    const t = (e.clipboardData || window.clipboardData).getData('text');
    if (/\n/.test(t)) { e.preventDefault(); addText(t); draw(); }
  });
  back.addEventListener('click', (e) => {
    if (e.target === back || e.target.closest('[data-x]')) { back.remove(); return; }
    const rm = e.target.closest('[data-rm]'); if (rm) { lines.splice(+rm.dataset.rm, 1); draw(); return; }
    if (e.target.closest('[data-add]')) { take(); inp.focus(); return; }
    if (e.target.closest('[data-ok]')) { take(); const out = lines.slice(); back.remove(); if (out.length) onLines(out); }
  });
}

// ---------------------------------------------------------------- the Hide plan
// What a file takes inside the cover: its bytes plus the small header that carries its name (typed text has none).
const ws = (f) => f.size + (f.typed ? 0 : 9 + new TextEncoder().encode(f.name).length);

function plan() {
  const h = S.hide, N = h.payloads.length, M = h.covers.length;
  const out = { kind: null, problems: [], headline: '' };
  if (h.text) {
    out.kind = 'text';
    out.headline = 'Turns your message into ordinary-looking text.';
    if (!h.message.trim()) out.problems.push('Type the message to hide.');
  } else {
    if (!N) out.problems.push('Choose what to hide.');
    if (!M) out.problems.push('Choose where to hide it.');
    if (N === 1 && M === 1) out.kind = h.second ? 'two' : h.same ? 'same' : 'single';
    else if (N === 1 && M > 1) out.kind = 'split';
    else if (N > 1 && M >= 1) out.kind = 'bulk';
    out.headline = { single: 'Hides one file in one cover.', same: 'Hides one file. The cover looks the same whether it holds one secret, two or none.', two: 'Hide two files in one cover', split: 'Hide data in many pieces', bulk: M >= N ? 'Hides each of the ' + N + ' files in its own cover, in the order shown.' : M === 1 ? 'Hides each of the ' + N + ' files in its own copy of the cover.' : 'Hides the ' + N + ' files in turn in the ' + M + ' covers. Each cover is copied as often as needed.' }[out.kind] || '';
    const want = { split: [N === 1 && M > 1, 'needs 1 file and 2 or more covers'], bulk: [N > 1 && M >= 1, 'needs 2 or more files and at least one cover'] }[h.layout];
    if (want && !want[0] && N && M) out.problems.unshift('Layout "' + h.layout + '" ' + want[1] + ' (you have ' + N + ' file(s), ' + M + ' cover(s)).');
    if (h.second && !h.second.file) out.problems.push('Choose the second file.');
  }
  if (!h.pass) out.problems.push('Enter a passphrase.');
  else if (h.pass !== h.confirm) out.problems.push(h.confirm ? 'The two passphrase boxes do not match.' : 'Type the passphrase again to confirm it.');
  if (h.second) {
    if (!h.second.pass) out.problems.push('Enter the second passphrase.');
    else if (h.second.pass !== h.confirm2) out.problems.push(h.confirm2 ? 'The second passphrase boxes do not match.' : 'Type the second passphrase again to confirm it.');
    else if (h.second.pass === h.pass) out.problems.push('The two passphrases must differ.');
  }
  if (S.prev && S.prev.items) for (const it of S.prev.items) if (it.fits === false && (out.kind === 'single' || out.kind === 'bulk')) { out.problems.push(S.opts.extra ? 'A file is too big for its cover, and this kind of cover cannot take extra room. Use a PNG, BMP or WAV, or a bigger one.' : 'A file is too big for its cover. Turn on extra room under Output.'); break; } else if (it.error) { out.problems.push(it.error); break; } else if (it.fits === false) { out.problems.push('A file is too big for its cover.'); break; }
  if (S.prevError) out.problems.push(S.prevError);
  return out;
}
function problems() {
  if (S.tab === 'hide') return plan().problems;
  if (S.tab === 'reveal') { const p = []; if (!S.reveal.files.length) p.push('Choose the files to open.'); if (!S.reveal.passes.length && !S.reveal.draft) p.push('Add at least one passphrase.'); return p; }
  return S.analyze.files.length ? [] : ['Choose the images to scan.'];
}

// ---------------------------------------------------------------- capacity and risk, measured by Rust
let prevSeq = 0, prevTimer = null;
function schedulePreview() { clearTimeout(prevTimer); prevTimer = setTimeout(doPreview, 220); }
async function doPreview() {
  const seq = ++prevSeq;
  const p = plan(), h = S.hide, o = S.opts;
  if (S.tab !== 'hide' || !p.kind || p.kind === 'text') { S.prev = null; S.prevError = null; render(); return; }
  const base = { mode: o.mode, parity: o.parity, rate: o.rate / 100 };
  S.previewing = true;
  try {
    let items = [];
    if (p.kind === 'single') {
      let r = await api.preview({ op: 'embed', carrier: h.covers[0].path, payload_len: ws(h.payloads[0]), ...base, measure: true });
      const extra = extraNeeded(o, h.covers[0].path, ws(h.payloads[0]), r);
      if (extra) r = await api.preview({ op: 'embed', carrier: h.covers[0].path, payload_len: r.capacity, ...base, measure: true });
      items = [{ cap: r.capacity, fits: r.fits || !!extra, fill: extra ? 1 : r.fill, extra, score: r.measured ? r.measured.after.score : null, note: r.detect_note, error: r.error }];
    } else if (p.kind === 'same' || p.kind === 'two') {
      const r = await api.preview({ op: 'hidden', carrier: h.covers[0].path, payload_len: ws(h.payloads[0]), hidden_len: p.kind === 'two' && h.second.file ? ws(h.second.file) : null, ...base, measure: true });
      const big = Math.max(ws(h.payloads[0]), p.kind === 'two' && h.second.file ? ws(h.second.file) : 0);
      items = [{ cap: r.capacity, fits: r.public_fits !== false && r.hidden_fits !== false, fill: r.capacity ? Math.min(1, big / r.capacity) : 1, score: r.measured ? r.measured.after.score : null, note: r.detect_note, error: r.error }];
    } else if (p.kind === 'split') {
      const r = await api.preview({ op: 'split', carriers: h.covers.map((c) => c.path), secret_len: ws(h.payloads[0]), k: Math.min(h.k, h.covers.length), ...base });
      const sl = r.split ? r.split.share_len : 0;
      items = (r.split ? r.split.carriers : []).map((c) => ({ cap: c.capacity, fits: c.fits, fill: c.capacity ? Math.min(1, sl / c.capacity) : 1 }));
      if (r.error) items.length && (items[0].error = r.error);
    } else {
      const M = h.covers.length;
      const biggest = h.covers.map((_, c) => h.payloads.reduce((m, f, j) => (j % M === c && (m < 0 || ws(f) > ws(h.payloads[m])) ? j : m), -1));
      for (let c = 0; c < M; c++) {
        const i = biggest[c];
        if (i < 0) { items.push({ cap: 0, fits: true, fill: 0, unused: true }); continue; }
        const r = await api.preview({ op: 'embed', carrier: h.covers[c].path, payload_len: ws(h.payloads[i]), ...base, measure: false });
        if (seq !== prevSeq) return;
        const extra = extraNeeded(o, h.covers[c].path, ws(h.payloads[i]), r);
        items.push({ cap: r.capacity, fits: r.fits || !!extra, fill: extra ? 1 : r.fill, extra, score: null, error: extra ? null : r.error });
      }
    }
    if (seq !== prevSeq) return;
    S.prev = { kind: p.kind, items }; S.prevError = null;
  } catch (e) {
    if (seq !== prevSeq) return;
    S.prev = null; S.prevError = msgOf(e);
  }
  S.previewing = false;
  render();
}
// How many bytes would go into the extra block, or 0 when the cover holds it all (or extra room is off or not possible).
function extraNeeded(o, path, size, r) {
  if (!o.extra || r.fits || !r.capacity) return 0;
  return /\.(png|bmp|wav)$/i.test(path) ? Math.max(0, size - r.capacity) : 0;
}
function changed() { if (S.tab === 'hide') { S.prev = null; schedulePreview(); } render(); }

// ---------------------------------------------------------------- pieces of the page
const ib = (icon, act, arg, title, cls) => '<button class="ico ' + (cls || '') + '" data-act="' + act + '" ' + (arg != null ? 'data-arg="' + arg + '"' : '') + ' title="' + esc(title) + '">' + ICON[icon] + '</button>';
const tb = (icon, label, act, title, pri) => '<button class="tbtn ' + (pri ? 'pri' : '') + '" data-act="' + act + '" title="' + esc(title || label) + '" aria-label="' + esc(title || label) + '">' + ICON[icon] + '</button>';
const stars = (s) => '*'.repeat(Math.min(14, s.length));
const sizeSum = (l) => fmt(l.reduce((a, f) => a + f.size, 0));
const strengthOf = (v) => { const f = v.length < 8 ? 0.25 : v.length < 14 ? 0.6 : 1; return { f, label: v ? (f < 0.5 ? 'weak' : f < 1 ? 'okay' : 'strong') : '' }; };
const strengthHead = (vals) => {
  const used = vals.filter(Boolean);
  const w = used.length ? Math.min(...used.map((v) => strengthOf(v).f)) : 0;
  const lab = used.length ? strengthOf(used.reduce((a, b) => (strengthOf(a).f <= strengthOf(b).f ? a : b))).label : '';
  return '<span class="sstrength" title="Passphrase strength"><span class="tiny">' + lab + '</span><span class="sbar"><i style="width:' + (used.length ? w * 100 : 0) + '%;background:' + (w < 0.5 ? 'var(--red)' : 'var(--green)') + '"></i></span></span>';
};
const confirmNote = (a, b) => (!b ? '' : a === b ? '<div class="tiny ok" style="margin-top:4px">' + ICON.check + ' Matches</div>' : '<div class="tiny" style="margin-top:4px;color:var(--red)">' + ICON.warn + ' Does not match yet</div>');
const riskOf = (s) => (s < 12 ? ['low', 'Low'] : s < 35 ? ['mid', 'Medium'] : ['high', 'High']);
const riskBlock = (score) => {
  const [cls, label] = riskOf(score);
  return '<div class="riskrow"><span class="lbl">Worst risk</span><span class="rk ' + cls + '">' + label + ' - ' + Math.round(score) + '/100</span></div><div class="gauge" style="min-width:0"><i style="left:' + Math.min(96, Math.max(3, score)) + '%"></i></div><div class="gcap"><span>Hard to spot</span><span>Easy to spot</span></div>';
};
const gaugeBar = (score) => '<div class="gauge"><i style="left:' + Math.min(96, Math.max(3, score)) + '%"></i></div>';
const passRow = (id, bind, val, ph, cbind, cval) => '<div class="pwrow"><input class="in" id="' + id + '" type="' + (showPass ? 'text' : 'password') + '" autocomplete="off" data-bind="' + bind + '" value="' + esc(val) + '" placeholder="' + ph + '"><button data-showpass title="' + (showPass ? 'Hide' : 'Show') + ' passphrases">' + (showPass ? ICON.eyeOff : ICON.eye) + '</button></div><div class="pwrow" style="margin-top:7px"><input class="in" id="' + id + 'c" type="' + (showPass ? 'text' : 'password') + '" autocomplete="off" data-bind="' + cbind + '" value="' + esc(cval || '') + '" placeholder="re-enter"><button data-showpass title="' + (showPass ? 'Hide' : 'Show') + ' passphrases">' + (showPass ? ICON.eyeOff : ICON.eye) + '</button></div>' + confirmNote(val, cval || '');
const rng = (id, label, min, max, val, extra, step, bind, dis) => '<label>' + label + '</label><div class="r"><input type="range" min="' + min + '" max="' + max + '" ' + (step ? 'step="' + step + '"' : '') + ' value="' + val + '" ' + (bind ? 'data-bind="' + bind + '"' : 'data-opt="' + id + '"') + ' ' + (dis ? 'disabled' : '') + '><span class="v">' + extra + '</span></div>';
const segBtn = (cur, val, act, arg, label, title) => '<button class="' + (cur === val ? 'on' : '') + '" data-act="' + act + '" data-arg="' + arg + '"' + (title ? ' title="' + esc(title) + '"' : '') + '>' + label + '</button>';
const optSelect = (name, list, cur) => '<select class="in" data-opt="' + name + '">' + list.map(([v, l]) => '<option value="' + esc(v) + '" ' + (cur === v ? 'selected' : '') + '>' + esc(l) + '</option>').join('') + '</select>';

function zoneShell(key, title, files, toolbar, body, cntText) {
  return '<section class="zone ' + (focusZone === key ? 'focus' : '') + '" data-zone="' + key + '" tabindex="0"><header><h3>' + title + '</h3><span class="cnt" title="' + esc(cntText != null ? cntText : files.length + ' item' + (files.length === 1 ? '' : 's') + (files.length && typeof files[0] !== 'string' ? ', ' + sizeSum(files) : '')) + '">' + (cntText != null ? cntText.split(',')[0] : files.length) + '</span><span class="tools">' + toolbar + '</span></header><div class="zb">' + body + '</div></section>';
}
const emptyBody = (msg, add, dir, paste, extra) => '<div class="dropmsg"><div class="big">' + ICON.download + '</div><div>' + msg + '</div><div class="row2"><button class="tbtn pri" data-act="' + add + '" title="Add files" aria-label="Add files">' + ICON.filePlus + '</button><button class="tbtn" data-act="' + dir + '" title="Add a folder" aria-label="Add a folder">' + ICON.folderPlus + '</button><button class="tbtn" data-act="' + paste + '" title="Paste paths" aria-label="Paste paths">' + ICON.paste + '</button>' + (extra || '') + '</div></div>';
const typeBtn = () => '<button class="tbtn ' + (typing ? 'pri' : '') + '" data-typetext title="Type a short message to hide, no file needed" aria-label="Type text">' + ICON.text + '</button>';
const listToolbar = (key, add, dir, paste, files, extra) => tb('filePlus', 'Add', add, 'Add files') + tb('folderPlus', 'Folder', dir, 'Add a whole folder') + tb('paste', 'Paste', paste, 'Paste a list of paths') + (extra || '') + (files.length ? '<button class="tbtn" data-act="clearList" data-arg="' + key + '" title="Remove all">' + ICON.trash + '</button>' : '');
const rowOps = (key, i) => ib('arrowUp', 'move', key + ':' + i + ':-1', 'Move up') + ib('arrowDown', 'move', key + ':' + i + ':1', 'Move down') + ib('close', 'dropList', key + ':' + i, 'Remove', 'del');
const rowAttrs = (key, i) => 'data-row="' + i + '" data-rk="' + key + '" class="' + (sel.list === key && sel.i === i ? 'sel' : '') + '"';
function nameHTML(name) {
  const i = name.lastIndexOf('.');
  const ext = i > 0 && name.length - i <= 8 ? name.slice(i) : '';
  const base = ext ? name.slice(0, i) : name;
  return '<span class="nb">' + esc(base) + '</span>' + (ext ? '<span class="ne">' + esc(ext) + '</span>' : '');
}
const nameCell = (f) => '<span class="nmw">' + icFor(f) + '<span class="nt">' + nameHTML(f.name) + '</span></span>';
const icFor = (f) => '<span class="ic k-' + f.kind + '">' + (ICON[f.kind] || ICON.other) + '</span>';
// A clickable column header. The first click sorts ascending, the next one on the same column reverses it.
const sh = (key, col, label, cls) => {
  const s = sortBy[key], on = s && (s.col === col || (col === 'score' && s.col === 'gauge'));
  return '<th class="sortable ' + (cls || '') + (on ? (s.dir > 0 ? ' asc' : ' desc') : '') + '" data-sortcol="' + key + ':' + col + '" title="Sort by ' + esc(String(label || 'order added').toLowerCase()) + '">' + label + '</th>';
};
// `get` supplies the values of columns that are not name, size or position.
function sorted(key, files, get) {
  const arr = files.map((f, i) => ({ f, i })), s = sortBy[key];
  if (!s) return arr;
  const col = s.col === 'gauge' ? 'score' : s.col;
  const val = (x) => (col === 'idx' ? x.i : col === 'name' ? x.f.name.toLowerCase() : col === 'size' ? x.f.size : get && get[col] ? get[col](x.f, x.i) : x.i);
  arr.sort((a, b) => {
    const va = val(a), vb = val(b);
    const c = typeof va === 'number' && typeof vb === 'number' ? va - vb : String(va).localeCompare(String(vb), undefined, { numeric: true });
    return (c || a.i - b.i) * s.dir;
  });
  return arr;
}
const typedEditor = () => (typing ? '<div class="typed"><label class="l">Message to hide</label><textarea class="in" id="typed" placeholder="Type or paste the text to hide" style="height:96px">' + esc(typedDraft) + '</textarea><div class="tiny" id="typedcount" style="margin-top:4px">' + typedInfo() + '</div><div class="row2"><button class="tbtn pri" data-typeadd title="Add the message" aria-label="Add message">' + ICON.check + '</button><button class="tbtn" data-typecancel title="Cancel" aria-label="Cancel">' + ICON.close + '</button></div></div>' : '');
const typedInfo = () => typedDraft.length + ' characters, ' + bytesOf(typedDraft) + ' bytes. 1 KB holds about 1,000 characters.';

// ---------------------------------------------------------------- Hide
function hideZones() {
  const h = S.hide, p = plan(), res = S.result && S.result.tab === 'hide' ? S.result.items : null;
  const split = p.kind === 'split', items = (S.prev && S.prev.items) || [];
  const n = Math.max(h.payloads.length, h.covers.length);
  const left = h.text
    ? zoneShell('payloads', 'Hide', [], '', '<div style="padding:10px"><label class="l">Message to hide</label><textarea class="in" id="msg" data-bind="hide.message" style="height:150px" placeholder="Type the message to hide">' + esc(h.message) + '</textarea><div class="tiny" style="margin-top:4px">' + bytesOf(h.message) + ' bytes</div></div>', bytesOf(h.message) + ' bytes')
    : zoneShell('payloads', 'Hide', h.payloads, listToolbar('payloads', 'addPayload', 'addPayloadDir', 'pastePayload', h.payloads, typeBtn()),
      typedEditor() + (h.payloads.length ? '<table><thead><tr>' + sh('payloads', 'idx', '#', 'c-idx') + sh('payloads', 'name', 'File') + sh('payloads', 'size', 'Size', 'c-size') + '<th class="c-act"></th></tr></thead><tbody>' + sorted('payloads', h.payloads).map(({ f, i }) => '<tr ' + rowAttrs('payloads', i) + '><td class="dim">' + (i + 1) + '</td><td class="nm" title="' + esc(f.path) + '">' + nameCell(f) + '</td><td class="dim">' + fmt(f.size) + '</td><td class="act">' + rowOps('payloads', i) + '</td></tr>').join('') + '</tbody></table>' : (typing ? '' : emptyBody('Drop the stuff to hide here', 'addPayload', 'addPayloadDir', 'pastePayload', typeBtn()))));
  const gen = S.result && S.result.tab === 'hide' && S.result.text != null ? S.result.text : null;
  let right;
  if (h.text && gen != null) right = zoneShell('covers', 'Cover text', [], '<button class="tbtn" data-copytext title="Copy the text" aria-label="Copy">' + ICON.copy + '</button><button class="tbtn pri" data-savetext title="Save as a .txt file" aria-label="Save">' + ICON.download + '</button>', '<div class="textout">' + esc(gen) + '</div>', gen.split('\n').length + ' lines, ' + gen.split(/\s+/).filter(Boolean).length + ' words');
  else if (h.text) right = zoneShell('covers', 'Cover', [], '', '<div class="dropmsg"><div class="big">' + ICON.text + '</div><div>Generated text is the cover.<br>Style and realism are in the panel.</div></div>');
  else right = zoneShell('covers', 'Cover(s)', h.covers, listToolbar('covers', 'addCover', 'addCoverDir', 'pasteCover', h.covers),
    h.covers.length ? '<table><thead><tr>' + sh('covers', 'idx', '#', 'c-idx') + sh('covers', 'name', 'Cover') + (res ? sh('covers', 'saved', 'Saved as', 'c-res2') : sh('covers', 'room', 'Room', 'c-fill')) + '<th class="c-act"></th></tr></thead><tbody>' + (p.kind === 'bulk' && h.covers.length < h.payloads.length && res ? res.map((_, j) => ({ f: h.covers[j % h.covers.length], i: j % h.covers.length, j })) : sorted('covers', h.covers, { room: (f, i) => (items[i] ? items[i].cap + (items[i].extra || 0) : 0), saved: (f, i) => (res && res[i] && res[i].output ? baseName(res[i].output) : '') })).map(({ f, i, j }) => {
      const it = items[i], rr = res && (j != null ? res[j] : split ? res[i] : p.kind === 'bulk' ? res[i] : res[0]);
      const badge = (s) => '<span class="rk ' + riskOf(s)[0] + '">' + Math.round(s) + '</span>';
      const head = '<tr ' + rowAttrs('covers', i) + '><td class="dim">' + (i + 1) + '</td><td class="nm" title="' + esc(f.path) + '">' + nameCell(f) + '</td>';
      const tail = '<td class="act">' + rowOps('covers', i) + '</td></tr>';
      if (res) return head + '<td><div class="rescell">' + (rr ? (rr.error ? '<span class="tiny" style="color:var(--red)" title="' + esc(rr.error) + '">' + esc(rr.error) + '</span>' : '<span class="mono ok rn" title="' + esc(rr.output) + '">' + esc(baseName(rr.output)) + '</span>' + (rr.report ? badge(rr.report.score) : '')) : '') + '</div></td>' + tail;
      return head + '<td>' + (it && !it.unused ? '<div class="holds" title="' + (it.extra ? 'Room for ' + fmt(it.cap) + ' inside the file, plus ' + fmt(it.extra) + ' extra at the end of it' : it.fits ? 'Room for ' + fmt(it.cap) + ' of hidden data' : 'Only room for ' + fmt(it.cap) + ', not enough for what you are hiding') + '"><span class="' + (it.fits ? 'dim' : 'bad') + '">' + fmt(it.cap) + (it.extra ? ' <span class="xtra">+ ' + fmt(it.extra) + '</span>' : it.fits ? '' : ', too small') + '</span>' + (it.extra ? '<div class="fillbar two"><i style="width:' + Math.max(3, Math.round(it.cap / (it.cap + it.extra) * 100)) + '%"></i><i class="x" style="width:' + Math.round(it.extra / (it.cap + it.extra) * 100) + '%"></i></div>' : '<div class="fillbar" title="' + Math.round(it.fill * 100) + '% full"><i class="' + (it.fits ? '' : 'bad') + '" style="width:' + Math.max(3, Math.round(it.fill * 100)) + '%"></i></div>') + '</div>' : '') + '</td>' + tail;
    }).join('') + '</tbody></table>' : emptyBody('Choose images or audio as the cover', 'addCover', 'addCoverDir', 'pasteCover'));
  return [left, right];
}

function secondSection(h) {
  const f = h.second.file;
  const slot = f
    ? '<div class="slot2 full" data-zone="second">' + icFor(f) + '<span class="s2n" title="' + esc(f.path) + '"><b>' + esc(f.name) + '</b><small>' + fmt(f.size) + '</small></span><button class="tbtn" data-act="chooseSecond" title="Pick a different file" aria-label="Replace">' + ICON.filePlus + '</button><button class="ico del" data-act="removeSecond" title="Remove">' + ICON.close + '</button></div>'
    : '<div class="slot2" data-zone="second" data-act="chooseSecond" role="button" tabindex="0"><span class="s2i">' + ICON.download + '</span><span class="s2n"><b>Choose the second file</b><small>Click to browse, or drop a file here</small></span></div>';
  return '<div class="sec"><h4>' + ICON.filePlus + 'Second secret' + strengthHead([h.second.pass]) + '</h4>' + slot + '<div style="margin-top:8px">' + passRow('pass2', 'hide.pass2', h.second.pass, 'second passphrase', 'hide.confirm2', h.confirm2) + '</div></div>';
}

function hidePanel() {
  const h = S.hide, o = S.opts;
  let p = plan();
  if (h.second && !['single', 'two', 'same'].includes(p.kind) && !h.text) { h.second = null; p = plan(); }
  const pr = p.problems, items = (S.prev && S.prev.items) || [];
  const totalNeed = p.kind === 'split' ? 0 : p.kind === 'bulk' ? h.covers.reduce((a, _, c) => a + Math.max(0, ...h.payloads.filter((__, j) => j % h.covers.length === c).map(ws)), 0) : items.length ? h.payloads.slice(0, items.length).reduce((a, f) => a + ws(f), 0) : 0;
  const totalCap = items.reduce((a, i) => a + i.cap + (i.extra || 0), 0);
  const worst = items.some((i) => i.score != null) ? Math.max(...items.filter((i) => i.score != null).map((i) => i.score)) : null;
  const layoutDesc = { split: 'Hide data in many pieces', bulk: 'Many files. Each one goes into its own cover.' }[h.layout] || (p.headline || 'Picks the layout from what you chose.');
  const head = h.layout === 'auto' ? (p.kind ? 'Layout: ' + layoutLabel(p.kind) : 'Layout') : 'Layout';
  const style = S.styles.find((x) => x.id === o.style) || { adjustable_order: false };
  const done = S.result && S.result.tab === 'hide';
  return '<div class="pb">' +
    '<div class="sec"><h4>' + ICON.layers + head + '</h4><div class="seg full">' + ['auto', 'split', 'bulk'].map((k) => segBtn(h.layout, k, 'setLayout', k, layoutName[k], '')).join('') + '</div><div class="tiny" style="margin-top:6px;line-height:1.4">' + esc(layoutDesc) + '</div>' +
    (['single', 'two', 'same'].includes(p.kind) && !h.text ? '<div class="row" style="padding:10px 0 0"><div class="grow"><b>Second secret</b></div><div class="sw ' + (h.second ? 'on' : '') + '" id="secondsw" data-act="toggleSecond" role="switch" aria-checked="' + !!h.second + '"></div></div>' : '') +
    '<div class="kv" style="margin-top:8px"><label>Cover is</label><div class="seg full"><button class="' + (!h.text ? 'on' : '') + '" ' + (h.text ? 'data-act="toggleText"' : '') + '>File</button><button class="' + (h.text ? 'on' : '') + '" ' + (!h.text ? 'data-act="toggleText"' : '') + '>Text</button></div>' +
    (p.kind === 'split' ? rng('k', 'Needed (K)', 1, h.covers.length, Math.min(h.k, h.covers.length), Math.min(h.k, h.covers.length) + '/' + h.covers.length, 0, 'hide.k') : '') + '</div></div>' +
    '<div class="sec"><h4>' + ICON.key + (h.second ? 'First passphrase' : 'Passphrase') + strengthHead([h.pass]) + '</h4>' + passRow('pass', 'hide.pass', h.pass, 'passphrase', 'hide.confirm', h.confirm) + '</div>' +
    (h.second ? secondSection(h) : '') +
    '<details class="sec" data-sec="adv" ' + secOpen('adv', h.same) + '><summary>' + CHEV + ICON.sliders + 'Advanced<span class="tag">' + (o.mode === 'match' ? 'careful' : 'simple') + ', parity ' + o.parity + '</span></summary><div class="kv">' +
    '<label>Method</label><div class="seg full">' + segBtn(o.mode, 'match', 'setOpt', 'mode=match', 'Careful', 'Harder to detect (recommended)') + segBtn(o.mode, 'replace', 'setOpt', 'mode=replace', 'Simple', 'Simpler, but easier to detect') + '</div>' +
    rng('parity', 'Parity', 0, 64, o.parity, o.parity ? o.parity + ' B' : 'off', 2) +
    rng('rate', 'Slot size', 2, 30, o.rate, o.rate + '%', 0, null, !(p.kind === 'two' || p.kind === 'same')) +
    '<label>KDF</label>' + optSelect('kdf', [['standard', 'Standard'], ['strong', 'Strong']], o.kdf) +
    '<label>Deniable</label><label class="chk"><input type="checkbox" ' + (h.same ? 'checked' : '') + ' data-act="toggleSame" ' + (h.second || h.text ? 'disabled' : '') + '> same with 1, 2 or no secrets</label></div></details>' +
    '<details class="sec" data-sec="out" ' + secOpen('out', false) + '><summary>' + CHEV + ICON.download + 'Output<span class="tag">' + ({ keyed: 'ordinary name', suffix: 'cover + _1', custom: 'pattern' }[o.naming] || o.naming) + '</span></summary><div class="kv">' +
    '<label>File name</label>' + optSelect('naming', [['keyed', 'Ordinary name'], ['suffix', 'Cover name + _1'], ['custom', 'Custom pattern']], o.naming) +
    (o.naming === 'custom' ? '<label>Pattern</label><input class="in" data-opt="pattern" value="' + esc(o.pattern) + '">' : '') +
    '<label>Save in</label><div class="pwrow"><input class="in" data-opt="outDir" value="' + esc(o.outDir) + '" placeholder="next to each cover"><button data-browse title="Choose a folder">' + ICON.folderPlus + '</button></div>' +
    (['single', 'bulk'].includes(p.kind) ? '<label>Extra room</label><label class="chk"><input type="checkbox" data-opt="extra" ' + (o.extra ? 'checked' : '') + '> use when the cover is too small</label>' : '') +
    '<label>Overwrite</label><label class="chk"><input type="checkbox" data-opt="overwrite" ' + (o.overwrite ? 'checked' : '') + '> allow (never the inputs)</label></div></details>' +
    (h.text ? '<details class="sec" data-sec="txt" ' + secOpen('txt', true) + '><summary>' + CHEV + ICON.text + 'Text cover</summary><div class="kv"><label>Style</label>' + optSelect('style', S.styles.map((s) => [s.id, s.id]), o.style) + (style.adjustable_order ? rng('order', 'Realism', 1, 6, o.order, o.order) : '') + '</div></details>' : '') +
    '<div class="sec"><h4>Check</h4>' + (items.length ? '<div class="kv"><label>Capacity</label><div><div class="fillbar" style="height:7px"><i class="' + (totalNeed > totalCap ? 'bad' : '') + '" style="width:' + Math.max(3, Math.min(100, Math.round((totalNeed / Math.max(totalCap, 1)) * 100))) + '%"></i></div><div class="tiny">' + fmt(totalNeed) + ' of ' + fmt(totalCap) + '</div></div></div>' + (worst != null ? riskBlock(worst) : '<div class="tiny" style="margin-top:8px">' + (S.previewing ? 'Measuring on a copy...' : (items[0] && items[0].note) ? esc(items[0].note) : 'Risk is shown after hiding.') + '</div>') : '<div class="tiny">' + (S.previewing ? 'Measuring on a copy...' : 'Nothing to check yet.') + '</div>') +
    '<div class="issues" style="margin-top:8px">' + (pr.map((x) => '<div class="p">' + ICON.warn + '<span>' + esc(x) + '</span></div>').join('') || (done ? '<div class="k">' + ICON.check + '<span>' + esc(doneText()) + '. New names are in the Risk column.</span></div>' : '<div class="k">' + ICON.check + '<span>' + esc(p.headline || 'Ready') + '</span></div>')) + '</div></div></div>' +
    '<div class="runbox">' + (S.busy ? '<div class="progtxt" id="progtxt">' + esc(S.progress || '') + '</div>' : '') + '<button class="btn big' + (S.busy ? ' stop' : '') + '" id="run" data-run ' + ((S.busy ? S.stopping : pr.length) ? 'disabled' : '') + '>' + (S.busy ? (S.stopping ? '<span class="spin"></span>Stopping...' : ICON.stop + 'Stop') : ICON.play + (p.kind === 'text' ? 'Make text' : p.kind === 'split' ? 'Split and hide' : p.kind === 'bulk' ? 'Hide all' : 'Hide')) + '</button></div>';
}

// ---------------------------------------------------------------- Reveal
function revealZones() {
  const r = S.reveal, by = {};
  if (S.result && S.result.tab === 'reveal') S.result.items.forEach((x) => { (by[x.name] = by[x.name] || []).push(x); });
  const hasRes = !!(S.result && S.result.tab === 'reveal');
  const setRows = hasRes ? S.result.sets.map((x) => '<tr><td></td><td class="nm"><b>split ' + esc(x.set) + '</b></td><td><span class="rk ' + (x.output ? 'low' : 'mid') + '">' + (x.output ? 'rebuilt' : 'needs ' + x.missing) + '</span></td><td class="mono ok">' + (x.output ? esc(baseName(x.output)) : '') + '</td><td></td></tr>').join('') : '';
  const files = zoneShell('reveal', 'Reveal', r.files, listToolbar('reveal', 'addReveal', 'addRevealDir', 'pasteReveal', r.files),
    r.files.length ? '<table><thead><tr>' + sh('reveal', 'idx', '#', 'c-idx') + sh('reveal', 'name', 'File') + (hasRes ? sh('reveal', 'result', 'Result', 'c-state') + sh('reveal', 'saved', 'Saved as', 'c-res') : sh('reveal', 'size', 'Size', 'c-size')) + '<th class="c-act"></th></tr></thead><tbody>' + sorted('reveal', r.files, { result: (f) => ((by[f.name] || [])[0] || {}).status || '', saved: (f) => ((by[f.name] || []).filter((y) => y.output).map((y) => baseName(y.output))[0] || '') }).map(({ f, i }) => {
      const rs = by[f.name] || [], x = rs[0];
      const head = '<tr ' + rowAttrs('reveal', i) + '><td class="dim">' + (i + 1) + '</td><td class="nm" title="' + esc(f.path) + ' (' + fmt(f.size) + ')">' + nameCell(f) + '</td>';
      const tail = '<td class="act">' + rowOps('reveal', i) + '</td></tr>';
      if (!hasRes) return head + '<td class="dim">' + fmt(f.size) + '</td>' + tail;
      return head + '<td>' + (x ? '<span class="rk ' + (x.status === 'file' ? 'low' : x.status === 'share' ? 'mid' : '') + '">' + (x.status === 'file' ? (rs.length > 1 ? rs.length + ' found' : 'found') : x.status === 'share' ? 'share' : x.status === 'skipped' ? 'skipped' : 'nothing') + '</span>' : '') + '</td><td class="mono ok">' + rs.filter((y) => y.output).map((y) => esc(baseName(y.output))).join('<br>') + '</td>' + tail;
    }).join('') + setRows + '</tbody></table>' : emptyBody('Drop the files or folders to open here.', 'addReveal', 'addRevealDir', 'pasteReveal'));
  const pwz = zoneShell('passes', 'Passphrases', r.passes, '<button class="tbtn" data-showpass title="' + (showPass ? 'Hide' : 'Show') + ' the passphrases" aria-label="Show or hide">' + (showPass ? ICON.eyeOff : ICON.eye) + '</button><button class="tbtn" data-passpaste title="Paste a list of passphrases" aria-label="Paste list">' + ICON.paste + '</button>' + (r.passes.length ? '<button class="tbtn" data-act="clearPasses">' + ICON.trash + '</button>' : ''),
    '<table><thead><tr><th class="c-idx">#</th><th>Passphrase</th><th class="c-act1"></th></tr></thead><tbody>' + r.passes.map((p, i) => '<tr><td class="dim">' + (i + 1) + '</td><td class="mono"><span class="ic">' + ICON.key + '</span> ' + (showPass ? esc(p) : stars(p)) + '</td><td class="act">' + ib('close', 'removePass', i, 'Remove', 'del') + '</td></tr>').join('') + (!r.passes.length ? '<tr><td></td><td class="dim">Add every passphrase you might have used.</td><td></td></tr>' : '') + '</tbody></table><div style="padding:8px 10px"><div class="pwrow"><input class="in" id="draft" type="' + (showPass ? 'text' : 'password') + '" data-bind="reveal.draft" data-enter="addPass" value="' + esc(r.draft) + '" placeholder="type a passphrase and press Enter"><button data-act="addPass" title="Add">' + ICON.plus + '</button></div><div class="tiny" style="margin-top:6px">Every passphrase is tried on every file.</div></div>');
  return [files, pwz];
}
function revealPanel() {
  const o = S.opts, pr = problems(), r = S.reveal;
  const hasText = r.files.some((f) => f.kind === 'text');
  const style = S.styles.find((x) => x.id === o.style) || { adjustable_order: false };
  return '<div class="pb">' +
    '<div class="sec"><h4>' + ICON.sliders + 'Search</h4><div class="kv"><label>Sub-folders</label><label class="chk"><input type="checkbox" data-opt="recursive" ' + (o.recursive ? 'checked' : '') + '> included</label><label>Names only</label><label class="chk"><input type="checkbox" data-opt="namesOnly" ' + (o.namesOnly ? 'checked' : '') + '> only files with a tagged name</label></div></div>' +
    '<div class="sec"><h4>' + ICON.download + 'Output</h4><div class="kv"><label>Save in</label><div class="pwrow"><input class="in" data-opt="outDir" value="' + esc(o.outDir) + '" placeholder="recovered/ next to the files"><button data-browse title="Choose a folder">' + ICON.folderPlus + '</button></div></div></div>' +
    '<details class="sec" data-sec="rset" ' + secOpen('rset', false) + '><summary>' + CHEV + ICON.sliders + 'Settings<span class="tag">parity ' + o.parity + '</span></summary><div class="kv">' + rng('parity', 'Parity', 0, 64, o.parity, o.parity ? o.parity + ' B' : 'off', 2) + rng('rate', 'Slot size', 2, 30, o.rate, o.rate + '%') + '<label>KDF</label>' + optSelect('kdf', [['standard', 'Standard'], ['strong', 'Strong']], o.kdf) + '</div></details>' +
    (hasText ? '<details class="sec" data-sec="rtxt" ' + secOpen('rtxt', true) + '><summary>' + CHEV + ICON.text + 'Text files<span class="tag">' + o.style + '</span></summary><div class="kv"><label>Style</label>' + optSelect('style', S.styles.map((s) => [s.id, s.id]), o.style) + (style.adjustable_order ? rng('order', 'Realism', 1, 6, o.order, o.order) : '') + '</div></details>' : '') +
    '<div class="sec"><h4>Check</h4><div class="kv"><label>Work</label><div class="tiny">' + r.files.length + ' file(s) x ' + r.passes.length + ' passphrase(s) = ' + r.files.length * r.passes.length + ' tries</div></div><div class="issues" style="margin-top:8px">' + (pr.map((p) => '<div class="p">' + ICON.warn + '<span>' + esc(p) + '</span></div>').join('') || '<div class="k">' + ICON.check + '<span>Ready. A wrong passphrase looks like an empty file.</span></div>') + '</div></div></div>' +
    '<div class="runbox">' + (S.busy ? '<div class="progtxt" id="progtxt">' + esc(S.progress || '') + '</div>' : '') + '<button class="btn big' + (S.busy ? ' stop' : '') + '" id="run" data-run ' + ((S.busy ? S.stopping : pr.length) ? 'disabled' : '') + '>' + (S.busy ? (S.stopping ? '<span class="spin"></span>Stopping...' : ICON.stop + 'Stop') : ICON.play + 'Open') + '</button></div>';
}

// ---------------------------------------------------------------- Analyze
function analyzeZones() {
  const a = S.analyze, res = S.result && S.result.tab === 'analyze' ? S.result.map : {};
  const rows = sorted('analyze', a.files, { score: (f) => (res[f.path] || { score: -1 }).score });
  return [zoneShell('analyze', 'Images to scan', a.files, listToolbar('analyze', 'addAnalyze', 'addAnalyzeDir', 'pasteAnalyze', a.files),
    a.files.length ? '<table><thead><tr>' + sh('analyze', 'idx', '#', 'c-idx') + sh('analyze', 'name', 'Image') + sh('analyze', 'size', 'Size', 'c-size') + sh('analyze', 'score', 'Score', 'c-state') + sh('analyze', 'gauge', 'How easy to spot', 'c-gauge') + '<th class="c-act"></th></tr></thead><tbody>' + rows.map(({ f, i }) => {
      const r = res[f.path];
      return '<tr ' + rowAttrs('analyze', i) + '><td class="dim">' + (i + 1) + '</td><td class="nm" title="' + esc(f.path) + '">' + nameCell(f) + '</td><td class="dim">' + esc(f.summary || '') + '</td><td>' + (r && r.score != null ? '<span class="rk ' + r.cls + '">' + Math.round(r.score) + '</span>' : r && r.error ? '<span class="tiny" style="color:var(--red)" title="' + esc(r.error) + '">n/a</span>' : '') + '</td><td>' + (r && r.score != null ? gaugeBar(r.score) : r && r.error ? '<span class="tiny">' + esc(r.error.slice(0, 60)) + '</span>' : '') + '</td><td class="act">' + rowOps('analyze', i) + '</td></tr>';
    }).join('') + '</tbody></table>' : emptyBody('Drop the images to scan here.', 'addAnalyze', 'addAnalyzeDir', 'pasteAnalyze')), ''];
}
function analyzePanel() {
  const pr = problems();
  return '<div class="pb"><div class="sec"><h4>' + ICON.sliders + 'Detectors</h4><div class="tiny" style="line-height:1.6">Chi-square, RS and SPA for images.<br>Calibrated histogram for JPEG.</div></div><div class="sec"><h4>Check</h4><div class="issues">' + (pr.map((p) => '<div class="p">' + ICON.warn + '<span>' + esc(p) + '</span></div>').join('') || '<div class="k">' + ICON.check + '<span>Ready. A low score is a good sign, not proof.</span></div>') + '</div></div></div><div class="runbox">' + (S.busy ? '<div class="progtxt" id="progtxt">' + esc(S.progress || '') + '</div>' : '') + '<button class="btn big' + (S.busy ? ' stop' : '') + '" id="run" data-run ' + ((S.busy ? S.stopping : pr.length) ? 'disabled' : '') + '>' + (S.busy ? (S.stopping ? '<span class="spin"></span>Stopping...' : ICON.stop + 'Stop') : ICON.play + 'Scan') + '</button></div>';
}
function doneText() {
  const r = S.result;
  if (!r) return '';
  if (r.tab === 'hide') { if (r.layout === 'text') return 'Cover text ready'; const n = r.items.filter((i) => i.output).length; return 'Saved ' + n + (n === 1 ? ' file' : ' files'); }
  if (r.tab === 'reveal') { const n = r.items.filter((x) => x.status === 'file').length + r.sets.filter((x) => x.output).length; return n ? 'Recovered ' + n + (n === 1 ? ' item' : ' items') : 'Nothing found'; }
  return 'Scanned ' + Object.keys(r.map).length + ' image(s)';
}

// ---------------------------------------------------------------- render
function enhanceSelects(root) {
  $$('select[data-opt]', root).forEach((s) => {
    const wrap = document.createElement('div');
    wrap.className = 'dd';
    const cur = s.options[s.selectedIndex];
    wrap.innerHTML = '<button type="button"><span>' + esc(cur ? cur.textContent : '') + '</span>' + ICON.chevD + '</button><ul hidden>' + [...s.options].map((o) => '<li data-v="' + esc(o.value) + '" class="' + (o.selected ? 'on' : '') + '">' + esc(o.textContent) + ICON.check + '</li>').join('') + '</ul>';
    s.style.display = 'none';
    s.parentNode.insertBefore(wrap, s);
    wrap.addEventListener('click', (e) => {
      const li = e.target.closest('li');
      if (li) { s.value = li.dataset.v; s.dispatchEvent(new Event('change', { bubbles: true })); return; }
      if (e.target.closest('button')) {
        const ul = $('ul', wrap), open = ul.hidden;
        $$('.dd.open').forEach((d) => { d.classList.remove('open'); $('ul', d).hidden = true; });
        ul.hidden = !open; wrap.classList.toggle('open', open);
        if (open) { const r = ul.getBoundingClientRect(); ul.classList.toggle('up', r.bottom > innerHeight - 8 && wrap.getBoundingClientRect().top > r.height + 16); }
      }
    });
  });
}
function fillRanges(root) { $$('input[type=range]', root).forEach((r) => { const min = +r.min || 0, max = +r.max || 100; r.style.setProperty('--p', (((+r.value - min) / (max - min || 1)) * 100) + '%'); }); }

function keepFocus(fn) {
  const el = document.activeElement, id = el && el.id, a = el && el.selectionStart, b = el && el.selectionEnd;
  fn();
  if (id) { const n = document.getElementById(id); if (n) { n.focus(); try { n.setSelectionRange(a, b); } catch (x) { /* not a text box */ } } }
}
// Replace a block of the page only when its markup really changed, and keep its scroll positions.
function patch(el, html) {
  if (el.__h === html) return;
  const scrolls = $$('.zb,.pb', el).map((x) => x.scrollTop);
  el.innerHTML = html;
  el.__h = html;
  $$('.zb,.pb', el).forEach((x, i) => { x.scrollTop = scrolls[i] || 0; });
  enhanceSelects(el); fillRanges(el);
}
function render() {
  keepFocus(() => {
    patch($('#tabs'), [['hide', 'Hide'], ['reveal', 'Reveal'], ['analyze', 'Analyze']].map(([k, l]) => '<button class="tab ' + (S.tab === k ? 'on' : '') + '" data-tab="' + k + '">' + l + '</button>').join(''));
    const w = $('#work');
    w.className = 'work' + (S.tab === 'analyze' ? ' one' : '');
    if (!$('#z1')) w.innerHTML = '<div id="z1" class="dc"></div><div id="z2" class="dc"></div><div id="zp" class="dc"></div>';
    const zones = S.tab === 'hide' ? hideZones() : S.tab === 'reveal' ? revealZones() : analyzeZones();
    patch($('#z1'), zones[0]); patch($('#z2'), zones[1] || '');
    patch($('#zp'), '<aside class="panel">' + (S.tab === 'hide' ? hidePanel() : S.tab === 'reveal' ? revealPanel() : analyzePanel()) + '</aside>');
  });
}

// ---------------------------------------------------------------- jobs
const waiters = new Map(), early = new Map();
function onJob(ev) {
  const w = waiters.get(ev.id);
  if (!w) { (early.get(ev.id) || early.set(ev.id, []).get(ev.id)).push(ev); return; }
  deliver(w, ev);
}
function deliver(w, ev) {
  if (ev.event === 'step') { S.progress = ev.message; const t = $('#progtxt'); if (t) t.textContent = ev.message; return; }
  waiters.delete(w.id);
  w.resolve(ev);
}
async function jobAsync(spec) {
  const id = await api.startJob(spec);
  return new Promise((resolve) => {
    const w = { id, resolve };
    waiters.set(id, w);
    for (const ev of early.get(id) || []) { if (!waiters.has(id)) break; deliver(w, ev); }
    early.delete(id);
  });
}
function setBusy(b) {
  S.busy = b;
  if (!b) S.stopping = false;
  render();
}
const payloadDto = (f) => (f.typed ? { text: f.text } : { path: f.path });
function wipe() { const h = S.hide; h.pass = ''; h.confirm = ''; h.confirm2 = ''; if (h.second) h.second.pass = ''; S.reveal.draft = ''; S.reveal.passes = []; }

function stopJob() {
  if (!S.busy || S.stopping) return;
  S.stopping = true;
  render();
  api.cancelJob().catch(() => {});
}

async function run() {
  if (S.busy || problems().length) return;
  const h = S.hide, o = S.opts, jobTab = S.tab;
  S.progress = ''; S.result = null;
  setBusy(true);
  try {
    if (S.tab === 'hide') {
      const p = plan();
      if (p.kind === 'text') {
        const st = S.styles.find((x) => x.id === o.style) || {};
        const ev = await jobAsync({ kind: 'text_hide', message: h.message, pass: h.pass, style: o.style, order: st.adjustable_order ? o.order : null, save: null });
        if (ev.event === 'error') throw ev.message;
        S.result = { tab: 'hide', layout: 'text', text: ev.result.text, words: ev.result.words, items: [] };
      } else {
        const ev = await jobAsync({ kind: 'hide', layout: p.kind, payloads: h.payloads.map(payloadDto), covers: h.covers.map((c) => c.path), second: h.second && h.second.file ? payloadDto(h.second.file) : null, pass: h.pass, pass2: h.second ? h.second.pass : null, k: Math.min(h.k, h.covers.length), naming: { keyed: 'ordinary', suffix: 'suffix', custom: 'pattern' }[o.naming], pattern: o.pattern, out_dir: o.outDir || null, mode: o.mode, parity: o.parity, rate: o.rate / 100, kdf: o.kdf, overwrite: o.overwrite, extra: o.extra });
        if (ev.event === 'error') throw ev.message;
        S.result = { tab: 'hide', layout: ev.result.layout, items: ev.result.items };
      }
      const failed = S.result.items.filter((i) => i.error).length;
      if (failed) toast(failed + ' file(s) failed. See the Saved as column.', { err: true, ms: 6000 }); else celebrate(doneText());
    } else if (S.tab === 'reveal') {
      const r = S.reveal;
      if (r.draft) { r.passes.push(r.draft); r.draft = ''; }
      const st = S.styles.find((x) => x.id === o.style) || {};
      const ev = await jobAsync({ kind: 'bulk', inputs: r.files.map((f) => f.path), passes: r.passes, out_dir: o.outDir || null, recursive: false, names_only: o.namesOnly, text: r.files.some((f) => f.kind === 'text') ? { style: o.style, order: st.adjustable_order ? o.order : null } : null, parity: o.parity, rate: o.rate / 100, kdf: o.kdf });
      if (ev.event === 'error') throw ev.message;
      S.result = { tab: 'reveal', items: ev.result.items, sets: ev.result.sets, outDir: ev.result.out_dir };
      celebrate(doneText());
    } else {
      const map = {};
      let i = 0;
      for (const f of S.analyze.files) {
        if (S.stopping) break;
        S.progress = 'Scanning ' + (++i) + ' of ' + S.analyze.files.length + '...';
        const t = $('#progtxt'); if (t) t.textContent = S.progress;
        const ev = await jobAsync({ kind: 'analyze', image: f.path });
        if (ev.event === 'error') map[f.path] = { error: ev.message };
        else map[f.path] = { score: ev.result.report.score, cls: riskOf(ev.result.report.score)[0] };
        S.result = { tab: 'analyze', map };
        render();
      }
      if (S.stopping) toast('Stopped', { ms: 2500 }); else celebrate(doneText());
    }
    if (jobTab !== 'analyze') wipe();
  } catch (e) {
    if (S.stopping || /^Stopped\.?$/.test(msgOf(e))) toast('Stopped. Nothing was left half-written.', { ms: 3500 });
    else toast(msgOf(e), { err: true, ms: 7000 });
  }
  setBusy(false);
}

// ---------------------------------------------------------------- actions
async function act(name, arg) {
  const h = S.hide, r = S.reveal, A = {
    tab: () => { S.tab = arg; S.result = null; sel = { list: 'payloads', i: -1 }; focusZone = { hide: 'payloads', reveal: 'reveal', analyze: 'analyze' }[arg]; S.prev = null; if (arg === 'hide') schedulePreview(); render(); },
    addPayload: () => pickInto('payloads', false), addPayloadDir: () => pickInto('payloads', true),
    addCover: () => pickInto('covers', false), addCoverDir: () => pickInto('covers', true),
    addReveal: () => pickInto('reveal', false), addRevealDir: () => pickInto('reveal', true),
    addAnalyze: () => pickInto('analyze', false), addAnalyzeDir: () => pickInto('analyze', true),
    pastePayload: () => pasteModal((l) => addPaths('payloads', l), { title: 'Paste paths' }), pasteCover: () => pasteModal((l) => addPaths('covers', l), { title: 'Paste paths' }),
    pasteReveal: () => pasteModal((l) => addPaths('reveal', l), { title: 'Paste paths' }), pasteAnalyze: () => pasteModal((l) => addPaths('analyze', l), { title: 'Paste paths' }),
    dropList: () => { const [k, i] = String(arg).split(':'); listOf(k).splice(+i, 1); S.result = null; sel.i = -1; changed(); },
    move: () => { const [k, i, d] = String(arg).split(':'); const l = listOf(k), j = +i + +d; if (j >= 0 && j < l.length) { [l[+i], l[j]] = [l[j], l[+i]]; } S.result = null; changed(); },
    clearList: () => { listOf(arg).length = 0; S.result = null; changed(); },
    setLayout: () => { h.layout = arg; changed(); },
    toggleSecond: () => { h.second = h.second ? null : { file: null, pass: '' }; if (h.second) h.same = false; h.confirm2 = ''; changed(); },
    chooseSecond: () => chooseSecond(),
    removeSecond: () => { h.second.file = null; changed(); },
    toggleText: () => { h.text = !h.text; if (h.text) h.second = null; S.result = null; changed(); },
    toggleSame: () => { h.same = !h.same; changed(); },
    setOpt: () => { const [k, v] = String(arg).split('='); S.opts[k] = v; changed(); },
    addPass: () => { if (r.draft && !r.passes.includes(r.draft)) { r.passes.push(r.draft); r.draft = ''; } render(); },
    removePass: () => { r.passes.splice(+arg, 1); render(); },
    clearPasses: () => { r.passes = []; render(); },
  };
  const fn = A[name];
  if (fn) await fn();
}

document.addEventListener('click', (e) => {
  const a = e.target.closest('[data-act]');
  const tab = e.target.closest('[data-tab]');
  if (tab) { act('tab', tab.dataset.tab); return; }
  if (e.target.closest('.modalb')) return;
  const zone = e.target.closest('[data-zone]'); if (zone && zone.dataset.zone !== 'second') focusZone = zone.dataset.zone;
  if (a) { e.preventDefault(); act(a.dataset.act, a.dataset.arg); return; }
  if (e.target.closest('[data-run]')) { if (S.busy) stopJob(); else run(); return; }
  const row = e.target.closest('tr[data-row]'); if (row) { sel = { list: row.dataset.rk, i: +row.dataset.row }; render(); return; }
  if (e.target.closest('[data-showpass]')) { showPass = !showPass; render(); return; }
  const th = e.target.closest('[data-sortcol]');
  if (th) {
    const [k, col] = th.dataset.sortcol.split(':'), cur = sortBy[k];
    sortBy[k] = cur && cur.col === col ? { col, dir: -cur.dir } : { col, dir: 1 };
    render();
    return;
  }
  if (e.target.closest('[data-passpaste]')) { pasteModal((lines) => { for (const l of lines) if (!S.reveal.passes.includes(l)) S.reveal.passes.push(l); render(); }, { title: 'Paste passphrases', secret: true, placeholder: 'a passphrase, then Enter', empty: 'No passphrases added yet.' }); return; }
  if (e.target.closest('[data-typetext]')) { typing = !typing; render(); const t = $('#typed'); if (t) t.focus(); return; }
  if (e.target.closest('[data-typecancel]')) { typing = false; typedDraft = ''; render(); return; }
  if (e.target.closest('[data-typeadd]')) {
    if (typedDraft.trim()) { const bytes = bytesOf(typedDraft); S.hide.payloads.push({ typed: true, text: typedDraft, name: 'Typed message ' + (++typedCount), kind: 'text', size: bytes, path: 'typed:' + typedCount }); S.result = null; }
    typing = false; typedDraft = ''; changed(); return;
  }
  if (e.target.closest('[data-browse]')) { api.pickFolders('Choose the folder to save in').then((p) => { if (p.length) { S.opts.outDir = p[0]; render(); } }); return; }
  if (e.target.closest('[data-copytext]')) { copyText(S.result.text); return; }
  if (e.target.closest('[data-savetext]')) { saveText(); return; }
  if (!e.target.closest('.dd')) $$('.dd.open').forEach((d) => { d.classList.remove('open'); $('ul', d).hidden = true; });
});
async function copyText(t) {
  try { await navigator.clipboard.writeText(t); } catch (x) { const ta = document.createElement('textarea'); ta.value = t; document.body.appendChild(ta); ta.select(); document.execCommand('copy'); ta.remove(); }
  toast('Cover text copied');
}
async function saveText() {
  try { const p = await api.saveDialog('cover.txt'); if (!p) return; await api.saveText(p, S.result.text); toast('Saved ' + baseName(p)); } catch (e) { toast(msgOf(e), { err: true }); }
}
document.addEventListener('input', (e) => {
  const t = e.target;
  if (t.id === 'typed') { typedDraft = t.value; const c = $('#typedcount'); if (c) c.textContent = typedInfo(); return; }
  if (t.dataset && t.dataset.bind) {
    const [a, b] = t.dataset.bind.split('.');
    if (b === 'pass2') S.hide.second.pass = t.value; else if (b === 'confirm2') S.hide.confirm2 = t.value; else S[a][b] = t.type === 'range' ? +t.value : t.value;
    if (t.dataset.bind === 'hide.k') changed(); else render();
    return;
  }
  if (t.dataset && t.dataset.opt) {
    S.opts[t.dataset.opt] = t.type === 'range' ? +t.value : t.type === 'checkbox' ? t.checked : t.value;
    if (t.type === 'range') fillRanges();
    changed();
  }
  if (t.type === 'range') fillRanges();
}, true);
document.addEventListener('change', (e) => { const t = e.target; if (t.dataset && t.dataset.opt && (t.tagName === 'SELECT' || t.type === 'checkbox')) { S.opts[t.dataset.opt] = t.type === 'checkbox' ? t.checked : t.value; changed(); } });
document.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.dataset && e.target.dataset.enter) { e.preventDefault(); act(e.target.dataset.enter); } });
document.addEventListener('mouseover', (e) => {
  const row = e.target.closest('tr[data-row]');
  $$('.zb tr.hl').forEach((r) => r.classList.remove('hl'));
  if (row && S.tab === 'hide') $$('tr[data-row="' + row.dataset.row + '"]').forEach((r) => r.classList.add('hl'));
});

// ---------------------------------------------------------------- files dropped from the desktop
function setupDrop() {
  try {
    const wv = window.__TAURI__.webview.getCurrentWebview();
    wv.onDragDropEvent((ev) => {
      const p = ev.payload;
      const at = p.position ? document.elementFromPoint(p.position.x / devicePixelRatio, p.position.y / devicePixelRatio) : null;
      const z = at && at.closest ? at.closest('[data-zone]') : null;
      $$('[data-zone].drag').forEach((x) => x.classList.remove('drag'));
      if (p.type === 'over' || p.type === 'enter') { if (z) z.classList.add('drag'); return; }
      if (p.type !== 'drop' || !z || !p.paths) return;
      dropOn(z.dataset.zone, p.paths);
    });
  } catch (e) { /* not running inside the desktop shell */ }
}
async function dropOn(zone, paths) {
  if (zone === 'second') { if (S.hide.second && paths[0]) { const en = await api.describe([paths[0]], 'any', false); if (en[0]) { S.hide.second.file = toItem(en[0]); changed(); } } return; }
  if (LISTS[zone]) addPaths(zone, paths);
}

// ---------------------------------------------------------------- start-up
function setupWindowControls() {
  let w = null;
  try { w = window.__TAURI__.window.getCurrentWindow(); } catch (e) { /* not running inside the desktop shell */ }
  const bind = (id, fn) => { const b = $('#' + id); if (b) b.onclick = () => { try { fn(); } catch (e) { /* ignore */ } }; };
  bind('wmin', () => w.minimize()); bind('wclose', () => w.close());
}
async function init() {
  setupWindowControls();
  const d = await api.defaults();
  S.opts.parity = d.parity; S.opts.rate = Math.round(d.rate * 100); S.styles = d.styles;
  const st = S.styles.find((s) => s.id === S.opts.style) || S.styles[0];
  if (st) { S.opts.style = st.id; S.opts.order = st.default_order; }
  await api.onJob(onJob);
  setupDrop();
  render();
  setTimeout(() => $('#app').classList.add('ready'), 1200);
}
window.StegoUI = { S, act, setSecond: async (p) => { const en = await api.describe([p], 'any', false); S.hide.second.file = toItem(en[0]); changed(); }, render, run, plan, problems, addPaths, dropOn, doPreview, schedulePreview, wipe, riskOf, setTyped: (t) => { S.hide.payloads.push({ typed: true, text: t, name: 'Typed message ' + (++typedCount), kind: 'text', size: bytesOf(t), path: 'typed:' + typedCount }); changed(); } };
init().catch((e) => { document.body.insertAdjacentHTML('afterbegin', '<pre style="color:#b00;padding:12px">Start-up failed: ' + esc(msgOf(e)) + '</pre>'); });
