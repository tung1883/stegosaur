'use strict';
// Stegosaur desktop UI. All numbers come from Rust: capacity and detectability are measured, never modelled here.

const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmt = (n) => (n >= 1e6 ? (n / 1e6).toFixed(1) + ' MB' : n >= 1024 ? ((n / 1024) >= 100 ? Math.round(n / 1024) : (n / 1024).toFixed(1)) + ' KB' : n + ' B');
const isAudio = (c) => c === 'wav' || c === 'flac';
const baseName = (p) => String(p).split(/[\\/]/).pop();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const svg = (d) => '<svg viewBox="0 0 24 24" aria-hidden="true">' + d + '</svg>';
const ICON = {
  folder: svg('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>'),
  image: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M21 16l-5-5-8 8"/>'),
  audio: svg('<path d="M9 18V6l10-2v12"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>'),
  file: svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>'),
  block: svg('<circle cx="12" cy="12" r="9"/><path d="M5.6 5.6l12.8 12.8"/>'),
  up: svg('<path d="M12 19V5"/><path d="M6 11l6-6 6 6"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  check: svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
};

// ---------- tasks and their file slots ----------
// slot: [id, label, filter, multi]   filter: carrier = any lossless image or audio, image = images only, any = any file
const OPS = [
  { id: 'hide', label: 'Hide', run: () => (mode() === 'text' ? 'Generate text' : S.second ? 'Hide both' : 'Hide it'), slots: () => (S.cover === 'text' ? [] : [['carrier', 'Cover', 'carrier'], ['payload', 'File to hide', 'any']].concat(S.second ? [['hidden', 'Second file', 'any']] : [])) },
  { id: 'reveal', label: 'Reveal', run: () => 'Recover', slots: () => [['carrier', 'Image, audio or text file', 'reveal']] },
  { id: 'analyze', label: 'Analyze', run: () => 'Scan', slots: () => [['image', 'Image to scan', 'image'], ['compare', 'Compare with original (optional)', 'image']] },
  { id: 'split', label: 'Split / Join', run: () => (S.task === 'join' ? 'Rebuild' : 'Split'), slots: () => (S.task === 'join' ? [['joinCarriers', 'Images with shares', 'carrier', true]] : [['secret', 'Secret file', 'any'], ['carriers', 'Carrier images', 'carrier', true]]) },
  { id: 'bulk', label: 'Bulk', run: () => 'Find and recover', slots: () => [] },
];

const S = {
  op: 'hide',
  cover: 'file', second: false, twoSlot: false,
  dir: null,
  listing: null,
  roots: [],
  active: 'carrier',
  files: { carriers: [], joinCarriers: [] },
  pass: '', pass2: '',
  passList: '', recursive: false, namesOnly: false, bulk: null,
  mode: 'match', parity: 16, rate: 10, k: 2,
  task: 'split', style: 'syslog', order: 4, message: '',
  out: '', saveText: '',
  styles: [],
  preview: null, previewError: null, previewing: false,
  pix: null, view: 'stego', pixError: null,
  real: null,
  report: null, reportB: null, analyzing: null,
  textOut: null, textRecovered: null, textEstimate: null,
  busy: false, log: [], step: '',
  started: Date.now(),
};
let previewSeq = 0, viewsSeq = 0, previewTimer = null, viewsTimer = null, estimateTimer = null;

const op = () => OPS.find((o) => o.id === S.op);
// The job the current screen will run. Hide picks embed / hidden / text from what the person chose;
// Reveal tells image-or-audio files from text files.
function mode() {
  if (S.op === 'hide') return S.cover === 'text' ? 'text' : S.second || S.twoSlot ? 'hidden' : 'embed';
  if (S.op === 'reveal') { const f = S.files.carrier; return f && f.kind !== 'carrier' ? 'textreveal' : 'reveal'; }
  return S.op;
}
const isTextMode = () => mode() === 'text' || mode() === 'textreveal';
const slotDefs = () => op().slots();
const file = (id) => S.files[id] || null;
const multi = (id) => slotDefs().some((s) => s[0] === id && s[3]);

// ---------- helpers ----------
function toast(m, o) {
  o = o || {};
  const t = $('#toast');
  t.innerHTML = (o.icon || '') + '<span></span>';
  t.lastChild.textContent = m;
  t.classList.toggle('err', !!o.err);
  t.classList.add('on');
  clearTimeout(toast.h);
  toast.h = setTimeout(() => t.classList.remove('on'), o.ms || 2200);
}

function celebrate(m) {
  const b = $('#logo');
  b.classList.remove('wig');
  void b.offsetWidth;
  b.classList.add('wig');
  toast(m, { icon: ICON.check });
}
function logLine(text, kind) {
  S.log.push({ t: (Date.now() - S.started) / 1000, text, kind: kind || 'd' });
  if (S.log.length > 200) S.log.shift();
  if (kind === 'r') toast(text, { err: true, ms: 7000 });
  else if (S.busy && (!kind || kind === 'd')) { S.step = text; const m2 = $('#msg2'); if (m2) { m2.className = 'msg'; m2.textContent = text; } }
}

function entryOf(e) {
  return { path: e.path, name: e.name, size: e.size, summary: e.summary, samples: e.samples, carrier: e.carrier, kind: e.kind };
}

function allowed(e, filter) {
  if (e.kind === 'dir') return false;
  if (filter === 'carrier') return e.kind === 'carrier';
  if (filter === 'reveal') return e.kind === 'carrier' || e.kind === 'other';
  if (filter === 'image') return e.kind === 'carrier' && !isAudio(e.carrier) && e.carrier !== 'jpeg';
  return true;
}

function setErr(e) {
  return typeof e === 'string' ? e : (e && e.message) || String(e);
}

// ---------- folder navigation ----------
async function openDir(path) {
  try {
    S.listing = await api.listDir(path);
    S.dir = S.listing.dir;
    try { localStorage.setItem('stegosaur.dir', S.dir); } catch (e) { /* storage may be unavailable */ }
  } catch (e) {
    toast(setErr(e));
    return false;
  }
  renderLeft();
  return true;
}

function renderLeft() {
  const l = S.listing;
  const defs = slotDefs();
  const filter = (defs.find((d) => d[0] === S.active) || [])[2];
  const chosen = new Set([].concat(...defs.map((d) => S.files[d[0]]).map((f) => (Array.isArray(f) ? f : f ? [f] : []))).map((f) => f.path));
  const rows = l
    ? l.entries.map((e, i) => {
        const ok = e.kind === 'dir' || (defs.length > 0 && allowed(e, filter));
        const ik = e.kind === 'dir' ? 'folder' : e.kind === 'carrier' ? (isAudio(e.carrier) ? 'audio' : 'image') : e.kind === 'unusable' ? 'block' : 'file';
        const icon = ICON[ik];
        const extra = e.kind === 'dir' ? '' : e.kind === 'carrier' ? esc(e.summary || '') + ' · ' + fmt(e.size) : e.kind === 'unusable' ? (/lossy/i.test(e.note || '') ? 'lossy' : 'unusable') : fmt(e.size);
        return `<div class="entry ${ok ? '' : 'off'} ${chosen.has(e.path) ? 'sel' : ''}" data-i="${i}" title="${esc(e.name)}${e.note ? ' - ' + esc(e.note) : ''}"><span class="ic k-${ik}">${icon}</span><span class="nm">${esc(e.name)}</span><small>${extra}</small></div>`;
      }).join('')
    : '<div class="help">Loading…</div>';
  $('#left').innerHTML = `<div class="crumb"><button id="up" ${l && l.parent ? '' : 'disabled'} title="Up one folder">${ICON.up} Up</button><select id="roots" title="Jump to">${['<option value="">Jump to…</option>'].concat(S.roots.map((r) => `<option value="${esc(r)}">${esc(r)}</option>`)).join('')}</select><button id="browse">Browse…</button></div><div class="path">${esc(S.dir || '')}</div><div class="files" id="files">${rows}</div>`;
  $('#up').onclick = () => l && l.parent && openDir(l.parent);
  $('#roots').onchange = (e) => e.target.value && openDir(e.target.value);
  $('#browse').onclick = async () => {
    try { const p = await api.pickFolder(); if (p) openDir(p); } catch (e) { toast(setErr(e)); }
  };
  $('#files').onclick = (e) => {
    const row = e.target.closest('.entry');
    if (!row || row.classList.contains('off')) return;
    const entry = S.listing.entries[+row.dataset.i];
    if (entry.kind === 'dir') openDir(entry.path); else pick(entry);
  };
}

function pick(entry) {
  const def = slotDefs().find((d) => d[0] === S.active);
  if (!def || !allowed(entry, def[2])) return;
  const f = entryOf(entry);
  if (def[3]) {
    const list = S.files[def[0]];
    if (!list.some((x) => x.path === f.path)) list.push(f);
  } else {
    S.files[def[0]] = f;
    const next = slotDefs().find((d) => (d[3] ? S.files[d[0]].length === 0 : !S.files[d[0]]));
    if (next) S.active = next[0];
  }
  onChange(true);
}

function clearSlot(id, index) {
  if (Array.isArray(S.files[id])) S.files[id].splice(index, 1); else delete S.files[id];
  onChange(true);
}

// ---------- the right-hand inspector ----------
function slotsHTML(keep) {
  return slotDefs().filter((d) => !keep || keep(d)).map(([id, label, , isMulti]) => {
    const on = S.active === id;
    let body;
    if (isMulti) {
      const list = S.files[id];
      body = list.length
        ? list.map((f, i) => `<div class="v"><span>${esc(f.name)}</span><button class="x" data-rm="${id}:${i}" title="Remove">${ICON.close}</button></div>`).join('')
        : '<div class="v empty">click files on the left to add</div>';
    } else {
      const f = S.files[id];
      body = f ? `<div class="v"><span>${esc(f.name)} <span class="more">${f.size != null ? fmt(f.size) : ''}</span></span><button class="x" data-rm="${id}:0" title="Clear">${ICON.close}</button></div>` : '<div class="v empty">click a file on the left</div>';
    }
    return `<div class="slot ${on ? 'on' : ''}" data-slot="${id}"><div class="t">${esc(label)}</div>${body}</div>`;
  }).join('');
}

const seg = (id, opts, val) => `<div class="seg" id="${id}" style="width:100%">${opts.map(([k, l]) => `<button style="flex:1" class="${val === k ? 'on' : ''}" data-v="${k}">${l}</button>`).join('')}</div>`;
const field = (label, inner, id) => `<div class="field"><label>${label} <b id="${id || ''}"></b></label>${inner}</div>`;
const passInput = (id, label, value) => field(`${label}`, `<input class="in" id="${id}" type="password" autocomplete="off" value="${esc(value)}"><div class="meter" style="margin:6px 0 0"><div class="bar"><i id="${id}-m" style="width:0"></i></div></div>`, id + '-w');

function renderRight() {
  const m = mode();
  const modeF = field('Mode', seg('mode', [['match', 'match (recommended)'], ['replace', 'replace']], S.mode), 'mode-v');
  const parF = field('Error correction', `<input id="parity" type="range" min="0" max="64" step="2" value="${S.parity}">`, 'parity-v');
  const rateF = field('Slot size', `<input id="rate" type="range" min="2" max="30" value="${S.rate}">`, 'rate-v');
  const outF = (label, ph) => field(label, `<input class="in" id="out" type="text" placeholder="${esc(ph)}" value="${esc(S.out)}">`);
  const saveF = (label) => field(label, `<input class="in" id="save" type="text" value="${esc(S.saveText)}">`);
  const styleF = () => {
    const sty = S.styles.find((x) => x.id === S.style) || { adjustable_order: false };
    let x = field('Style', `<select id="style">${S.styles.map((o) => `<option value="${o.id}" ${o.id === S.style ? 'selected' : ''}>${o.id}</option>`).join('')}</select>`);
    if (sty.adjustable_order) x += field('Realism (context order)', `<input id="order" type="range" min="1" max="6" value="${S.order}">`, 'order-v');
    return x;
  };
  let h = '', P = '', R = '';
  if (S.op === 'hide') {
    h += `<h4>Hide it in</h4><div class="field">${seg('cover', [['file', 'Image or audio'], ['text', 'Generated text']], S.cover)}</div>`;
    if (m === 'text') {
      h += '<h4>Message</h4>' + styleF() + field('Message', `<textarea id="msg" placeholder="type what to hide">${esc(S.message)}</textarea>`);
      P = passInput('pass', 'Passphrase', S.pass);
      R = saveF('Save to file (optional)');
    } else {
      h += `<h4>Files</h4>${slotsHTML((d) => d[0] !== 'hidden')}` +
        `<div class="row"><div class="grow"><b>Also hide a second file</b><small>It gets its own passphrase. Each one opens only its own file.</small></div><div class="sw ${S.second ? 'on' : ''}" id="second" role="switch" aria-checked="${S.second}" tabindex="0"></div></div>` +
        (S.second ? '' : `<label class="check"><input type="checkbox" id="twoslot" ${S.twoSlot ? 'checked' : ''}><span>Look the same whether or not a second file is hidden</span></label>`);
      if (S.second) h += slotsHTML((d) => d[0] === 'hidden');
      P = passInput('pass', S.second ? 'First passphrase' : 'Passphrase', S.pass) + (S.second ? passInput('pass2', 'Second passphrase', S.pass2) : '');
      R = modeF + parF + (S.second || S.twoSlot ? rateF : '') + outF('Save as', 'auto: camera-style name');
    }
  } else if (S.op === 'bulk') {
    h += '<h4>Folder</h4><div class="slot on"><div class="t">Looks through</div><div class="v"><span>' + esc(S.dir || '') + '</span></div></div><div class="help">The folder shown on the left. Change it there. Every image and audio file in it is checked with every passphrase below.</div>';
    P = field('One passphrase per line', '<textarea class="in pwlist" id="passlist" autocomplete="off" spellcheck="false" placeholder="one passphrase per line">' + esc(S.passList) + '</textarea>', 'passlist-w');
    R = '<label class="check"><input type="checkbox" id="recursive" ' + (S.recursive ? 'checked' : '') + '><span>Also look inside sub-folders</span></label>' +
      '<label class="check"><input type="checkbox" id="namesonly" ' + (S.namesOnly ? 'checked' : '') + '><span>Only try a passphrase on files whose name matches it (faster; needs files named by this tool)</span></label>' +
      outF('Save recovered files in', 'auto: a "recovered" folder inside this folder');
  } else if (S.op === 'reveal') {
    h += `<h4>Read from</h4>${slotsHTML()}`;
    P = passInput('pass', 'Passphrase', S.pass);
    R = m === 'textreveal' ? styleF() + saveF('Save recovered message to (optional)') : parF + rateF + outF('Save recovered file as', 'auto: <name>.recovered.bin');
  } else {
    h += `<h4>Files</h4>${slotsHTML()}`;
    if (S.op === 'split') h += `<div class="field" style="padding-top:10px">${seg('task', [['split', 'Split a secret'], ['join', 'Join shares']], S.task)}</div>`;
    if (S.op === 'analyze') R = '<div class="help">Estimates how much of an image carries a hidden message (chi-square, RS and SPA). Add an original to compare.</div>';
    if (S.op === 'split') {
      P = passInput('pass', 'Passphrase', S.pass);
      if (S.task === 'join') R = parF + outF('Save recovered file as', 'auto: recovered.bin next to the first image');
      else R = field('Shares needed (k)', `<input id="k" type="range" min="1" max="${Math.max(1, S.files.carriers.length)}" value="${Math.min(S.k, Math.max(1, S.files.carriers.length))}">`, 'k-v') + modeF + parF + outF('Save shares in', 'auto: next to the first image');
    }
  }
  if (P) h += '<h4>Passphrase</h4>' + P;
  h += '<h4 id="caph">Preview</h4><div id="dyn"></div>';
  if (R) h += '<h4>Settings</h4>' + R;
  h += '<div class="runbar"><div class="msg" id="msg2"></div><button class="btn" id="run"></button></div>';
  $('#right').innerHTML = h;
  bindRight();
  updateDynamic();
}

function bindRight() {
  const on = (id, ev, fn) => { const e = $('#' + id); if (e) e.addEventListener(ev, fn); };
  $('#right').onclick = (e) => {
    const rm = e.target.closest('[data-rm]');
    if (rm) { e.stopPropagation(); const [id, i] = rm.dataset.rm.split(':'); clearSlot(id, +i); return; }
    const slot = e.target.closest('[data-slot]');
    if (slot) { S.active = slot.dataset.slot; renderRight(); renderLeft(); }
  };
  on('pass', 'input', (e) => { S.pass = e.target.value; touch(); });
  on('pass2', 'input', (e) => { S.pass2 = e.target.value; touch(); });
  on('passlist', 'input', (e) => { S.passList = e.target.value; touch(); });
  on('recursive', 'change', (e) => { S.recursive = e.target.checked; });
  on('namesonly', 'change', (e) => { S.namesOnly = e.target.checked; });
  on('out', 'input', (e) => { S.out = e.target.value; touch(); });
  on('save', 'input', (e) => { S.saveText = e.target.value; touch(); });
  on('parity', 'input', (e) => { S.parity = +e.target.value; S.real = null; changedParams(); });
  on('rate', 'input', (e) => { S.rate = +e.target.value; S.real = null; changedParams(); });
  on('k', 'input', (e) => { S.k = +e.target.value; changedParams(); });
  on('order', 'input', (e) => { S.order = +e.target.value; S.textOut = null; changedParams(); });
  on('msg', 'input', (e) => { S.message = e.target.value; S.textOut = null; changedParams(); });
  on('style', 'change', (e) => { S.style = e.target.value; const st = S.styles.find((s) => s.id === S.style); if (st) S.order = st.default_order; S.textOut = null; renderRight(); changedParams(); });
  const segClick = (id, fn) => { const el = $('#' + id); if (el) el.addEventListener('click', (e) => { const b = e.target.closest('button'); if (b) fn(b.dataset.v); }); };
  segClick('cover', (v) => { S.cover = v; S.active = slotDefs()[0] ? slotDefs()[0][0] : null; S.preview = null; S.previewError = null; S.pix = null; S.real = null; renderChips(); renderRight(); renderLeft(); renderCenter(); schedulePreview(); scheduleEstimate(); });
  const sw = $('#second');
  if (sw) {
    const flip = () => { S.second = !S.second; if (!S.second) delete S.files.hidden; S.active = slotDefs().find((d) => !S.files[d[0]]) ? slotDefs().find((d) => !S.files[d[0]])[0] : S.active; S.real = null; onChange(true); };
    sw.onclick = flip;
    sw.onkeydown = (e) => { if (e.key === ' ' || e.key === 'Enter') { e.preventDefault(); flip(); } };
  }
  on('twoslot', 'change', (e) => { S.twoSlot = e.target.checked; S.real = null; renderRight(); changedParams(); });
  segClick('mode', (v) => { S.mode = v; S.real = null; renderRight(); changedParams(); });
  segClick('task', (v) => { S.task = v; S.out = ''; S.active = slotDefs()[0] ? slotDefs()[0][0] : null; renderRight(); renderLeft(); changedParams(); });
  $('#run').onclick = run;
}

// ---------- problems, parameters, request building ----------
function problems() {
  const p = [];
  const need = (id, msg) => { if (!file(id)) p.push(msg); };
  const m = mode();
  if (m === 'embed' || m === 'hidden') {
    need('carrier', 'Choose a cover file.'); need('payload', 'Choose the file to hide.');
    if (!S.pass) p.push(S.second ? 'Enter the first passphrase.' : 'Enter a passphrase.');
    if (S.second) { need('hidden', 'Choose the second file.'); if (!S.pass2) p.push('Enter the second passphrase.'); else if (S.pass && S.pass === S.pass2) p.push('The two passphrases must differ.'); }
  }
  if (m === 'reveal' || m === 'textreveal') { need('carrier', 'Choose the file to read.'); if (!S.pass) p.push('Enter the passphrase.'); }
  if (m === 'analyze') { need('image', 'Choose an image to scan.'); }
  if (m === 'bulk') { if (!S.dir) p.push('Choose a folder on the left.'); if (!passList().length) p.push('Enter at least one passphrase.'); }
  if (m === 'split') {
    if (S.task === 'join') { if (!S.files.joinCarriers.length) p.push('Add the images that hold shares.'); if (!S.pass) p.push('Enter the passphrase.'); }
    else { need('secret', 'Choose the secret file.'); if (!S.files.carriers.length) p.push('Add at least one carrier image.'); if (!S.pass) p.push('Enter the passphrase.'); }
  }
  if (m === 'text' && !S.message) p.push('Type a message to hide.');
  if (m === 'text' && !S.pass) p.push('Enter the passphrase.');
  if (S.preview && S.preview.error) p.push(S.preview.error);
  if (S.previewError) p.push(S.previewError);
  return p;
}

const passList = () => S.passList.split('\n').map((x) => x.replace(/\r$/, '')).filter((x) => x.length > 0);

function cfgFields() {
  return { mode: S.mode, parity: S.parity, rate: S.rate / 100 };
}

function previewReq(measure) {
  const c = cfgFields();
  const m = mode();
  if (m === 'embed' && file('carrier')) return { op: 'embed', carrier: file('carrier').path, payload_len: file('payload') ? file('payload').size : 0, ...c, measure };
  if (m === 'hidden' && file('carrier')) return { op: 'hidden', carrier: file('carrier').path, payload_len: file('payload') ? file('payload').size : 0, hidden_len: S.second && file('hidden') ? file('hidden').size : null, ...c, measure };
  if (m === 'split' && S.task === 'split' && S.files.carriers.length && file('secret')) return { op: 'split', carriers: S.files.carriers.map((f) => f.path), secret_len: file('secret').size, k: Math.min(S.k, S.files.carriers.length), ...c };
  return null;
}

function viewsReq() {
  const c = cfgFields();
  const base = { max_dim: 640, ...c };
  const m = mode();
  if ((m === 'embed' || m === 'hidden') && file('carrier')) {
    const car = file('carrier');
    if (S.real && S.real.carrier === car.path) return { ...base, op: m, carrier: car.path, stego: S.real.output };
    if (file('payload') && S.preview && S.preview.fits) return { ...base, op: m, carrier: car.path, payload_len: file('payload').size, hidden_len: m === 'hidden' && S.second && file('hidden') ? file('hidden').size : null };
    return { ...base, op: 'embed', carrier: car.path, stego: car.path };
  }
  if (m === 'reveal' && file('carrier')) return { ...base, op: 'embed', carrier: file('carrier').path, stego: file('carrier').path };
  if (m === 'analyze' && file('image')) return { ...base, op: 'embed', carrier: (file('compare') || file('image')).path, stego: file('image').path };
  if (m === 'split' && S.task === 'split' && S.files.carriers[0] && file('secret') && S.preview && S.preview.fits) return { ...base, op: 'embed', carrier: S.files.carriers[0].path, payload_len: file('secret').size + 48 };
  if (m === 'split' && S.files[S.task === 'join' ? 'joinCarriers' : 'carriers'][0]) { const f = S.files[S.task === 'join' ? 'joinCarriers' : 'carriers'][0]; return { ...base, op: 'embed', carrier: f.path, stego: f.path }; }
  return null;
}

// ---------- change handling ----------
function touch() { updateDynamic(); }

function changedParams() {
  updateDynamic();
  schedulePreview();
  scheduleEstimate();
}

function onChange(rerender) {
  if (rerender) { renderRight(); renderLeft(); }
  S.report = null; S.reportB = null;
  schedulePreview();
  scheduleEstimate();
  renderCenter();
}

function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(doPreview, 150);
  clearTimeout(viewsTimer);
  viewsTimer = setTimeout(doViews, 200);
}

