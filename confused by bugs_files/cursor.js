const SLUG = document.body.dataset.slug;
const WS_URL = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws/${SLUG}`;

const CURSOR_SVG = `<svg width="12" height="18" viewBox="0 0 12 18" xmlns="http://www.w3.org/2000/svg">
  <path d="M1 1 L1 15 L4 11.5 L6.5 16.5 L8 15.8 L5.5 10.8 L9.5 10.8 Z"
        fill="black" stroke="white" stroke-width="1.5" stroke-linejoin="round"/>
</svg>`;

// All block-level text containers
function getBlocks() {
  return [...document.querySelectorAll('p, li, h1, h2, h3, h4, h5, h6')];
}

// Absolute char offset within a block, walking all text nodes
function absoluteOffset(block, container, nodeOffset) {
  const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
  let total = 0, node;
  while ((node = walker.nextNode())) {
    if (node === container) return total + nodeOffset;
    total += node.length;
  }
  return total;
}

// Range at absolute char offset within a block
function rangeAtOffset(block, absOffset) {
  const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
  let total = 0, node;
  while ((node = walker.nextNode())) {
    if (total + node.length >= absOffset) {
      const r = document.createRange();
      r.setStart(node, absOffset - total);
      r.setEnd(node, absOffset - total);
      return r;
    }
    total += node.length;
  }
  return null;
}

// Cross-browser caret from point
function caretAt(x, y) {
  if (document.caretRangeFromPoint) return document.caretRangeFromPoint(x, y);
  if (document.caretPositionFromPoint) {
    const pos = document.caretPositionFromPoint(x, y);
    if (!pos) return null;
    const r = document.createRange();
    r.setStart(pos.offsetNode, pos.offset);
    r.setEnd(pos.offsetNode, pos.offset);
    return r;
  }
  return null;
}

// Fixed overlay layer
const layer = document.createElement('div');
layer.style.cssText = 'position:fixed;top:0;left:0;width:100%;height:100%;pointer-events:none;z-index:9999;overflow:hidden;';
document.body.appendChild(layer);

// State
const remoteCursors = new Map(); // id -> {block, offset, dx, dy}
const cursorEls = new Map();     // id -> element

function getOrCreateEl(id) {
  if (!cursorEls.has(id)) {
    const el = document.createElement('div');
    el.style.cssText = 'position:absolute;top:0;left:0;will-change:transform;';
    el.innerHTML = CURSOR_SVG;
    layer.appendChild(el);
    cursorEls.set(id, el);
  }
  return cursorEls.get(id);
}

function renderCursors() {
  const blocks = getBlocks();
  const snappedGroups = new Map(); // summaryEl -> [id]

  for (const [id, c] of remoteCursors) {
    const block = blocks[c.block];
    if (!block) continue;

    const details = block.closest('details');
    if (details && !details.open) {
      const summary = details.querySelector('summary');
      if (!snappedGroups.has(summary)) snappedGroups.set(summary, []);
      snappedGroups.get(summary).push(id);
      continue;
    }

    const range = rangeAtOffset(block, c.offset);
    if (!range) continue;

    // Get character rect for sub-char displacement
    const charRange = range.cloneRange();
    try {
      const node = charRange.startContainer;
      charRange.setEnd(node, Math.min(charRange.startOffset + 1, node.length));
    } catch (_) {}

    const rect = charRange.getBoundingClientRect();
    const cw = rect.width || 8;
    const ch = rect.height || 16;
    const x = rect.left + c.dx * cw;
    const y = rect.top + c.dy * ch;

    const el = getOrCreateEl(id);
    el.style.transform = `translate(${x}px,${y}px)`;
  }

  // Render snapped groups as a horizontal row after the summary
  for (const [summary, ids] of snappedGroups) {
    const rect = summary.getBoundingClientRect();
    ids.forEach((id, i) => {
      const el = getOrCreateEl(id);
      el.style.transform = `translate(${rect.right + 2 + i * 14}px,${rect.top}px)`;
    });
  }
}

// WebSocket
let ws;
function connect() {
  ws = new WebSocket(WS_URL);
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.type === 'cursor') {
      remoteCursors.set(msg.id, { block: msg.block, offset: msg.offset, dx: msg.dx, dy: msg.dy });
      renderCursors();
    } else if (msg.type === 'leave') {
      remoteCursors.delete(msg.id);
      const el = cursorEls.get(msg.id);
      if (el) { layer.removeChild(el); cursorEls.delete(msg.id); }
    } else if (msg.type === 'count') {
      const el = document.getElementById('viewer-count');
      if (el) el.textContent = `${msg.n} reading now`;
    }
  };
  ws.onclose = () => setTimeout(connect, 2000);
}
connect();

// Throttle helper
function throttle(fn, ms) {
  let last = 0;
  return (...args) => { const now = Date.now(); if (now - last >= ms) { last = now; fn(...args); } };
}

// Track mouse → encode block/offset/dx/dy → send
document.addEventListener('mousemove', throttle((e) => {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;

  const range = caretAt(e.clientX, e.clientY);
  if (!range || range.startContainer.nodeType !== Node.TEXT_NODE) return;

  const blocks = getBlocks();
  const blockIdx = blocks.findIndex(b => b.contains(range.startContainer));
  if (blockIdx === -1) return;

  const offset = absoluteOffset(blocks[blockIdx], range.startContainer, range.startOffset);

  const charRange = range.cloneRange();
  try {
    charRange.setEnd(range.startContainer, Math.min(range.startOffset + 1, range.startContainer.length));
  } catch (_) {}
  const rect = charRange.getBoundingClientRect();
  const dx = rect.width  > 0 ? (e.clientX - rect.left) / rect.width  : 0;
  const dy = rect.height > 0 ? (e.clientY - rect.top)  / rect.height : 0;

  ws.send(JSON.stringify({ type: 'cursor', block: blockIdx, offset, dx, dy }));
}, 40));

// Re-render on scroll since viewport rects shift
document.addEventListener('scroll', () => renderCursors(), { passive: true });
