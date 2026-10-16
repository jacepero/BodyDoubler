'use strict';
/* Greenhouse weight diff — single-page UI. No build step, no dependencies. */
(() => {
  // ------------------------------------------------------------------ utilities
  const $ = (id) => document.getElementById(id);
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  const SVG_NS = 'http://www.w3.org/2000/svg';
  const svgEl = (tag, attrs = {}, text) => {
    const e = document.createElementNS(SVG_NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    if (text != null) e.textContent = text;
    return e;
  };
  const cssVar = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const isDark = () => getComputedStyle(document.documentElement).colorScheme === 'dark';
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const finite = (v) => typeof v === 'number' && Number.isFinite(v);
  const FONT = '11px system-ui, -apple-system, "Segoe UI", sans-serif';

  async function api(path, params) {
    const qs = new URLSearchParams(params || {}).toString();
    const res = await fetch(path + (qs ? '?' + qs : ''));
    const body = await res.json().catch(() => ({ error: res.statusText }));
    if (!res.ok) throw new Error(body.error || res.statusText);
    return body;
  }

  function quantile(sorted, q) {
    if (!sorted.length) return NaN;
    const i = clamp((sorted.length - 1) * q, 0, sorted.length - 1);
    const lo = Math.floor(i), hi = Math.ceil(i);
    return sorted[lo] + (sorted[hi] - sorted[lo]) * (i - lo);
  }

  function decodeF32(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return new Float32Array(bytes.buffer);
  }

  // ------------------------------------------------------------------ formatting
  function fmtPct(v) {
    if (!finite(v)) return '–';
    const p = v * 100;
    if (p === 0) return '0%';
    const a = Math.abs(p);
    if (a >= 10) return p.toFixed(1) + '%';
    if (a >= 1) return p.toFixed(2) + '%';
    return p.toPrecision(2) + '%';
  }
  function fmtNum(v) {
    if (!finite(v)) return '–';
    if (v === 0) return '0';
    const a = Math.abs(v);
    if (a >= 0.01 && a < 10000) return String(+v.toPrecision(3));
    return v.toExponential(2).replace(/\.?0+e/, 'e'); // 5.00e-3 -> 5e-3
  }
  function fmtCount(n) {
    if (!finite(n)) return '–';
    const a = Math.abs(n);
    if (a >= 1e12) return (n / 1e12).toFixed(2) + 'T';
    if (a >= 1e9) return (n / 1e9).toFixed(2) + 'B';
    if (a >= 1e6) return (n / 1e6).toFixed(1) + 'M';
    if (a >= 1e4) return (n / 1e3).toFixed(1) + 'K';
    return String(Math.round(n));
  }
  function fmtBytes(n) {
    if (n >= 2 ** 30) return (n / 2 ** 30).toFixed(1) + ' GB';
    if (n >= 2 ** 20) return (n / 2 ** 20).toFixed(1) + ' MB';
    return (n / 1024).toFixed(0) + ' KB';
  }
  const ISO_DAY = /^(\d{4})-(\d{2})-(\d{2})$/;
  function shortDay(name) {
    const m = ISO_DAY.exec(name);
    if (!m) return name;
    return new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
  const shape = (s) => (s && s.length ? s.join('×') : 'scalar');

  // ------------------------------------------------------------------ metrics
  function aggregate(list) {
    const s = { sa: 0, sb: 0, sd: 0, n: 0, ch: 0, maxabs: 0, count: 0, nonfinite: 0 };
    for (const t of list) {
      if (t.status !== 'ok') continue;
      s.sa += t.sa ?? NaN;
      s.sb += t.sb ?? NaN;
      s.sd += t.sd ?? NaN;
      s.n += t.numel;
      s.ch += t.ch;
      s.nonfinite += t.nonfinite || 0;
      if (finite(t.maxabs)) s.maxabs = Math.max(s.maxabs, t.maxabs);
      s.count++;
    }
    return s;
  }
  function cosDist(s) {
    if (!(s.sa > 0 && s.sb > 0)) return null;
    const na = Math.sqrt(s.sa), nb = Math.sqrt(s.sb);
    return Math.max(0, s.sd - (nb - na) ** 2) / (2 * na * nb);
  }
  const METRICS = {
    rel: { label: 'Relative change', long: 'Relative change ‖ΔW‖ / ‖W‖', fmt: fmtPct, f: (s) => (s.sa > 0 ? Math.sqrt(s.sd / s.sa) : null) },
    rms: { label: 'RMS update', long: 'RMS update per weight', fmt: fmtNum, f: (s) => (s.n ? Math.sqrt(s.sd / s.n) : null) },
    norm: { label: 'Update norm', long: 'Update norm ‖ΔW‖', fmt: fmtNum, f: (s) => Math.sqrt(s.sd) },
    cosdist: { label: '1 − cos', long: 'Direction change (1 − cosine)', fmt: fmtNum, f: cosDist },
    changed: { label: 'Weights changed', long: 'Share of weights that changed', fmt: fmtPct, f: (s) => (s.n ? s.ch / s.n : null) },
  };
  const metricOf = (s) => (s && s.count ? METRICS[state.metric].f(s) : null);
  const GROUP_LABEL = { all: 'all modules', attn: 'attention', mlp: 'MLP', norm: 'norms', embed: 'embeddings & head' };
  const inGroup = (t) => state.group === 'all' || t.group === state.group;

  // ------------------------------------------------------------------ color
  // Sequential: one hue (blue 100→700). Dark mode flips the anchor so "near zero" recedes into the surface.
  const BLUE = ['#cde2fb', '#b7d3f6', '#9ec5f4', '#86b6ef', '#6da7ec', '#5598e7', '#3987e5', '#2a78d6', '#256abf', '#1c5cab', '#184f95', '#104281', '#0d366b'];
  // Diverging: blue ↔ red around a neutral gray midpoint, equal arms.
  const DIV_LIGHT = ['#184f95', '#3987e5', '#9ec5f4', '#f0efec', '#f2a9a6', '#e34948', '#9e2b2b'];
  const DIV_DARK = ['#9ec5f4', '#5598e7', '#1c5cab', '#383835', '#9a3534', '#e66767', '#f4b0ae'];
  const hexRgb = (h) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
  function makeLut(stops, n = 256) {
    const rgb = stops.map(hexRgb);
    const lut = new Uint8ClampedArray(n * 3);
    for (let i = 0; i < n; i++) {
      const t = (i / (n - 1)) * (rgb.length - 1);
      const j = Math.min(Math.floor(t), rgb.length - 2), f = t - j;
      for (let c = 0; c < 3; c++) lut[i * 3 + c] = Math.round(rgb[j][c] + (rgb[j + 1][c] - rgb[j][c]) * f);
    }
    return lut;
  }

  /** values: finite numbers. kind 'seq' (>= 0) or 'div' (signed). */
  function makeScale(values, { kind = 'seq', log = false, hiQ = 1 } = {}) {
    const stops = kind === 'div' ? (isDark() ? DIV_DARK : DIV_LIGHT) : isDark() ? BLUE.slice().reverse() : BLUE;
    const lut = makeLut(stops);
    let t, ticks;
    if (kind === 'div') {
      const abs = values.map(Math.abs).sort((a, b) => a - b);
      const m = quantile(abs, 0.99) || abs[abs.length - 1] || 1;
      t = (v) => 0.5 + 0.5 * clamp(v / m, -1, 1);
      ticks = [-m, 0, m];
    } else {
      const pos = values.filter((v) => v > 0).sort((a, b) => a - b);
      const hi = pos.length ? quantile(pos, hiQ) : 1;
      if (log && pos.length) {
        let lo = quantile(pos, 0.02);
        if (!(lo < hi)) lo = hi / 10;
        const L0 = Math.log(lo), L1 = Math.log(hi);
        t = (v) => clamp((Math.log(v) - L0) / (L1 - L0), 0, 1);
        ticks = [lo, Math.sqrt(lo * hi), hi];
      } else {
        t = (v) => clamp(v / hi, 0, 1);
        ticks = [0, hi / 2, hi];
      }
    }
    const rgb = (v) => { const i = Math.round(t(v) * 255) * 3; return [lut[i], lut[i + 1], lut[i + 2]]; };
    return { kind, log, t, rgb, color: (v) => `rgb(${rgb(v).join(',')})`, ticks, stops };
  }

  function renderLegend(host, scale, fmt, { noChange = false, pending = false } = {}) {
    const lg = el('div', 'legend');
    const ramp = el('div', 'ramp');
    const bar = el('div', 'bar');
    bar.style.background = `linear-gradient(90deg, ${scale.stops.join(',')})`;
    ramp.append(bar);
    scale.ticks.forEach((v) => {
      const s = el('span', null, fmt(v));
      s.style.left = (scale.t(v) * 100).toFixed(1) + '%';
      ramp.append(s);
    });
    lg.append(ramp);
    if (scale.log) lg.append(el('span', null, 'log scale'));
    if (noChange) {
      const s = el('span');
      const sw = el('i', 'sw'); sw.style.background = cssVar('--nochange');
      s.append(sw, document.createTextNode('no change'));
      lg.append(s);
    }
    if (pending) {
      const s = el('span');
      const sw = el('i', 'sw'); sw.style.background = cssVar('--pending'); sw.style.outline = '1px solid ' + cssVar('--grid');
      s.append(sw, document.createTextNode('computing'));
      lg.append(s);
    }
    host.append(lg);
  }

  // ------------------------------------------------------------------ tooltip
  const tip = $('tooltip');
  function showTip(evt, title, rows) {
    tip.textContent = '';
    if (title) tip.append(el('div', 't', title));
    // rows: [value, label, keyColor?] — the value leads, the label follows.
    for (const r of rows || []) {
      const row = el('div', 'r');
      const label = el('span');
      if (r[2]) { const k = el('i', 'key'); k.style.background = r[2]; label.append(k); }
      label.append(document.createTextNode(r[1]));
      row.append(el('b', null, r[0]), label);
      tip.append(row);
    }
    tip.classList.add('show');
    const pad = 14, w = tip.offsetWidth, h = tip.offsetHeight;
    let x = evt.clientX + pad, y = evt.clientY + pad;
    if (x + w > window.innerWidth - 8) x = evt.clientX - w - pad;
    if (y + h > window.innerHeight - 8) y = evt.clientY - h - pad;
    tip.style.left = Math.max(8, x) + 'px';
    tip.style.top = Math.max(8, y) + 'px';
  }
  const hideTip = () => tip.classList.remove('show');

  // ------------------------------------------------------------------ canvas heatmap
  /**
   * spec: rows [{key,label}], cols [{key,label}], cell(ri,ci) -> {v} | {pending,progress} | null,
   *       scale, tip(ri,ci,cell) -> {title, rows}, onClick(ri,ci,cell), highlightRow, highlightCol
   */
  function renderHeatmap(host, spec) {
    host.textContent = '';
    const { rows, cols } = spec;
    if (!rows.length || !cols.length) {
      host.append(el('div', 'empty', spec.emptyText || 'Nothing to show yet.'));
      return;
    }
    const scroll = el('div', 'heat-scroll');
    const canvas = el('canvas');
    canvas.setAttribute('role', 'img');
    canvas.setAttribute('aria-label', spec.ariaLabel || 'Heatmap (use the Table button for an accessible view)');
    scroll.append(canvas);
    host.append(scroll);
    const ctx = canvas.getContext('2d');
    ctx.font = FONT;
    const nR = rows.length, nC = cols.length;
    const labelW = Math.ceil(Math.max(24, ...rows.map((r) => ctx.measureText(r.label).width))) + 12;
    const cellH = nR <= 14 ? 22 : nR <= 28 ? 16 : nR <= 56 ? 11 : nR <= 110 ? 7 : 5;
    const colTextW = Math.ceil(Math.max(...cols.map((c) => ctx.measureText(c.label).width)));
    const avail = Math.max(240, host.clientWidth) - labelW - 4;
    let cellW = clamp(Math.floor(avail / nC), spec.minCellW || 12, spec.maxCellW || 80);
    const rotate = colTextW + 10 > cellW;
    const diag = Math.ceil(colTextW * 0.71);
    if (rotate) cellW = clamp(Math.floor((avail - diag) / nC), spec.minCellW || 12, spec.maxCellW || 80);
    const headH = rotate ? diag + 16 : 20;
    const W = labelW + nC * cellW + (rotate ? diag : 4);
    const H = headH + nR * cellH + 2;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    canvas.style.width = W + 'px';
    canvas.style.height = H + 'px';
    const gap = cellW >= 10 && cellH >= 10 ? 2 : 1;
    const hl = rows.findIndex((r) => r.key === spec.highlightRow);
    const hc = cols.findIndex((c) => c.key === spec.highlightCol);
    let hover = null;

    function draw() {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);
      ctx.font = FONT;
      const nochange = cssVar('--nochange'), pending = cssVar('--pending');
      const text1 = cssVar('--text-1'), muted = cssVar('--muted');
      ctx.textBaseline = 'middle';
      ctx.textAlign = 'right';
      const every = Math.max(1, Math.ceil(12 / cellH));
      rows.forEach((r, ri) => {
        if (ri % every && ri !== hl && ri !== nR - 1) return;
        ctx.fillStyle = ri === hl ? text1 : muted;
        ctx.fillText(r.label, labelW - 6, headH + ri * cellH + cellH / 2);
      });
      const colEvery = rotate ? Math.max(1, Math.ceil(13 / cellW)) : 1;
      cols.forEach((c, ci) => {
        if (ci % colEvery && ci !== hc && ci !== nC - 1) return;
        const x = labelW + ci * cellW + cellW / 2;
        ctx.fillStyle = ci === hc ? text1 : muted;
        if (rotate) {
          ctx.save();
          ctx.translate(x, headH - 6);
          ctx.rotate(-Math.PI / 4);
          ctx.textAlign = 'left';
          ctx.fillText(c.label, 0, 0);
          ctx.restore();
        } else {
          ctx.textAlign = 'center';
          ctx.fillText(c.label, x, headH / 2);
        }
      });
      for (let ri = 0; ri < nR; ri++) {
        for (let ci = 0; ci < nC; ci++) {
          const cell = spec.cell(ri, ci);
          if (!cell) continue;
          let fill;
          if (cell.pending) fill = pending;
          else if (!finite(cell.v)) fill = cssVar('--critical');
          else if (cell.v === 0) fill = nochange;
          else fill = spec.scale.color(cell.v);
          ctx.fillStyle = fill;
          ctx.fillRect(labelW + ci * cellW + gap / 2, headH + ri * cellH + gap / 2, cellW - gap, cellH - gap);
        }
      }
      ctx.strokeStyle = text1;
      ctx.lineWidth = 1.5;
      if (hl >= 0) ctx.strokeRect(labelW + 0.75, headH + hl * cellH + 0.75, nC * cellW - 1.5, cellH - 1.5);
      if (hc >= 0) ctx.strokeRect(labelW + hc * cellW + 0.75, headH + 0.75, cellW - 1.5, nR * cellH - 1.5);
      if (hover) {
        ctx.lineWidth = 2;
        ctx.strokeRect(labelW + hover.ci * cellW + 1, headH + hover.ri * cellH + 1, cellW - 2, cellH - 2);
      }
    }
    draw();

    function hit(e) {
      const r = canvas.getBoundingClientRect();
      const ci = Math.floor((e.clientX - r.left - labelW) / cellW);
      const ri = Math.floor((e.clientY - r.top - headH) / cellH);
      if (ci < 0 || ri < 0 || ci >= nC || ri >= nR) return null;
      const cell = spec.cell(ri, ci);
      return cell ? { ri, ci, cell } : null;
    }
    canvas.addEventListener('pointermove', (e) => {
      const h = hit(e);
      if (!h) {
        if (hover) { hover = null; draw(); }
        canvas.style.cursor = 'default';
        hideTip();
        return;
      }
      if (!hover || hover.ri !== h.ri || hover.ci !== h.ci) { hover = h; draw(); }
      const t = spec.tip(h.ri, h.ci, h.cell);
      showTip(e, t.title, t.rows);
      canvas.style.cursor = spec.onClick && !h.cell.pending ? 'pointer' : 'default';
    });
    canvas.addEventListener('pointerleave', () => { hover = null; draw(); hideTip(); });
    canvas.addEventListener('click', (e) => {
      const h = hit(e);
      if (h && spec.onClick && !h.cell.pending) { hideTip(); spec.onClick(h.ri, h.ci, h.cell); }
    });
  }

  /** Accessible twin of a heatmap. */
  function renderMatrixTable(host, rows, cols, cell, fmt) {
    host.textContent = '';
    const wrap = el('div', 'table-wrap');
    const table = el('table', 'matrix-table');
    const thead = el('thead'), hr = el('tr');
    hr.append(el('th', null, ''));
    cols.forEach((c) => hr.append(el('th', null, c.label)));
    thead.append(hr);
    const tbody = el('tbody');
    rows.forEach((r, ri) => {
      const tr = el('tr');
      tr.append(el('td', null, r.label));
      cols.forEach((c, ci) => {
        const x = cell(ri, ci);
        tr.append(el('td', null, !x ? '' : x.pending ? '…' : fmt(x.v)));
      });
      tbody.append(tr);
    });
    table.append(thead, tbody);
    wrap.append(table);
    host.append(wrap);
  }

  // ------------------------------------------------------------------ SVG charts
  function niceTicks(lo, hi, count = 4) {
    if (!(hi > lo)) hi = lo + (Math.abs(lo) || 1);
    const raw = (hi - lo) / count;
    const mag = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) || 10 * mag;
    const start = Math.floor(lo / step + 1e-9) * step;
    const ticks = [];
    for (let v = start; v <= hi + step * 0.5 && ticks.length < 12; v += step) ticks.push(+v.toPrecision(12));
    if (ticks[ticks.length - 1] < hi) ticks.push(+(ticks[ticks.length - 1] + step).toPrecision(12));
    return ticks;
  }

  /** Left margin wide enough for the longest y tick label. */
  function leftMargin(labels) {
    const ctx = leftMargin.ctx || (leftMargin.ctx = document.createElement('canvas').getContext('2d'));
    ctx.font = FONT;
    return Math.ceil(Math.max(24, ...labels.map((l) => ctx.measureText(l).width))) + 14;
  }

  function topRoundedBar(x, y, w, h, r) {
    r = Math.min(r, w / 2, h);
    if (h <= 0 || w <= 0) return '';
    return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
  }

  /**
   * Single-series line over categorical x positions.
   * spec: xLabels, ys (number|null), fmt, color, height, tip(i) -> {title, rows}, notes {i: text}, band [[lo,hi]], yZero
   */
  function renderLine(host, spec) {
    host.textContent = '';
    const n = spec.xLabels.length;
    const vals = spec.ys.filter(finite);
    if (spec.band) spec.band.forEach((b) => b && vals.push(...b.filter(finite)));
    if (!n || !vals.length) {
      host.append(el('div', 'empty', spec.emptyText || 'Waiting for data…'));
      return;
    }
    const W = Math.max(260, host.clientWidth), H = spec.height || 170;
    let lo = spec.yZero === false ? Math.min(...vals) : Math.min(0, ...vals);
    let hi = Math.max(...vals);
    if (hi === lo) { hi = lo + (Math.abs(lo) || 1) * 0.1; lo = spec.yZero === false ? lo - (Math.abs(lo) || 1) * 0.1 : lo; }
    const ticks = niceTicks(lo, hi, 4);
    lo = Math.min(lo, ticks[0]);
    hi = ticks[ticks.length - 1];
    const m = { l: leftMargin(ticks.map(spec.fmt)), r: spec.endLabel === false ? 16 : 62, t: 16, b: 26 };
    const pw = W - m.l - m.r, ph = H - m.t - m.b;
    const x = (i) => m.l + (n === 1 ? pw / 2 : (i * pw) / (n - 1));
    const y = (v) => m.t + (1 - (v - lo) / (hi - lo)) * ph;
    const svg = svgEl('svg', { viewBox: `0 0 ${W} ${H}`, height: H, role: 'img', 'aria-label': spec.ariaLabel || '' });
    ticks.forEach((tv) => {
      if (tv < lo - 1e-12) return;
      svg.append(svgEl('line', { class: tv === lo ? 'baseline' : 'gridline', x1: m.l, x2: W - m.r, y1: Math.round(y(tv)) + 0.5, y2: Math.round(y(tv)) + 0.5 }));
      svg.append(svgEl('text', { x: m.l - 8, y: y(tv) + 4, 'text-anchor': 'end' }, spec.fmt(tv)));
    });
    const every = Math.max(1, Math.ceil(n / Math.max(1, Math.floor(pw / 62))));
    spec.xLabels.forEach((lab, i) => {
      if (i % every && i !== n - 1) return;
      if (i !== n - 1 && x(n - 1) - x(i) < 52) return; // keep the last label clear
      svg.append(svgEl('text', { x: x(i), y: H - 8, 'text-anchor': 'middle' }, lab));
    });
    const color = spec.color || cssVar('--series-1');
    if (spec.band) {
      let d = '', back = '';
      spec.band.forEach((b, i) => {
        if (!b || !finite(b[0])) return;
        d += (d ? 'L' : 'M') + x(i) + ',' + y(b[1]);
        back = 'L' + x(i) + ',' + y(b[0]) + back;
      });
      if (d) svg.append(svgEl('path', { d: d + back + 'Z', fill: cssVar('--band-1'), stroke: 'none' }));
    }
    let d = '', pen = false;
    spec.ys.forEach((v, i) => {
      if (!finite(v)) { pen = false; return; }
      d += (pen ? 'L' : 'M') + x(i).toFixed(1) + ',' + y(v).toFixed(1);
      pen = true;
    });
    svg.append(svgEl('path', { d, fill: 'none', stroke: color, 'stroke-width': 2, 'stroke-linejoin': 'round', 'stroke-linecap': 'round' }));
    const surface = cssVar('--surface');
    if (n <= 40 && !spec.band) {
      spec.ys.forEach((v, i) => {
        if (finite(v)) svg.append(svgEl('circle', { cx: x(i), cy: y(v), r: 4, fill: color, stroke: surface, 'stroke-width': 2 }));
      });
    }
    let last = -1;
    spec.ys.forEach((v, i) => { if (finite(v)) last = i; });
    if (last >= 0 && spec.endLabel !== false) {
      svg.append(svgEl('text', { class: 'endlabel', x: x(last) + 9, y: y(spec.ys[last]) + 4 }, spec.fmt(spec.ys[last])));
    }
    if (spec.notes) {
      for (const i in spec.notes) {
        const cx = x(+i);
        const mark = svgEl('path', { class: 'note-mark', d: `M${cx - 4},${m.t - 12}h8l-4,6z` });
        mark.append(svgEl('title', {}, spec.notes[i]));
        svg.append(mark);
      }
    }
    // Crosshair: snaps to the nearest x; the whole plot is the hit target.
    const cross = svgEl('line', { class: 'crosshair', y1: m.t, y2: m.t + ph, visibility: 'hidden' });
    const dot = svgEl('circle', { r: 5, fill: color, stroke: surface, 'stroke-width': 2, visibility: 'hidden', 'pointer-events': 'none' });
    const hitRect = svgEl('rect', { x: m.l - 10, y: 0, width: pw + 20, height: H, fill: 'transparent' });
    svg.append(cross, dot, hitRect);
    hitRect.addEventListener('pointermove', (e) => {
      const r = svg.getBoundingClientRect();
      const px = ((e.clientX - r.left) / r.width) * W;
      const i = n === 1 ? 0 : clamp(Math.round(((px - m.l) / pw) * (n - 1)), 0, n - 1);
      cross.setAttribute('x1', x(i)); cross.setAttribute('x2', x(i)); cross.setAttribute('visibility', 'visible');
      if (finite(spec.ys[i])) {
        dot.setAttribute('cx', x(i)); dot.setAttribute('cy', y(spec.ys[i])); dot.setAttribute('visibility', 'visible');
      } else dot.setAttribute('visibility', 'hidden');
      const t = spec.tip(i);
      showTip(e, t.title, t.rows);
    });
    hitRect.addEventListener('pointerleave', () => {
      cross.setAttribute('visibility', 'hidden'); dot.setAttribute('visibility', 'hidden'); hideTip();
    });
    if (spec.onClick) {
      hitRect.style.cursor = 'pointer';
      hitRect.addEventListener('click', (e) => {
        const r = svg.getBoundingClientRect();
        const px = ((e.clientX - r.left) / r.width) * W;
        spec.onClick(n === 1 ? 0 : clamp(Math.round(((px - m.l) / pw) * (n - 1)), 0, n - 1));
      });
    }
    host.append(svg);
  }

  /**
   * Vertical bars. spec: bars [{v}], xAt(i)->[x0,x1] in [0,1], xTicks [{at, label}], yLog, fmt, tip(i), height
   */
  function renderBars(host, spec) {
    host.textContent = '';
    const bars = spec.bars;
    const vals = bars.map((b) => b.v).filter((v) => finite(v) && (!spec.yLog || v > 0));
    if (!vals.length) {
      host.append(el('div', 'empty', spec.emptyText || 'No data.'));
      return;
    }
    const W = Math.max(240, host.clientWidth), H = spec.height || 160;
    const m = { l: 52, r: 12, t: 10, b: 24 };
    let y, ticks;
    if (spec.yLog) {
      const hi = Math.max(...vals);
      const top = 10 ** Math.ceil(Math.log10(hi));
      const decades = Math.max(1, Math.ceil(Math.log10(top)));
      y = (v) => m.t + (1 - Math.log10(Math.max(v, 1)) / decades) * ph;
      ticks = [];
      const stepD = Math.max(1, Math.ceil(decades / 4));
      for (let k = 0; k <= decades; k += stepD) ticks.push(10 ** k);
    } else {
      ticks = niceTicks(0, Math.max(...vals), 4);
      const hi = ticks[ticks.length - 1];
      y = (v) => m.t + (1 - v / hi) * ph;
    }
    m.l = leftMargin(ticks.map(spec.fmt));
    const pw = W - m.l - m.r, ph = H - m.t - m.b;
    const svg = svgEl('svg', { viewBox: `0 0 ${W} ${H}`, height: H, role: 'img', 'aria-label': spec.ariaLabel || '' });
    ticks.forEach((tv, k) => {
      svg.append(svgEl('line', { class: k === 0 ? 'baseline' : 'gridline', x1: m.l, x2: W - m.r, y1: Math.round(y(tv)) + 0.5, y2: Math.round(y(tv)) + 0.5 }));
      svg.append(svgEl('text', { x: m.l - 8, y: y(tv) + 4, 'text-anchor': 'end' }, spec.fmt(tv)));
    });
    (spec.xTicks || []).forEach((t) => svg.append(svgEl('text', { x: m.l + t.at * pw, y: H - 6, 'text-anchor': t.anchor || 'middle' }, t.label)));
    const color = spec.color || cssVar('--series-1');
    const base = y(spec.yLog ? 1 : 0);
    const rects = [];
    bars.forEach((b, i) => {
      const [f0, f1] = spec.xAt(i);
      const bw = (f1 - f0) * pw;
      const gap = bw > 8 ? 2 : bw > 3 ? 1 : 0;
      const x0 = m.l + f0 * pw + gap / 2, w = Math.max(0.5, bw - gap);
      if (!finite(b.v) || b.v <= 0 || (spec.yLog && b.v < 1)) { rects.push(null); return; }
      const top = y(b.v);
      const p = svgEl('path', { d: topRoundedBar(x0, top, w, base - top, 4), fill: color });
      svg.append(p);
      rects.push(p);
    });
    // One hit band per bar, full plot height (bigger than the mark).
    const hitRect = svgEl('rect', { x: m.l, y: 0, width: pw, height: H, fill: 'transparent' });
    svg.append(hitRect);
    let lastI = -1;
    hitRect.addEventListener('pointermove', (e) => {
      const r = svg.getBoundingClientRect();
      const f = (((e.clientX - r.left) / r.width) * W - m.l) / pw;
      let i = bars.findIndex((_, k) => { const [a, b] = spec.xAt(k); return f >= a && f < b; });
      if (i < 0) i = f < 0.5 ? 0 : bars.length - 1;
      if (lastI !== i) {
        if (rects[lastI]) rects[lastI].setAttribute('opacity', 1);
        if (rects[i]) rects[i].setAttribute('opacity', 0.7);
        lastI = i;
      }
      const t = spec.tip(i);
      showTip(e, t.title, t.rows);
    });
    hitRect.addEventListener('pointerleave', () => {
      if (rects[lastI]) rects[lastI].setAttribute('opacity', 1);
      lastI = -1;
      hideTip();
    });
    host.append(svg);
  }

  // ------------------------------------------------------------------ state
  const state = {
    root: '',
    runs: [],
    run: null,
    metric: 'rel',
    group: 'all',
    scale: 'log',
    mode: 'consecutive',
    timeline: null,
    pairs: new Map(), // key -> parsed pair
    dayA: null,
    dayB: null,
    dayKey: null,
    dayStatus: null,
    highlightRow: null,
    tensor: null,
    tensorDetail: null,
    tensorSpectrum: null,
    tensorError: null,
    tmapMode: 'rms',
    histScale: 'linear',
    tlTable: false,
    dayTable: false,
    sort: null, // {key, dir}
    search: '',
    seq: 0,
  };
  const timers = {};

  const readyDays = () => (state.run ? state.run.days.filter((d) => !d.error) : []);
  const dayIndex = (name) => readyDays().findIndex((d) => d.name === name);

  function parsePair(res) {
    const t = res.tensors, out = [];
    for (let i = 0; i < t.name.length; i++) {
      out.push({
        name: t.name[i], shape: t.shape[i], dtype: t.dtype[i], status: t.status[i],
        row: t.row[i], rowLabel: t.row_label[i], rowOrder: t.row_order[i],
        module: t.module[i], moduleLabel: t.module_label[i], group: t.group[i], colOrder: t.col_order[i],
        numel: t.numel[i] ?? t.shape[i].reduce((a, b) => a * b, 1),
        sa: t.sq_a[i], sb: t.sq_b[i], sd: t.sq_d[i], sumd: t.sum_d[i], maxabs: t.maxabs[i], ch: t.changed[i], nonfinite: t.nonfinite[i],
      });
    }
    return { a: res.a, b: res.b, key: res.key, tensors: out, elapsed: res.elapsed };
  }

  function rowsOf(tensors) {
    const map = new Map();
    for (const t of tensors) if (inGroup(t) && !map.has(t.row)) map.set(t.row, { key: t.row, label: t.rowLabel, order: t.rowOrder });
    return [...map.values()].sort((a, b) => a.order - b.order);
  }

  // ------------------------------------------------------------------ URL hash
  function writeHash() {
    const p = new URLSearchParams();
    if (state.run) p.set('run', state.run.name);
    if (state.metric !== 'rel') p.set('metric', state.metric);
    if (state.group !== 'all') p.set('group', state.group);
    if (state.scale !== 'log') p.set('scale', state.scale);
    if (state.mode !== 'consecutive') p.set('mode', state.mode);
    if (state.dayA) p.set('a', state.dayA);
    if (state.dayB) p.set('b', state.dayB);
    if (state.tensor) p.set('t', state.tensor);
    history.replaceState(null, '', '#' + p.toString());
  }
  const readHash = () => Object.fromEntries(new URLSearchParams(location.hash.slice(1)));

  // ------------------------------------------------------------------ controls
  function setSeg(id, value) {
    $(id).querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === value)));
  }
  function wireSeg(id, onChange) {
    $(id).addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b) return;
      setSeg(id, b.dataset.v);
      onChange(b.dataset.v);
    });
  }

  function wire() {
    $('run').addEventListener('change', () => setRun($('run').value, {}));
    $('rescan').addEventListener('click', () => loadRuns(readHash()));
    $('metric').addEventListener('change', () => { state.metric = $('metric').value; state.sort = null; writeHash(); renderAll(); });
    $('group').addEventListener('change', () => { state.group = $('group').value; writeHash(); renderAll(); });
    wireSeg('scale', (v) => { state.scale = v; writeHash(); renderAll(); });
    wireSeg('mode', (v) => { state.mode = v; state.timeline = null; writeHash(); renderTimeline(); pollTimeline(); });
    wireSeg('tmap-mode', (v) => { state.tmapMode = v; renderTensorMap(); });
    wireSeg('hist-scale', (v) => { state.histScale = v; renderHistogram(); });
    $('tl-table-toggle').addEventListener('click', () => {
      state.tlTable = !state.tlTable;
      $('tl-table-toggle').setAttribute('aria-pressed', String(state.tlTable));
      renderTimeline();
    });
    $('day-table-toggle').addEventListener('click', () => {
      state.dayTable = !state.dayTable;
      $('day-table-toggle').setAttribute('aria-pressed', String(state.dayTable));
      renderDay();
    });
    $('day-a').addEventListener('change', () => selectPair($('day-a').value, state.dayB));
    $('day-b').addEventListener('change', () => selectPair(state.dayA, $('day-b').value));
    $('day-prev').addEventListener('click', () => shiftPair(-1));
    $('day-next').addEventListener('click', () => shiftPair(1));
    $('tensor-search').addEventListener('input', () => { state.search = $('tensor-search').value.trim().toLowerCase(); renderTensorTable(); });
    $('tensor-close').addEventListener('click', () => { state.tensor = null; writeHash(); renderTensor(); });
    $('meta-field').addEventListener('change', () => renderMeta());
    $('theme').addEventListener('click', () => {
      const next = isDark() ? 'light' : 'dark';
      document.documentElement.dataset.theme = next;
      try { localStorage.setItem('weightviz-theme', next); } catch (_) { /* storage unavailable */ }
      themeLabel();
      renderAll();
    });
    matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => { themeLabel(); renderAll(); });
    let rt;
    window.addEventListener('resize', () => { clearTimeout(rt); rt = setTimeout(renderAll, 150); });
    document.addEventListener('scroll', hideTip, { passive: true });
  }

  const themeLabel = () => { $('theme').textContent = isDark() ? '☀ Light' : '☾ Dark'; };

  // ------------------------------------------------------------------ runs
  async function loadRuns(h) {
    let res;
    try {
      res = await api('/api/runs');
    } catch (e) {
      showWelcome(`Could not reach the weightviz server: ${e.message}`);
      return;
    }
    state.root = res.root;
    state.runs = res.runs;
    const sel = $('run');
    sel.textContent = '';
    res.runs.forEach((r) => sel.append(new Option(r.name, r.name)));
    if (!res.runs.length) {
      showWelcome();
      return;
    }
    $('welcome').hidden = true;
    $('views').hidden = false;
    const name = res.runs.some((r) => r.name === h.run) ? h.run : state.run && res.runs.some((r) => r.name === state.run.name) ? state.run.name : res.runs[0].name;
    sel.value = name;
    setRun(name, h);
  }

  function showWelcome(message) {
    const app = $('welcome');
    app.textContent = '';
    app.hidden = false;
    $('views').hidden = true;
    const w = el('div', 'welcome card');
    w.append(el('h2', null, message ? 'Something went wrong' : 'No checkpoints found yet'));
    if (message) w.append(el('p', null, message));
    else {
      w.append(el('p', null, `weightviz is watching ${state.root}. Put one folder per day of training under a run folder:`));
      w.append(el('pre', null, `${state.root}/\n  my-run/\n    2026-10-15/model.safetensors   (+ optional meta.json)\n    2026-10-16/model.safetensors`));
      w.append(el('p', null, 'Then press Rescan. To try it out with synthetic data:'));
      w.append(el('pre', null, `python -m weightviz demo ${state.root}`));
    }
    app.append(w);
  }

  function setRun(name, h) {
    state.run = state.runs.find((r) => r.name === name);
    state.timeline = null;
    state.dayKey = null; // day names can repeat across runs
    state.dayStatus = null;
    state.tensorDetail = null;
    state.tensorSpectrum = null;
    const days = readyDays();
    const names = days.map((d) => d.name);
    const fill = (sel) => {
      sel.textContent = '';
      days.forEach((d) => sel.append(new Option(shortDay(d.name) === d.name ? d.name : `${shortDay(d.name)} · ${d.name}`, d.name)));
    };
    fill($('day-a'));
    fill($('day-b'));
    if (names.includes(h.a) && names.includes(h.b) && h.a !== h.b) {
      state.dayA = h.a;
      state.dayB = h.b;
    } else {
      state.dayA = names[names.length - 2] || null;
      state.dayB = names[names.length - 1] || null;
    }
    $('day-a').value = state.dayA || '';
    $('day-b').value = state.dayB || '';
    state.tensor = h.t || null;
    state.highlightRow = null;
    const bad = state.run.days.filter((d) => d.error);
    const n = state.run.days.length;
    $('tl-sub').textContent = `${n} checkpoint${n === 1 ? '' : 's'}${bad.length ? ` · ${bad.length} unreadable` : ''}`;
    writeHash();
    renderTimeline();
    if (days.length < 2) {
      $('day-sub').textContent = 'Need at least two readable days to diff.';
      renderDay();
    } else {
      loadDayPair();
    }
    pollTimeline();
    renderTensor();
    if (state.tensor) loadTensor();
  }

  // Pick up new days as they land (e.g. tonight's checkpoint) without a manual rescan.
  async function autoRescan() {
    try {
      const res = await api('/api/runs');
      const sig = (runs) => JSON.stringify(runs.map((r) => [r.name, r.days.map((d) => [d.name, d.error, d.fingerprint])]));
      if (sig(res.runs) !== sig(state.runs)) {
        const keepLatest = state.dayB && dayIndex(state.dayB) === readyDays().length - 1;
        state.runs = res.runs;
        const h = readHash();
        if (keepLatest) { delete h.a; delete h.b; }
        if (state.run && res.runs.some((r) => r.name === state.run.name)) setRun(state.run.name, h);
        else loadRuns(h);
      }
    } catch (_) { /* server restarting; try again later */ }
  }

  // ------------------------------------------------------------------ timeline
  async function pollTimeline() {
    clearTimeout(timers.tl);
    if (!state.run) return;
    const run = state.run.name, mode = state.mode;
    let tl;
    try {
      tl = await api('/api/timeline', { run, mode });
    } catch (e) {
      $('tl-sub').textContent = e.message;
      return;
    }
    if (!state.run || state.run.name !== run || state.mode !== mode) return;
    state.timeline = tl;
    const need = tl.pairs.filter((p) => p.state === 'done' && !state.pairs.has(p.key));
    for (let i = 0; i < need.length; i += 4) {
      await Promise.all(need.slice(i, i + 4).map(async (p) => {
        const r = await api('/api/pair', { run, a: p.a, b: p.b });
        if (r.state === 'done') state.pairs.set(r.key, parsePair(r.result));
      }));
    }
    renderTimeline();
    updateJobStatus();
    if (tl.pairs.some((p) => p.state === 'queued' || p.state === 'running')) timers.tl = setTimeout(pollTimeline, 1000);
  }

  function timelineModel() {
    const tl = state.timeline;
    if (!tl) return null;
    const pairs = tl.pairs.map((p) => ({ ...p, data: state.pairs.get(p.key) || null }));
    const rowMap = new Map();
    const perPair = pairs.map((p) => {
      if (!p.data) return null;
      const rows = new Map();
      const sel = p.data.tensors.filter(inGroup);
      for (const t of sel) {
        if (!rows.has(t.row)) rows.set(t.row, []);
        rows.get(t.row).push(t);
        if (!rowMap.has(t.row)) rowMap.set(t.row, { key: t.row, label: t.rowLabel, order: t.rowOrder });
      }
      const rowAgg = new Map();
      for (const [k, list] of rows) rowAgg.set(k, aggregate(list));
      const total = aggregate(sel);
      // Biggest mover tensor by current metric.
      let top = null, topV = -Infinity;
      for (const t of sel) {
        const v = metricOf(aggregate([t]));
        if (finite(v) && v > topV) { topV = v; top = t; }
      }
      return { rowAgg, total, top, topV };
    });
    const rows = [...rowMap.values()].sort((a, b) => a.order - b.order);
    return { pairs, perPair, rows };
  }

  function renderTimeline() {
    const M = METRICS[state.metric];
    const days = readyDays();
    const model = timelineModel();
    const groupNote = state.group === 'all' ? '' : ` · ${GROUP_LABEL[state.group]}`;
    $('tl-line-title').textContent = `Whole-model ${M.label.toLowerCase()}${groupNote}`;
    $('tl-heat-title').textContent = `${M.label} by layer and day${groupNote}`;
    renderTimelineTiles(model, days);
    renderMeta();
    const heat = $('tl-heat');
    if (days.length < 2) {
      heat.textContent = '';
      heat.append(el('div', 'empty', days.length ? 'Only one day so far. Diffs appear once a second day lands.' : 'No readable checkpoints in this run.'));
      renderLine($('tl-line'), { xLabels: [], ys: [] });
      return;
    }
    if (!model) {
      heat.textContent = '';
      heat.append(el('div', 'progress-line', 'Loading…'));
      return;
    }
    const { pairs, perPair, rows } = model;
    const cols = pairs.map((p) => ({ key: p.key, label: shortDay(p.b) }));
    const cell = (ri, ci) => {
      const pp = perPair[ci];
      if (!pp) return pairs[ci].state === 'error' ? null : { pending: true, progress: pairs[ci].progress };
      const s = pp.rowAgg.get(rows[ri].key);
      if (!s || !s.count) return null;
      return { v: metricOf(s), s };
    };
    const values = [];
    rows.forEach((_, ri) => cols.forEach((_, ci) => { const c = cell(ri, ci); if (c && finite(c.v)) values.push(c.v); }));
    const scale = makeScale(values, { log: state.scale === 'log' });
    const selCol = pairs.findIndex((p) => p.a === state.dayA && p.b === state.dayB);
    if (state.tlTable) {
      renderMatrixTable(heat, rows, cols, cell, M.fmt);
    } else {
      renderHeatmap(heat, {
        rows, cols, cell, scale,
        highlightRow: state.highlightRow,
        highlightCol: selCol >= 0 ? cols[selCol].key : null,
        minCellW: 14, maxCellW: 56,
        ariaLabel: `${M.long} per layer for each day`,
        emptyText: 'Computing diffs…',
        tip: (ri, ci, c) => {
          const p = pairs[ci];
          const title = `${p.a} → ${p.b} · ${rows[ri].label}`;
          if (c.pending) return { title, rows: [[c.progress ? fmtPct(c.progress) : 'queued', 'computing']] };
          return {
            title,
            rows: [
              [M.fmt(c.v), M.long],
              [fmtPct(c.s.n ? c.s.ch / c.s.n : null), 'weights changed'],
              [String(c.s.count), 'tensors'],
            ],
          };
        },
        onClick: (ri, ci) => {
          const p = pairs[ci];
          state.highlightRow = rows[ri].key;
          selectPair(p.a, p.b);
          $('day').scrollIntoView({ behavior: 'smooth', block: 'start' });
        },
      });
    }
    const pending = pairs.some((p) => !state.pairs.has(p.key) && p.state !== 'error');
    const errs = pairs.filter((p) => p.state === 'error');
    if (values.length || pending) renderLegend(heat, scale, M.fmt, { noChange: true, pending });
    if (errs.length) heat.append(el('div', 'card-note', `Failed: ${errs.map((p) => `${p.a} → ${p.b}: ${p.error}`).join('; ')}`));

    // Whole-model line, positioned by day.
    const ys = days.map(() => null);
    const notes = {};
    days.forEach((d, i) => { if (d.meta && d.meta.notes) notes[i] = String(d.meta.notes); });
    pairs.forEach((p, ci) => {
      const i = days.findIndex((d) => d.name === p.b);
      if (i >= 0 && perPair[ci]) ys[i] = metricOf(perPair[ci].total);
    });
    renderLine($('tl-line'), {
      xLabels: days.map((d) => shortDay(d.name)),
      ys,
      notes,
      fmt: M.fmt,
      height: 190,
      ariaLabel: `Whole-model ${M.long} per day`,
      emptyText: 'Computing diffs…',
      tip: (i) => {
        const d = days[i];
        const ci = pairs.findIndex((p) => p.b === d.name);
        const rowsOut = [];
        if (ci < 0) rowsOut.push(['—', state.mode === 'baseline' ? 'baseline day' : 'first day']);
        else if (!perPair[ci]) rowsOut.push(['…', 'computing']);
        else {
          rowsOut.push([M.fmt(ys[i]), M.long, cssVar('--series-1')]);
          const top = perPair[ci].top;
          if (top) rowsOut.push([M.fmt(perPair[ci].topV), `top: ${top.rowLabel} ${top.moduleLabel}`]);
        }
        if (d.meta && finite(d.meta.loss)) rowsOut.push([fmtNum(d.meta.loss), 'loss']);
        if (d.meta && d.meta.notes) rowsOut.push([String(d.meta.notes), 'note']);
        const title = ci >= 0 ? `${pairs[ci].a} → ${d.name}` : d.name;
        return { title, rows: rowsOut };
      },
      onClick: (i) => {
        const ci = pairs.findIndex((p) => p.b === days[i].name);
        if (ci >= 0) { selectPair(pairs[ci].a, pairs[ci].b); $('day').scrollIntoView({ behavior: 'smooth', block: 'start' }); }
      },
    });
  }

  function renderTimelineTiles(model, days) {
    const host = $('tl-tiles');
    host.textContent = '';
    const M = METRICS[state.metric];
    const tile = (k, v, d, small) => {
      const t = el('div', 'tile');
      t.append(el('div', 'k', k), el('div', 'v' + (small ? ' small' : ''), v));
      if (d) t.append(el('div', 'd', d));
      host.append(t);
    };
    const last = days[days.length - 1];
    tile('Days of training', String(Math.max(0, days.length - 1)), days.length ? `${days[0].name} → ${last.name}` : '');
    tile('Parameters', last ? fmtCount(last.n_params) : '–', last ? `${last.n_tensors} tensors · ${fmtBytes(last.nbytes)} per day` : '');
    if (!model) return;
    const done = model.perPair.map((p, i) => (p ? i : -1)).filter((i) => i >= 0);
    if (!done.length) {
      tile(`Latest ${M.label.toLowerCase()}`, '…', 'computing');
      return;
    }
    const li = done[done.length - 1];
    const latest = metricOf(model.perPair[li].total);
    let delta = '';
    if (state.mode === 'consecutive' && done.length > 1) {
      const prev = metricOf(model.perPair[done[done.length - 2]].total);
      if (finite(prev) && prev > 0 && finite(latest)) {
        const r = latest / prev - 1;
        delta = `${r >= 0 ? '↑' : '↓'} ${fmtPct(Math.abs(r))} vs. previous day`;
      }
    }
    const p = model.pairs[li];
    tile(`${state.mode === 'baseline' ? 'Drift since day one' : `Latest day's ${M.label.toLowerCase()}`}`, M.fmt(latest), delta || `${p.a} → ${p.b}`);
    const top = model.perPair[li].top;
    if (top) tile('Biggest mover (latest)', `${top.rowLabel} · ${top.moduleLabel}`, `${M.fmt(model.perPair[li].topV)} ${M.label.toLowerCase()}`, true);
  }

  function renderMeta() {
    const days = readyDays();
    const fields = new Set();
    days.forEach((d) => Object.entries(d.meta || {}).forEach(([k, v]) => { if (finite(v) && !['day'].includes(k)) fields.add(k); }));
    const card = $('tl-meta-card');
    if (!fields.size) { card.hidden = true; return; }
    card.hidden = false;
    const sel = $('meta-field');
    const list = [...fields];
    const prev = sel.value;
    if (sel.options.length !== list.length || [...sel.options].some((o, i) => o.value !== list[i])) {
      sel.textContent = '';
      list.forEach((f) => sel.append(new Option(f, f)));
    }
    sel.value = list.includes(prev) ? prev : list.includes('loss') ? 'loss' : list[0];
    const f = sel.value;
    const ys = days.map((d) => (d.meta && finite(d.meta[f]) ? d.meta[f] : null));
    const fmt = (v) => (Math.abs(v) >= 1e4 ? fmtCount(v) : fmtNum(v));
    renderLine($('tl-meta'), {
      xLabels: days.map((d) => shortDay(d.name)),
      ys,
      fmt,
      color: cssVar('--series-2'),
      height: 140,
      yZero: false,
      ariaLabel: `${f} per day`,
      emptyText: `No numeric “${f}” in meta.json`,
      tip: (i) => ({ title: days[i].name, rows: [[finite(ys[i]) ? fmt(ys[i]) : '–', f, cssVar('--series-2')]] }),
    });
  }

  // ------------------------------------------------------------------ day view
  function selectPair(a, b) {
    if (!a || !b) return;
    state.dayA = a;
    state.dayB = b;
    $('day-a').value = a;
    $('day-b').value = b;
    writeHash();
    loadDayPair();
    renderTimeline();
    if (state.tensor) loadTensor();
  }

  function shiftPair(step) {
    const ia = dayIndex(state.dayA), ib = dayIndex(state.dayB), n = readyDays().length;
    if (ia < 0 || ib < 0) return;
    const na = ia + step, nb = ib + step;
    if (na < 0 || nb < 0 || na >= n || nb >= n) return;
    selectPair(readyDays()[na].name, readyDays()[nb].name);
  }

  async function loadDayPair() {
    clearTimeout(timers.day);
    const { dayA, dayB } = state;
    if (!state.run || !dayA || !dayB) return;
    const run = state.run.name;
    let r;
    try {
      r = await api('/api/pair', { run, a: dayA, b: dayB });
    } catch (e) {
      r = { state: 'error', error: e.message };
    }
    if (!state.run || state.run.name !== run || state.dayA !== dayA || state.dayB !== dayB) return;
    if (r.state === 'done') {
      state.pairs.set(r.key, parsePair(r.result));
      state.dayKey = r.key;
      state.dayStatus = null;
    } else {
      state.dayStatus = r;
      if (r.state !== 'error') timers.day = setTimeout(loadDayPair, 700);
    }
    renderDay();
    updateJobStatus();
  }

  function currentPair() {
    const p = state.dayKey && state.pairs.get(state.dayKey);
    return p && p.a === state.dayA && p.b === state.dayB ? p : null;
  }

  function renderDay() {
    const n = readyDays().length;
    const ia = dayIndex(state.dayA), ib = dayIndex(state.dayB);
    $('day-prev').disabled = !(ia > 0 && ib > 0);
    $('day-next').disabled = !(ia >= 0 && ib >= 0 && ia < n - 1 && ib < n - 1);
    const pair = currentPair();
    const st = state.dayStatus;
    const containers = [$('day-heat'), $('tensor-table'), $('day-tiles')];
    const sub = $('day-sub');
    if (!pair) {
      const prevPair = state.dayKey && state.pairs.get(state.dayKey);
      if (st && st.state === 'error') {
        sub.textContent = '';
        $('day-heat').textContent = '';
        $('day-heat').append(el('div', 'empty', `Could not diff: ${st.error}`));
        return;
      }
      if (st) {
        const pct = st.state === 'running' ? fmtPct(st.progress || 0) : 'queued';
        sub.textContent = `Computing ${state.dayA} → ${state.dayB}: ${pct}`;
        if (prevPair) { containers.forEach((c) => c.classList.add('loading')); return; }
        const heat = $('day-heat');
        heat.textContent = '';
        const pl = el('div', 'progress-line', `Reading both checkpoints and diffing every tensor… ${pct}`);
        const bar = el('div', 'bar'), fill = el('i');
        fill.style.width = ((st.progress || 0) * 100).toFixed(1) + '%';
        bar.append(fill);
        pl.append(bar);
        heat.append(pl);
      }
      return;
    }
    containers.forEach((c) => c.classList.remove('loading'));
    sub.textContent = `${pair.tensors.length} tensors · diffed in ${pair.elapsed}s`;
    renderDayAlerts(pair);
    renderDayTiles(pair);
    renderDayHeat(pair);
    renderTensorTable();
  }

  function renderDayAlerts(pair) {
    const host = $('day-alerts');
    host.textContent = '';
    const add = (level, icon, text) => {
      const a = el('div', 'alert ' + level);
      a.append(el('span', 'ic', icon), el('span', null, text));
      host.append(a);
    };
    const nonfinite = pair.tensors.filter((t) => t.nonfinite > 0);
    if (nonfinite.length) add('critical', '⚠', `Non-finite values (NaN/Inf) in ${nonfinite.length} tensor(s) on ${pair.b}: ${nonfinite.slice(0, 4).map((t) => t.name).join(', ')}${nonfinite.length > 4 ? '…' : ''}`);
    const changes = { added: [], removed: [], reshaped: [] };
    pair.tensors.forEach((t) => { if (changes[t.status]) changes[t.status].push(t.name); });
    for (const [k, list] of Object.entries(changes)) {
      if (list.length) add('warning', '!', `${list.length} tensor(s) ${k}: ${list.slice(0, 4).join(', ')}${list.length > 4 ? '…' : ''}`);
    }
  }

  function renderDayTiles(pair) {
    const host = $('day-tiles');
    host.textContent = '';
    const M = METRICS[state.metric];
    const sel = pair.tensors.filter(inGroup);
    const s = aggregate(sel);
    const tile = (k, v, d, small) => {
      const t = el('div', 'tile');
      t.append(el('div', 'k', k), el('div', 'v' + (small ? ' small' : ''), v));
      if (d) t.append(el('div', 'd', d));
      host.append(t);
    };
    const groupNote = state.group === 'all' ? 'whole model' : GROUP_LABEL[state.group];
    tile(M.long, M.fmt(metricOf(s)), groupNote);
    const rel = METRICS.rel.f(s);
    if (state.metric !== 'rel') tile('Relative change', fmtPct(rel), '‖ΔW‖ / ‖W‖');
    tile('Weights changed', fmtPct(s.n ? s.ch / s.n : null), `${fmtCount(s.ch)} of ${fmtCount(s.n)}`);
    tile('Direction change', fmtNum(cosDist(s)), '1 − cosine(W_a, W_b)');
    let top = null, topV = -Infinity;
    for (const t of sel) {
      const v = metricOf(aggregate([t]));
      if (finite(v) && v > topV) { topV = v; top = t; }
    }
    if (top) tile('Biggest mover', `${top.rowLabel} · ${top.moduleLabel}`, `${M.fmt(topV)} ${M.label.toLowerCase()}`, true);
  }

  function renderDayHeat(pair) {
    const M = METRICS[state.metric];
    const sel = pair.tensors.filter(inGroup);
    const rows = rowsOf(sel);
    const colMap = new Map();
    for (const t of sel) if (!colMap.has(t.module)) colMap.set(t.module, { key: t.module, label: t.moduleLabel, order: t.colOrder });
    const cols = [...colMap.values()].sort((a, b) => a.order - b.order || a.label.localeCompare(b.label));
    const cells = new Map();
    for (const t of sel) {
      const k = t.row + '\u0000' + t.module;
      if (!cells.has(k)) cells.set(k, []);
      cells.get(k).push(t);
    }
    const cache = new Map();
    const cell = (ri, ci) => {
      const k = rows[ri].key + '\u0000' + cols[ci].key;
      if (cache.has(k)) return cache.get(k);
      const list = cells.get(k);
      let out = null;
      if (list) {
        const s = aggregate(list);
        out = s.count ? { v: metricOf(s), s, list } : null;
      }
      cache.set(k, out);
      return out;
    };
    const values = [];
    rows.forEach((_, ri) => cols.forEach((_, ci) => { const c = cell(ri, ci); if (c && finite(c.v)) values.push(c.v); }));
    const scale = makeScale(values, { log: state.scale === 'log' });
    const host = $('day-heat');
    const groupNote = state.group === 'all' ? '' : ` · ${GROUP_LABEL[state.group]}`;
    $('day-heat-title').textContent = `${M.label}: layer × module${groupNote}`;
    if (state.dayTable) {
      renderMatrixTable(host, rows, cols, cell, M.fmt);
      return;
    }
    const selT = state.tensor && sel.find((t) => t.name === state.tensor);
    renderHeatmap(host, {
      rows, cols, cell, scale,
      highlightRow: selT ? selT.row : state.highlightRow,
      highlightCol: selT ? selT.module : null,
      minCellW: 16, maxCellW: 64,
      ariaLabel: `${M.long} per layer and module`,
      tip: (ri, ci, c) => ({
        title: c.list.length === 1 ? c.list[0].name : `${rows[ri].label} · ${cols[ci].label} (${c.list.length} tensors)`,
        rows: [
          [M.fmt(c.v), M.long],
          [fmtPct(c.s.n ? c.s.ch / c.s.n : null), 'weights changed'],
          [c.list.length === 1 ? shape(c.list[0].shape) : fmtCount(c.s.n), c.list.length === 1 ? 'shape' : 'weights'],
        ],
      }),
      onClick: (ri, ci, c) => {
        let best = c.list[0], bv = -Infinity;
        for (const t of c.list) { const v = metricOf(aggregate([t])); if (finite(v) && v > bv) { bv = v; best = t; } }
        selectTensor(best.name);
      },
    });
    if (values.length) renderLegend(host, scale, M.fmt, { noChange: true });
  }

  const COLUMNS = [
    { key: 'name', label: 'Tensor', get: (t) => t.name, text: true },
    { key: 'shape', label: 'Shape', get: (t) => t.numel, show: (t) => shape(t.shape) },
    { key: 'rel', label: 'Rel. change', get: (t) => METRICS.rel.f(aggregate([t])), show: (v) => fmtPct(v) },
    { key: 'rms', label: 'RMS ΔW', get: (t) => METRICS.rms.f(aggregate([t])), show: (v) => fmtNum(v) },
    { key: 'norm', label: '‖ΔW‖', get: (t) => METRICS.norm.f(aggregate([t])), show: (v) => fmtNum(v) },
    { key: 'maxabs', label: 'max |ΔW|', get: (t) => t.maxabs, show: (v) => fmtNum(v) },
    { key: 'cosdist', label: '1 − cos', get: (t) => cosDist(aggregate([t])), show: (v) => fmtNum(v) },
    { key: 'changed', label: 'Changed', get: (t) => (t.numel ? t.ch / t.numel : null), show: (v) => fmtPct(v) },
  ];
  const TABLE_LIMIT = 400;

  function renderTensorTable() {
    const pair = currentPair();
    const table = $('tensor-table');
    table.textContent = '';
    if (!pair) return;
    const sort = state.sort || { key: state.metric, dir: -1 };
    const col = COLUMNS.find((c) => c.key === sort.key) || COLUMNS[2];
    let list = pair.tensors.filter(inGroup);
    if (state.search) list = list.filter((t) => t.name.toLowerCase().includes(state.search));
    const keyed = list.map((t) => ({ t, k: col.get(t) }));
    keyed.sort((x, y) => {
      if (col.text) return sort.dir * String(x.k).localeCompare(String(y.k));
      const a = finite(x.k) ? x.k : -Infinity, b = finite(y.k) ? y.k : -Infinity;
      return sort.dir * (a - b);
    });
    const thead = el('thead'), hr = el('tr');
    COLUMNS.forEach((c) => {
      const th = el('th', null, c.label);
      th.scope = 'col';
      th.tabIndex = 0;
      if (c.key === col.key) th.setAttribute('aria-sort', sort.dir < 0 ? 'descending' : 'ascending');
      const go = () => {
        const cur = state.sort || { key: state.metric, dir: -1 };
        state.sort = { key: c.key, dir: cur.key === c.key ? -cur.dir : c.text ? 1 : -1 };
        renderTensorTable();
      };
      th.addEventListener('click', go);
      th.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); go(); } });
      hr.append(th);
    });
    thead.append(hr);
    const tbody = el('tbody');
    keyed.slice(0, TABLE_LIMIT).forEach(({ t }) => {
      const tr = el('tr');
      tr.tabIndex = 0;
      if (t.name === state.tensor) tr.className = 'selected';
      const name = el('td', 'name', t.name.replace(/^model\./, '').replace(/\.weight$/, ''));
      name.title = t.name;
      if (t.status !== 'ok') name.append(el('span', 'st', t.status));
      tr.append(name);
      COLUMNS.slice(1).forEach((c) => {
        if (t.status !== 'ok' && c.key !== 'shape') { tr.append(el('td', null, '–')); return; }
        tr.append(el('td', null, c.key === 'shape' ? c.show(t) : c.show(c.get(t))));
      });
      const open = () => { if (t.status === 'ok') selectTensor(t.name); };
      tr.addEventListener('click', open);
      tr.addEventListener('keydown', (e) => { if (e.key === 'Enter') open(); });
      tbody.append(tr);
    });
    table.append(thead, tbody);
    $('tensor-table-note').textContent = keyed.length > TABLE_LIMIT ? `Showing ${TABLE_LIMIT} of ${keyed.length}. Use the filter to narrow down.` : `${keyed.length} tensor${keyed.length === 1 ? '' : 's'}`;
  }

  // ------------------------------------------------------------------ tensor detail
  function selectTensor(name) {
    state.tensor = name;
    writeHash();
    loadTensor();
    renderTensorTable();
    const pair = currentPair();
    if (pair) renderDayHeat(pair);
    $('tensor').hidden = false;
    $('tensor').scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  async function loadTensor() {
    if (!state.tensor || !state.run || !state.dayA || !state.dayB) return;
    const req = ++state.seq;
    const params = { run: state.run.name, a: state.dayA, b: state.dayB, name: state.tensor };
    state.tensorError = null;
    $('tensor').hidden = false;
    for (const id of ['tmap', 'hist', 'spectrum', 'tensor-tiles']) $(id).classList.add('loading');
    if (!state.tensorDetail || state.tensorDetail.name !== state.tensor) {
      state.tensorDetail = null;
      state.tensorSpectrum = null;
      renderTensor();
    }
    try {
      const d = await api('/api/tensor', params);
      if (req !== state.seq) return;
      state.tensorDetail = d;
      state.tensorSpectrum = null;
      renderTensor();
      const s = await api('/api/spectrum', params);
      if (req !== state.seq) return;
      state.tensorSpectrum = s;
      renderSpectrum();
    } catch (e) {
      if (req !== state.seq) return;
      state.tensorError = e.message;
      renderTensor();
    }
  }

  function renderTensor() {
    const sec = $('tensor');
    if (!state.tensor) { sec.hidden = true; return; }
    sec.hidden = false;
    $('tensor-name').textContent = state.tensor;
    const d = state.tensorDetail;
    for (const id of ['tmap', 'hist', 'spectrum', 'tensor-tiles']) $(id).classList.toggle('loading', !d && !state.tensorError);
    if (state.tensorError) {
      $('tensor-sub').textContent = state.tensorError;
      for (const id of ['tmap', 'hist', 'spectrum', 'tensor-tiles']) $(id).textContent = '';
      return;
    }
    if (!d) {
      $('tensor-sub').textContent = 'Loading…';
      return;
    }
    $('tensor-sub').textContent = `${shape(d.shape)} · ${d.dtype_a === d.dtype_b ? d.dtype_a : `${d.dtype_a} → ${d.dtype_b}`} · ${state.dayA} → ${state.dayB}`;
    const host = $('tensor-tiles');
    host.textContent = '';
    const s = d.stats;
    const tile = (k, v, sub) => {
      const t = el('div', 'tile');
      t.append(el('div', 'k', k), el('div', 'v', v));
      if (sub) t.append(el('div', 'd', sub));
      host.append(t);
    };
    tile('Relative change', fmtPct(s.rel), `‖ΔW‖ ${fmtNum(s.norm_d)} / ‖W‖ ${fmtNum(s.norm_a)}`);
    tile('RMS update', fmtNum(s.rms_d), `max |ΔW| ${fmtNum(s.maxabs)}`);
    tile('Weights changed', fmtPct(s.changed_frac), `${fmtCount(Math.round(s.changed_frac * s.numel))} of ${fmtCount(s.numel)}`);
    tile('Direction change', fmtNum(finite(s.cos) ? 1 - s.cos : null), '1 − cosine');
    renderTensorMap();
    renderHistogram();
    renderSpectrum();
  }

  function renderTensorMap() {
    const d = state.tensorDetail;
    const host = $('tmap');
    host.textContent = '';
    host.classList.remove('loading');
    if (!d) return;
    const g = d.grid;
    const [R, C] = d.matrix;
    const seg = $('tmap-mode');
    if (g.rows === 1) {
      seg.hidden = true;
      $('tmap-title').textContent = 'Update profile (1-D tensor)';
      const mean = decodeF32(g.delta_mean), lo = decodeF32(g.delta_min), hi = decodeF32(g.delta_max);
      const [, bc] = g.block;
      const xs = Array.from({ length: g.cols }, (_, j) => String(j * bc));
      renderLine(host, {
        xLabels: xs,
        ys: Array.from(mean),
        band: Array.from(mean, (_, j) => [lo[j], hi[j]]),
        fmt: fmtNum,
        yZero: false,
        endLabel: false,
        height: 260,
        ariaLabel: 'Change per index',
        tip: (j) => ({
          title: bc > 1 ? `index ${j * bc}–${Math.min(C, (j + 1) * bc) - 1}` : `index ${j}`,
          rows: bc > 1
            ? [[fmtNum(mean[j]), 'mean ΔW', cssVar('--series-1')], [`${fmtNum(lo[j])} … ${fmtNum(hi[j])}`, 'range']]
            : [[fmtNum(mean[j]), 'ΔW', cssVar('--series-1')]],
        }),
      });
      host.append(el('div', 'card-note', bc > 1 ? `Line: mean ΔW per ${bc} entries; band: min–max.` : 'ΔW per entry.'));
      return;
    }
    seg.hidden = false;
    const mode = state.tmapMode;
    $('tmap-title').textContent = mode === 'weight' ? `Weights on ${state.dayB} (RMS per cell)` : mode === 'signed' ? 'Update map, signed (mean ΔW per cell)' : 'Update map (RMS |ΔW| per cell)';
    const vals = decodeF32(mode === 'rms' ? g.delta_rms : mode === 'signed' ? g.delta_mean : g.weight_rms);
    const fin = Array.from(vals).filter(finite);
    const scale = makeScale(fin, { kind: mode === 'signed' ? 'div' : 'seq', log: mode === 'rms' && state.scale === 'log', hiQ: 0.995 });
    const off = document.createElement('canvas');
    off.width = g.cols;
    off.height = g.rows;
    const octx = off.getContext('2d');
    const img = octx.createImageData(g.cols, g.rows);
    const nc = hexRgb(cssVar('--nochange').startsWith('#') ? cssVar('--nochange') : '#dcdad3');
    const bad = hexRgb(isDark() ? '#e66767' : '#d03b3b');
    for (let i = 0; i < vals.length; i++) {
      const v = vals[i];
      const c = !finite(v) ? bad : v === 0 && mode !== 'signed' ? nc : scale.rgb(v);
      img.data[i * 4] = c[0]; img.data[i * 4 + 1] = c[1]; img.data[i * 4 + 2] = c[2]; img.data[i * 4 + 3] = 255;
    }
    octx.putImageData(img, 0, 0);
    const maxW = Math.max(200, host.clientWidth);
    // Roughly true aspect, but squash very tall matrices (embeddings) so they use the card's width.
    const aspect = clamp(R / C, 0.3, 0.9);
    let w = maxW, h = Math.round(w * aspect);
    if (h > 600) { h = 600; w = Math.round(h / aspect); }
    const wrap = el('div', 'tmap');
    wrap.style.width = w + 'px';
    wrap.style.maxWidth = '100%';
    const canvas = el('canvas');
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = w + 'px';
    canvas.style.height = h + 'px';
    canvas.setAttribute('role', 'img');
    canvas.setAttribute('aria-label', `${mode} map of ${d.name}`);
    const ctx = canvas.getContext('2d');
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(off, 0, 0, canvas.width, canvas.height);
    wrap.append(canvas);
    const note = el('div', 'axis-note');
    const isTok = /embed|lm_head|wte|token_embd|^output\.weight$/.test(d.name);
    note.append(el('span', null, `↓ rows 0–${R - 1}${isTok ? ' (token ids)' : ''}`), el('span', null, `cols 0–${C - 1} →`));
    wrap.append(note);
    host.append(wrap);
    const [br, bc] = g.block;
    host.append(el('div', 'card-note', br * bc > 1 ? `Each cell summarises ${br}×${bc} = ${fmtCount(br * bc)} weights.` : 'One cell per weight.'));
    renderLegend(host, scale, fmtNum, { noChange: mode !== 'signed' && fin.some((v) => v === 0) });
    const label = mode === 'rms' ? 'RMS |ΔW|' : mode === 'signed' ? 'mean ΔW' : 'RMS W';
    canvas.addEventListener('pointermove', (e) => {
      const r = canvas.getBoundingClientRect();
      const gi = clamp(Math.floor(((e.clientY - r.top) / r.height) * g.rows), 0, g.rows - 1);
      const gj = clamp(Math.floor(((e.clientX - r.left) / r.width) * g.cols), 0, g.cols - 1);
      const r0 = gi * br, r1 = Math.min(R, (gi + 1) * br) - 1, c0 = gj * bc, c1 = Math.min(C, (gj + 1) * bc) - 1;
      const v = vals[gi * g.cols + gj];
      showTip(e, `rows ${r0 === r1 ? r0 : `${r0}–${r1}`} · cols ${c0 === c1 ? c0 : `${c0}–${c1}`}`, [[fmtNum(v), label]]);
    });
    canvas.addEventListener('pointerleave', hideTip);
  }

  function renderHistogram() {
    const d = state.tensorDetail;
    const host = $('hist');
    host.classList.remove('loading');
    if (!d) { host.textContent = ''; return; }
    const h = d.hist;
    const n = h.counts.length;
    const note = $('hist-note');
    if (!n || !h.edges.length) {
      host.textContent = '';
      host.append(el('div', 'empty', 'No weights changed.'));
      note.textContent = '';
      return;
    }
    const lim = h.edges[n];
    renderBars(host, {
      bars: h.counts.map((v) => ({ v })),
      xAt: (i) => [i / n, (i + 1) / n],
      xTicks: [
        { at: 0, label: fmtNum(-lim), anchor: 'start' },
        { at: 0.25, label: fmtNum(-lim / 2) },
        { at: 0.5, label: '0' },
        { at: 0.75, label: fmtNum(lim / 2) },
        { at: 1, label: fmtNum(lim), anchor: 'end' },
      ],
      yLog: state.histScale === 'log',
      fmt: fmtCount,
      height: 170,
      ariaLabel: 'Histogram of non-zero weight changes',
      tip: (i) => ({ title: `ΔW ${fmtNum(h.edges[i])} … ${fmtNum(h.edges[i + 1])}`, rows: [[fmtCount(h.counts[i]), 'weights']] }),
    });
    const parts = [`${fmtCount(h.zeros)} unchanged weights (ΔW = 0) not shown.`];
    if (h.below + h.above) parts.push(`${fmtCount(h.below + h.above)} beyond ±${fmtNum(lim)}.`);
    note.textContent = parts.join(' ');
  }

  function renderSpectrum() {
    const host = $('spectrum');
    const note = $('spectrum-note');
    const s = state.tensorSpectrum;
    host.classList.remove('loading');
    if (!state.tensorDetail) { host.textContent = ''; note.textContent = ''; return; }
    if (!s) {
      host.textContent = '';
      host.append(el('div', 'progress-line', 'Computing singular values of ΔW…'));
      note.textContent = '';
      return;
    }
    if (!s.values.length) {
      host.textContent = '';
      host.append(el('div', 'empty', 'Spectrum needs a 2-D tensor.'));
      note.textContent = '';
      return;
    }
    if (!(s.total > 0)) {
      host.textContent = '';
      host.append(el('div', 'empty', 'No change, so no spectrum.'));
      note.textContent = '';
      return;
    }
    const k = s.share.length;
    let cum = 0;
    const cumul = s.share.map((v) => (cum += v));
    renderBars(host, {
      bars: s.share.map((v) => ({ v })),
      xAt: (i) => [i / k, (i + 1) / k],
      xTicks: [
        { at: 0.5 / k, label: '1', anchor: 'middle' },
        { at: (Math.min(k, 8) - 0.5) / k, label: String(Math.min(k, 8)) },
        { at: (k - 0.5) / k, label: String(k), anchor: 'middle' },
      ],
      fmt: (v) => fmtPct(v),
      height: 150,
      ariaLabel: 'Share of update energy per singular direction',
      tip: (i) => ({
        title: `Direction ${i + 1}`,
        rows: [[fmtPct(s.share[i]), 'of update energy'], [fmtPct(cumul[i]), `top ${i + 1} combined`], [fmtNum(s.values[i]), 'singular value']],
      }),
    });
    const at = (j) => fmtPct(cumul[Math.min(k, j) - 1]);
    const top4 = cumul[Math.min(k, 4) - 1], topK = cumul[k - 1];
    const verdict = top4 > 0.6 ? 'Low-rank update: a few directions dominate.'
      : topK < 0.5 ? 'Diffuse update: energy is spread across many directions.'
      : 'Mixed: a few strong directions on top of a diffuse tail.';
    note.textContent = `Top direction carries ${at(1)} of ‖ΔW‖²; top ${Math.min(4, k)}: ${at(4)}; top ${k}: ${at(k)}. ${verdict}` +
      (s.method === 'randomized' ? ' (randomized SVD)' : '');
  }

  // ------------------------------------------------------------------ job status
  function updateJobStatus() {
    const host = $('jobstatus');
    host.textContent = '';
    const pend = [];
    if (state.timeline) state.timeline.pairs.forEach((p) => { if (p.state === 'queued' || p.state === 'running') pend.push(p); });
    if (state.dayStatus && (state.dayStatus.state === 'queued' || state.dayStatus.state === 'running') && !pend.some((p) => p.key === state.dayStatus.key)) pend.push(state.dayStatus);
    if (!pend.length) return;
    const running = pend.find((p) => p.state === 'running');
    host.append(document.createTextNode(`Diffing ${pend.length} day pair${pend.length === 1 ? '' : 's'}`));
    const bar = el('span', 'bar'), fill = el('i');
    fill.style.width = (((running && running.progress) || 0) * 100).toFixed(1) + '%';
    bar.append(fill);
    host.append(bar);
  }

  function renderAll() {
    hideTip();
    renderTimeline();
    renderDay();
    if (state.tensorDetail) renderTensor();
  }

  // ------------------------------------------------------------------ boot
  try {
    const saved = localStorage.getItem('weightviz-theme');
    if (saved === 'light' || saved === 'dark') document.documentElement.dataset.theme = saved;
  } catch (_) { /* storage unavailable */ }
  const h0 = readHash();
  if (METRICS[h0.metric]) state.metric = h0.metric;
  if (GROUP_LABEL[h0.group]) state.group = h0.group;
  if (h0.scale === 'linear') state.scale = 'linear';
  if (h0.mode === 'baseline') state.mode = 'baseline';
  $('metric').value = state.metric;
  $('group').value = state.group;
  setSeg('scale', state.scale);
  setSeg('mode', state.mode);
  themeLabel();
  wire();
  loadRuns(h0);
  setInterval(autoRescan, 60000);
})();