async function doPreview() {
  const seq = ++previewSeq;
  const req = previewReq(true);
  if (!req) { S.preview = null; S.previewError = null; S.previewing = false; updateDynamic(); clearTimeout(viewsTimer); viewsTimer = setTimeout(doViews, 0); return; }
  S.previewing = true; updateDynamic();
  try {
    const r = await api.preview(req);
    if (seq !== previewSeq) return;
    S.preview = r; S.previewError = null;
  } catch (e) {
    if (seq !== previewSeq) return;
    S.preview = null; S.previewError = setErr(e);
  }
  S.previewing = false;
  updateDynamic();
  clearTimeout(viewsTimer);
  viewsTimer = setTimeout(doViews, 0);
}

async function doViews() {
  const seq = ++viewsSeq;
  const req = viewsReq();
  if (!req) { S.pix = null; S.pixError = null; renderCenter(); return; }
  try {
    const b = await api.views(req);
    if (seq !== viewsSeq) return;
    const dv = new DataView(b.buffer, b.byteOffset, b.byteLength);
    const w = dv.getUint32(0, true), h = dv.getUint32(4, true), changed = dv.getFloat32(8, true);
    const n = w * h * 4;
    const slice = (i) => new Uint8ClampedArray(b.buffer.slice(b.byteOffset + 12 + i * n, b.byteOffset + 12 + (i + 1) * n));
    S.pix = { w, h, changed, stego: slice(1), orig: slice(0), diff: slice(2), lsb: slice(3) };
    S.pixError = null;
  } catch (e) {
    if (seq !== viewsSeq) return;
    S.pix = null; S.pixError = setErr(e);
  }
  renderCenter();
}

function scheduleEstimate() {
  clearTimeout(estimateTimer);
  if (mode() !== 'text') { S.textEstimate = null; return; }
  estimateTimer = setTimeout(async () => {
    if (!S.message) { S.textEstimate = null; updateDynamic(); return; }
    try { S.textEstimate = await api.textEstimate({ style: S.style, order: S.order, len: new TextEncoder().encode(S.message).length }); } catch (e) { S.textEstimate = null; }
    updateDynamic();
  }, 150);
}

// ---------- dynamic panel parts ----------
const RISK = { low: ['LOW', 'big'], medium: ['MEDIUM', 'big mid'], high: ['HIGH', 'big high'] };
const gaugePos = (s) => (s < 5 ? 5 + s * 2 : s < 20 ? 40 + (s - 5) * 2 : Math.min(95, 70 + (s - 20) * 0.4));
const barColor = (risk) => (risk === 'high' ? 'var(--red)' : risk === 'medium' ? '#f3d36b' : 'var(--green)');

function reportHTML(title, r) {
  const [name, cls] = RISK[r.risk];
  return `<div class="meter"><div class="t"><span><b>${title}</b></span></div></div>
    <div class="meter"><div class="t"><span>RS</span><span>${r.rs.toFixed(3)}</span></div><div class="bar"><i style="width:${Math.min(100, Math.max(0, r.rs * 100))}%;background:${barColor(r.risk)}"></i><u style="left:5%"></u></div></div>
    <div class="meter"><div class="t"><span>SPA</span><span>${r.spa.toFixed(3)}</span></div><div class="bar"><i style="width:${Math.min(100, Math.max(0, r.spa * 100))}%;background:${barColor(r.risk)}"></i><u style="left:5%"></u></div></div>
    <div class="${cls}"><b>${r.score.toFixed(0)} / 100</b> <small>${name} - classical detectors only</small></div>`;
}

function updateDynamic() {
  const dyn = $('#dyn');
  if (!dyn) return;
  const p = S.preview;
  let h = '';
  ['pass', 'pass2'].forEach((id) => {
    const m = $('#' + id + '-m'), w = $('#' + id + '-w'), v = S[id];
    if (m) { const f = v.length < 8 ? 0.25 : v.length < 14 ? 0.6 : 1; m.style.width = (v ? f * 100 : 0) + '%'; m.style.background = f < 0.5 ? 'var(--red)' : 'var(--green)'; if (w) w.textContent = v ? (v.length < 8 ? 'weak' : v.length < 14 ? 'okay' : 'strong') : ''; }
  });
  const setT = (id, t) => { const e = $('#' + id); if (e) e.textContent = t; };
  setT('parity-v', S.parity ? S.parity + ' bytes' : 'off');
  setT('rate-v', S.rate + ' %');
  setT('k-v', S.k + (S.files.carriers.length ? ' of ' + S.files.carriers.length : ''));
  setT('order-v', S.order);

  if ((mode() === 'embed' || mode() === 'hidden') && p) {
    const f = p.capacity ? Math.min(1, p.need / p.capacity) : 1;
    const label = mode() === 'hidden' ? 'each slot holds' : 'capacity';
    h += `<div class="slots"><i style="width:${Math.max(f * 100, 0)}%;background:${p.fits ? '#8fbf5f' : '#d2573f'}">${f > 0.18 ? fmt(p.need) : ''}</i><i style="flex:1;background:#ebe4c8;color:var(--dim)">${p.fits ? fmt(p.capacity - p.need) + ' free' : 'over by ' + fmt(p.need - p.capacity)}</i></div><div class="meter"><div class="t"><span>${label} (${esc(p.summary || '')})</span><b>${fmt(p.capacity)}</b></div></div>`;
    h += '<h4>How hidden is it?</h4><div class="gaugew"><div class="gauge"><i id="gp" style="left:' + (p.measured ? gaugePos(p.measured.after.score) : 5) + '%"></i></div><div class="row2"><span>Hard to spot</span><span>Easy to spot</span></div></div>';
    if (S.previewing && !p.measured) h += '<div class="help">measuring on a copy…</div>';
    else if (p.measured) h += reportHTML('Measured on a copy (untouched image: ' + p.measured.before.score.toFixed(0) + ')', p.measured.after);
    else if (p.detect_note) h += `<div class="help">${esc(p.detect_note)}</div>`;
  } else if (mode() === 'split' && S.task === 'split' && p && p.split) {
    h += `<div class="meter"><div class="t"><span>each share</span><b>${fmt(p.split.share_len)}</b></div></div>` + p.split.carriers.map((c, i) => `<div class="meter"><div class="t"><span>${esc(S.files.carriers[i] ? S.files.carriers[i].name : 'image ' + (i + 1))}</span><span class="${c.fits ? 'okline' : 'errline'}">holds ${fmt(c.capacity)} - ${c.fits ? 'fits' : 'too small'}</span></div></div>`).join('') + (S.files.carriers.length ? `<div class="help">Any ${Math.min(S.k, S.files.carriers.length)} of ${S.files.carriers.length} images rebuild the secret.</div>` : '');
  } else if (mode() === 'bulk') {
    const b = S.bulk;
    if (b) {
      const f = b.items.filter((i) => i.status === 'file').length, sh = b.items.filter((i) => i.status === 'share').length, n = b.items.filter((i) => i.status === 'nothing').length;
      h += '<div class="meter"><div class="t"><span>files checked</span><b>' + b.checked + '</b></div></div><div class="meter"><div class="t"><span>files recovered</span><b>' + f + '</b></div></div><div class="meter"><div class="t"><span>shares found</span><b>' + sh + '</b></div></div><div class="meter"><div class="t"><span>nothing found</span><b>' + n + '</b></div></div>' + b.sets.map((x) => '<div class="meter"><div class="t"><span>split ' + esc(x.set) + '</span><span class="' + (x.output ? 'okline' : 'errline') + '">' + (x.output ? 'rebuilt' : x.missing + ' more share(s) needed') + '</span></div></div>').join('');
    } else h += '<div class="help">Nothing has been run yet. Results appear in the middle.</div>';
  } else if (mode() === 'analyze' && S.report) {
    h += reportHTML('Image', S.report) + `<div class="help">chi-square p = ${S.report.chi_p.toFixed(2)} (unreliable on smooth histograms)</div>`;
    if (S.reportB) h += reportHTML('Original', S.reportB) + `<div class="help"><b>change:</b> RS ${(S.report.rs - S.reportB.rs >= 0 ? '+' : '') + (S.report.rs - S.reportB.rs).toFixed(3)} &nbsp; SPA ${(S.report.spa - S.reportB.spa >= 0 ? '+' : '') + (S.report.spa - S.reportB.spa).toFixed(3)}</div>`;
  } else if (mode() === 'reveal') {
    h += '<div class="help">Tries the normal layout first, then the two-passphrase (Hidden) layout. A wrong passphrase looks exactly like an image with nothing hidden in it, by design. Use the same error correction and slot size as when hiding.</div>';
  } else if (isTextMode()) {
    const sty = S.styles.find((s) => s.id === S.style);
    if (sty && mode() === 'text') h += `<div class="meter"><div class="t"><span>capacity</span><b>${(S.textEstimate ? S.textEstimate.bits_per_word : sty.bits_per_word).toFixed(2)} bits/word</b></div></div><div class="meter"><div class="t"><span>well-formed lines</span><b>${(S.textEstimate ? S.textEstimate.well_formed : sty.well_formed) != null ? (S.textEstimate ? S.textEstimate.well_formed : sty.well_formed) + '%' : 'n/a'}</b></div></div>`;
    if (S.textEstimate && mode() === 'text') h += `<div class="meter"><div class="t"><span>this message becomes</span><b>~${S.textEstimate.words} words / ${S.textEstimate.lines} lines</b></div></div>`;
    if (mode() === 'textreveal' && S.textRecovered != null) h += `<div class="help"><b>Recovered message</b></div><div class="outtext">${esc(S.textRecovered)}</div>`;
  }
  dyn.innerHTML = h;
  const caph = $('#caph');
  if (caph) caph.textContent = S.op === 'bulk' ? 'Summary' : S.op === 'split' ? 'Shares' : isTextMode() ? (mode() === 'textreveal' ? 'Recovered message' : 'Cover text') : S.op === 'analyze' ? 'Result' : 'Preview';

  const probs = problems();
  const run = $('#run'), m = $('#msg2');
  if (run) {
    run.textContent = S.busy ? 'Working…' : op().run();
    run.disabled = S.busy || probs.length > 0;
    run.classList.toggle('busy', S.busy);
  }
  if (m) { m.className = 'msg ' + (probs.length ? 'r' : 'g'); m.textContent = probs.length ? probs[0] : S.busy ? (S.step || 'Working') : 'Ready.'; }
  renderCenterStat();
}

// ---------- center and chrome ----------
function renderChips() {
  const c = file('carrier') || file('image');
  $('#ctx').textContent = c ? c.name + (c.summary ? '  ' + c.summary : '') : '';
  $('#chips').innerHTML = OPS.map((o) => `<span class="chip ${o.id === S.op ? 'on' : ''}" data-op="${o.id}">${o.label}</span>`).join(' ');
}

function renderCenterStat() {
  const st = $('#stat');
  if (!st) return;
  const mm = mode();
  if (mm === 'text') st.textContent = S.textOut ? S.textOut.split('\n').length + ' lines' : '';
  else if (S.pix) st.textContent = S.real && (mm === 'embed' || mm === 'hidden') ? `changed ${(S.pix.changed * 100).toFixed(2)}% of samples (the saved file)` : mm === 'embed' || mm === 'hidden' || mm === 'split' ? `changed ${(S.pix.changed * 100).toFixed(2)}% of samples (preview)` : mm === 'analyze' && file('compare') ? `differs from the original in ${(S.pix.changed * 100).toFixed(2)}% of samples` : '';
  else st.textContent = '';
}

function bulkHTML() {
  const b = S.bulk;
  if (!b) return '<div class="empty-stage"><img src="icon.png" alt=""><div>Pick a folder on the left and enter your passphrases. Every file is tried with every passphrase, so you do not need to know which file holds what.</div></div>';
  const note = { file: (i) => '<b>' + esc(i.detail) + '</b> &middot; ' + fmt(i.bytes), share: (i) => esc(i.detail), nothing: () => 'nothing found', skipped: (i) => 'skipped: ' + esc(i.detail) };
  const rows = b.items.map((i) => '<tr class="' + i.status + '"><td class="nm">' + esc(i.name) + '</td><td>' + note[i.status](i) + '</td><td class="out">' + (i.output ? esc(baseName(i.output)) : '') + '</td></tr>').join('');
  const sets = b.sets.map((x) => '<tr class="' + (x.output ? 'file' : 'share') + '"><td class="nm">split ' + esc(x.set) + '</td><td>' + (x.output ? '<b>rebuilt</b> from ' + x.found + ' of ' + x.n + ' shares &middot; ' + fmt(x.bytes) : 'found ' + x.found + ' of the ' + x.k + ' shares needed &mdash; ' + x.missing + ' more required') + '</td><td class="out">' + (x.output ? esc(baseName(x.output)) : '') + '</td></tr>').join('');
  return '<div class="bulk"><table><thead><tr><th>File</th><th>Found</th><th>Saved as</th></tr></thead><tbody>' + rows + sets + '</tbody></table><div class="help">Saved in ' + esc(b.out_dir) + '. Files that show nothing either hold nothing or need a passphrase that is not in the list.</div></div>';
}

function renderCenter() {
  const stage = $('#stage'), tools = $('#tools'), legend = $('#legend');
  if (mode() === 'bulk') {
    tools.innerHTML = '<b>Bulk recovery</b><span style="flex:1"></span><span id="stat">' + (S.bulk ? S.bulk.checked + ' files checked' : '') + '</span>';
    stage.innerHTML = bulkHTML();
    legend.innerHTML = '<span>A wrong passphrase looks the same as a file with nothing hidden, so a missing result can also mean a missing passphrase.</span>';
    return;
  }
  if (isTextMode()) {
    tools.innerHTML = '<b>Cover text</b><span style="flex:1"></span><span id="stat"></span>';
    stage.innerHTML = '<div class="textview" id="tv"></div>';
    $('#tv').textContent = S.textOut || (mode() === 'textreveal' ? 'Press Recover to read the hidden message from this text file. It will appear in the panel on the right.' : 'Type a message, then press Generate. The text is real output of the tool.');
    legend.innerHTML = '<span>The text hides your message; keep the style and realism setting to read it back.</span>';
    renderCenterStat();
    return;
  }
  if (!S.pix) {
    tools.innerHTML = '<span style="flex:1"></span><span id="stat"></span>';
    stage.innerHTML = `<div class="empty-stage"><img src="icon.png" alt=""><div>${S.pixError ? esc(S.pixError) : 'Choose a cover image to see it here, and what hiding does to it.'}</div></div>`;
    legend.innerHTML = '';
    renderCenterStat();
    return;
  }
  const viewOnly = S.op === 'analyze' || mode() === 'reveal';
  const views = [['stego', viewOnly ? 'Image' : 'Stego'], ['orig', 'Original'], ['diff', 'Difference'], ['lsb', 'LSB plane']].filter(([k]) => !(viewOnly && k === 'orig'));
  if (!views.some(([k]) => k === S.view)) S.view = 'stego';
  tools.innerHTML = `<div class="seg" id="views">${views.map(([k, l]) => `<button class="${S.view === k ? 'on' : ''}" data-v="${k}">${l}</button>`).join('')}</div><span style="flex:1"></span><span id="stat"></span>`;
  const hadCanvas = !!$('#cv');
  stage.innerHTML = '<canvas id="cv"></canvas>';
  if (hadCanvas) $('#cv').style.animation = 'none'; // pop in only when the picture first appears, not on every refresh
  legend.innerHTML = '<span><i class="sw" style="background:#ee8a3f"></i>changed</span><span class="hover" id="hov">hover for pixel values</span>';
  $('#views').onclick = (e) => { const b = e.target.closest('button'); if (b) { S.view = b.dataset.v; renderCenter(); } };
  drawCanvas();
  renderCenterStat();
}

function drawCanvas() {
  const cv = $('#cv'), px = S.pix;
  if (!cv || !px) return;
  cv.width = px.w; cv.height = px.h;
  const data = { stego: px.stego, orig: px.orig, diff: px.diff, lsb: px.lsb }[S.view] || px.stego;
  cv.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(data), px.w, px.h), 0, 0);
  cv.onmousemove = (e) => {
    const r = cv.getBoundingClientRect(), x = Math.floor(((e.clientX - r.left) / r.width) * px.w), y = Math.floor(((e.clientY - r.top) / r.height) * px.h);
    if (x < 0 || y < 0 || x >= px.w || y >= px.h) return;
    const i = (y * px.w + x) * 4, d = (S.view === 'orig' ? px.orig : px.stego);
    $('#hov').textContent = `x ${x} y ${y} - R ${d[i]} G ${d[i + 1]} B ${d[i + 2]} (LSB ${d[i] & 1}${d[i + 1] & 1}${d[i + 2] & 1})`;
  };
  cv.onmouseleave = () => { $('#hov').textContent = 'hover for pixel values'; };
}

// ---------- running jobs ----------
function jobSpec() {
  const f = (id) => file(id).path;
  const m = mode();
  if (m === 'embed') return { kind: 'embed', carrier: f('carrier'), payload: f('payload'), output: S.out || null, pass: S.pass, mode: S.mode, parity: S.parity };
  if (m === 'hidden') return { kind: 'hidden', carrier: f('carrier'), public_file: f('payload'), public_pass: S.pass, hidden_file: S.second ? f('hidden') : null, hidden_pass: S.second ? S.pass2 : null, output: S.out || null, mode: S.mode, parity: S.parity, rate: S.rate / 100 };
  if (m === 'reveal') return { kind: 'reveal', carrier: f('carrier'), pass: S.pass, output: S.out || null, parity: S.parity, rate: S.rate / 100 };
  if (m === 'analyze') return { kind: 'analyze', image: f('image') };
  if (m === 'bulk') return { kind: 'bulk', folder: S.dir, passes: passList(), out_dir: S.out || null, recursive: S.recursive, names_only: S.namesOnly };
  if (m === 'split') return S.task === 'join' ? { kind: 'join', carriers: S.files.joinCarriers.map((x) => x.path), pass: S.pass, output: S.out || null, parity: S.parity } : { kind: 'split', secret: f('secret'), k: Math.min(S.k, S.files.carriers.length), pass: S.pass, carriers: S.files.carriers.map((x) => x.path), out_dir: S.out || null, mode: S.mode, parity: S.parity };
  const sty = S.styles.find((x) => x.id === S.style) || {};
  const order = sty.adjustable_order ? S.order : null;
  if (m === 'textreveal') return { kind: 'text_reveal', text_file: f('carrier'), pass: S.pass, style: S.style, order, save: S.saveText || null };
  return { kind: 'text_hide', message: S.message, pass: S.pass, style: S.style, order, save: S.saveText || null };
}

function setBusy(b) {
  S.busy = b;
  const pr = $('#prog'), bar = pr.firstElementChild, st = $('#stage');
  if (b) { bar.style.transition = 'none'; bar.style.width = '6%'; pr.classList.add('on'); void bar.offsetWidth; bar.style.transition = 'width .5s ease'; st.classList.add('scan'); }
  else { bar.style.width = '100%'; st.classList.remove('scan'); setTimeout(() => pr.classList.remove('on'), 250); }
  updateDynamic();
}

async function run() {
  if (S.busy || problems().length) return;
  S.textOut = mode() === 'text' ? null : S.textOut;
  S.textRecovered = null;
  S.step = '';
  try {
    setBusy(true);
    await api.startJob(jobSpec());
  } catch (e) {
    setBusy(false);
    logLine(setErr(e), 'r');
  }
}

function wipePasswords() {
  S.pass = ''; S.pass2 = ''; S.passList = '';
  ['pass', 'pass2', 'passlist'].forEach((id) => { const e = $('#' + id); if (e) e.value = ''; });
}

function onJob(ev) {
  if (ev.event === 'step') {
    logLine(ev.message, 'd');
    const bar = $('#prog').firstElementChild;
    bar.style.width = Math.min(90, parseFloat(bar.style.width || '6') + 28) + '%';
    return;
  }
  if (ev.event === 'error') { setBusy(false); logLine(ev.message, 'r'); return; }
  const r = ev.result;
  if (r.kind === 'embedded' || r.kind === 'hidden') {
    S.real = { carrier: file('carrier').path, output: r.output };
    logLine(`Saved ${r.output}` + (r.kind === 'embedded' ? ` (${r.bytes} bytes hidden)` : '') + (r.report ? ` - detectability ${r.report.score.toFixed(0)}/100 ${r.report.risk}` : ''), 'g');
    wipePasswords(); setBusy(false); celebrate('Saved ' + baseName(r.output)); doViews(); openDir(S.dir);
    return;
  }
  if (r.kind === 'bulk') {
    S.bulk = r;
    const got = r.items.filter((i) => i.status === 'file').length + r.sets.filter((x) => x.output).length;
    logLine('Checked ' + r.checked + ' files: ' + got + ' recovered into ' + r.out_dir, 'g');
    wipePasswords(); setBusy(false); celebrate(got ? 'Recovered ' + got + (got === 1 ? ' item' : ' items') : 'Nothing found'); renderCenter(); renderRight(); openDir(S.dir);
    return;
  }
  if (r.kind === 'revealed') { logLine(`Recovered ${r.bytes} bytes (${r.layout === 'hidden' ? 'Hidden layout' : 'normal layout'}) into ${r.output}`, 'g'); wipePasswords(); setBusy(false); celebrate('Recovered ' + r.bytes + ' bytes'); openDir(S.dir); return; }
  if (r.kind === 'split') { logLine(`Wrote ${r.outputs.length} images: ${r.outputs.map(baseName).join(', ')}`, 'g'); wipePasswords(); setBusy(false); celebrate('Split into ' + r.outputs.length + ' images'); openDir(S.dir); return; }
  if (r.kind === 'joined') { logLine(`Recovered ${r.bytes} bytes from ${r.used} images into ${r.output}`, 'g'); wipePasswords(); setBusy(false); celebrate('Secret rebuilt'); openDir(S.dir); return; }
  if (r.kind === 'analyzed') {
    if (S.analyzing === 'b') { S.reportB = r.report; S.analyzing = null; logLine('Original scanned.', 'g'); setBusy(false); celebrate('Scan complete'); updateDynamic(); return; }
    S.report = r.report; S.reportB = null;
    logLine(`Scan finished: ${r.report.score.toFixed(0)}/100 ${r.report.risk}`, 'g');
    if (file('compare')) { S.analyzing = 'b'; api.startJob({ kind: 'analyze', image: file('compare').path }).catch((e) => { S.analyzing = null; setBusy(false); logLine(setErr(e), 'r'); }); updateDynamic(); return; }
    setBusy(false); celebrate('Scan complete'); updateDynamic();
    return;
  }
  if (r.kind === 'text_hidden') { S.textOut = r.text; logLine(`Cover text ready: ${r.words} words` + (r.saved ? `, saved to ${r.saved}` : ''), 'g'); wipePasswords(); setBusy(false); celebrate('Cover text ready'); renderCenter(); if (r.saved) openDir(S.dir); return; }
  if (r.kind === 'text_revealed') { S.textRecovered = r.text; logLine(`Recovered ${r.bytes} bytes` + (r.saved ? `, saved to ${r.saved}` : ''), 'g'); wipePasswords(); setBusy(false); celebrate('Message recovered'); updateDynamic(); if (r.saved) openDir(S.dir); return; }
}

// ---------- start-up ----------
function setOp(id) {
  S.op = id;
  S.out = ''; S.saveText = ''; // these mean different things for each task (file, folder, text file)
  S.active = slotDefs()[0] ? slotDefs()[0][0] : null;
  S.preview = null; S.previewError = null; S.pix = null; S.pixError = null;
  renderChips(); renderRight(); renderLeft(); renderCenter();
  schedulePreview(); scheduleEstimate();
}

document.addEventListener('click', (e) => {
  const chip = e.target.closest('[data-op]');
  if (chip) return setOp(chip.dataset.op);
});

document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === 'Enter') { e.preventDefault(); run(); }
});


function setupWindowControls() {
  let w = null;
  try { w = window.__TAURI__.window.getCurrentWindow(); } catch (e) { /* not running inside the desktop shell */ }
  const bind = (id, fn) => { const b = $('#' + id); if (b) b.onclick = () => { try { fn(); } catch (e) { /* ignore */ } }; };
  const maxIcon = '<svg viewBox="0 0 10 10" aria-hidden="true"><rect x="1.5" y="1.5" width="7" height="7" rx="1"/></svg>';
  const restoreIcon = '<svg viewBox="0 0 10 10" aria-hidden="true"><rect x="1" y="2.5" width="6.5" height="6.5" rx="1"/><path d="M3 2.5V1.8A.8.8 0 0 1 3.8 1H8.2A.8.8 0 0 1 9 1.8V6.2a.8.8 0 0 1-.8.8H7.5"/></svg>';
  const sync = async () => {
    if (!w) return;
    try { const m = await w.isMaximized(); const b = $('#wmax'); b.innerHTML = m ? restoreIcon : maxIcon; $('#app').classList.toggle('max', m); b.title = m ? 'Restore' : 'Maximize'; } catch (e) { /* ignore */ }
  };
  bind('wmin', () => w.minimize());
  bind('wmax', () => w.toggleMaximize());
  bind('wclose', () => w.close());
  if (w) { w.onResized(sync); sync(); }
}

async function init() {
  setupWindowControls();
  const d = await api.defaults();
  S.parity = d.parity; S.rate = Math.round(d.rate * 100); S.styles = d.styles;
  const sty = S.styles.find((s) => s.id === S.style);
  if (sty) S.order = sty.default_order;
  try { S.roots = await api.roots(); } catch (e) { S.roots = []; }
  await api.onJob(onJob);
  renderChips(); renderRight(); renderCenter();
  // The entrance animation is for the first paint only; later re-renders must not replay it.
  setTimeout(() => $('#app').classList.add('ready'), 1200);
  let start = null;
  try { start = localStorage.getItem('stegosaur.dir'); } catch (e) { start = null; }
  if (!(start && (await openDir(start)))) { if (!(await openDir(d.home || d.cwd))) await openDir(d.cwd); }
}

window.StegoUI = { S, setOp, openDir, pick, run, problems, doPreview, doViews, onJob };
init().catch((e) => { document.body.insertAdjacentHTML('afterbegin', `<pre style="color:#b00;padding:12px">Start-up failed: ${esc(setErr(e))}</pre>`); });
