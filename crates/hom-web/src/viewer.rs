/// Single-page HTML viewer served at GET /.
/// Renders pane cells onto a <canvas> using Canvas2D (fillText / fillRect).
/// No innerHTML is used — cell text is always set via fillText, which is XSS-safe.
pub const VIEWER_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>HOM — Web View</title>
<style>
  body { background:#0d1117; color:#c9d1d9; font-family:monospace; margin:0; padding:8px; }
  #status { color:#8b949e; font-size:12px; margin-bottom:8px; }
  #panes { display:flex; flex-wrap:wrap; gap:8px; }
  .pane { border:1px solid #30363d; border-radius:4px; background:#161b22; }
  .pane.focused { border-color:#58a6ff; }
  .pane-title { color:#58a6ff; font-size:11px; padding:2px 4px; border-bottom:1px solid #30363d; }
</style>
</head>
<body>
<div id="status">Connecting...</div>
<div id="panes"></div>
<script>
const CELL_W = 9, CELL_H = 16;
const FONT = '13px monospace';
const DEFAULT_FG = '#c9d1d9', DEFAULT_BG = '#161b22';

function toHex(n) {
  if (n === 0xFFFFFF) return DEFAULT_FG;
  if (n === 0x000000) return DEFAULT_BG;
  return '#' + n.toString(16).padStart(6, '0');
}

function renderPane(container, pane) {
  let canvas = container.querySelector('canvas');
  const needW = pane.cols * CELL_W, needH = pane.rows * CELL_H;
  if (!canvas || canvas.width !== needW || canvas.height !== needH) {
    if (canvas) container.removeChild(canvas);
    canvas = document.createElement('canvas');
    canvas.width = needW;
    canvas.height = needH;
    container.appendChild(canvas);
  }
  const ctx = canvas.getContext('2d');
  ctx.font = FONT;
  ctx.textBaseline = 'top';

  for (let row = 0; row < pane.rows; row++) {
    for (let col = 0; col < pane.cols; col++) {
      const cell = pane.cells[row * pane.cols + col];
      const x = col * CELL_W, y = row * CELL_H;
      ctx.fillStyle = toHex(cell.bg);
      ctx.fillRect(x, y, CELL_W, CELL_H);
      if (cell.ch !== ' ') {
        ctx.fillStyle = toHex(cell.fg);
        ctx.font = (cell.bold ? 'bold ' : '') + (cell.italic ? 'italic ' : '') + '13px monospace';
        ctx.fillText(cell.ch, x, y);
        if (cell.underline) {
          ctx.fillRect(x, y + CELL_H - 2, CELL_W, 1);
        }
      }
    }
  }

  const cx = pane.cursor_col * CELL_W, cy = pane.cursor_row * CELL_H;
  ctx.fillStyle = 'rgba(200,200,200,0.5)';
  ctx.fillRect(cx, cy, CELL_W, CELL_H);
}

function renderFrame(frame) {
  const panesDiv = document.getElementById('panes');
  const seen = new Set();

  for (const pane of frame.panes) {
    seen.add(pane.pane_id);
    let container = panesDiv.querySelector('[data-pane-id="' + pane.pane_id + '"]');
    if (!container) {
      container = document.createElement('div');
      container.className = 'pane';
      container.dataset.paneId = pane.pane_id;
      const title = document.createElement('div');
      title.className = 'pane-title';
      container.appendChild(title);
      panesDiv.appendChild(container);
    }
    container.className = 'pane' + (pane.focused ? ' focused' : '');
    container.querySelector('.pane-title').textContent = pane.title + ' [' + pane.pane_id + ']';
    renderPane(container, pane);
  }

  for (const el of panesDiv.querySelectorAll('[data-pane-id]')) {
    if (!seen.has(el.dataset.paneId)) panesDiv.removeChild(el);
  }
}

let ws = null, reconnectDelay = 1000;
function connect() {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  ws = new WebSocket(proto + '://' + location.host + '/ws');
  ws.onopen = () => { document.getElementById('status').textContent = 'Connected'; reconnectDelay = 1000; };
  ws.onmessage = (e) => { try { renderFrame(JSON.parse(e.data)); } catch(err) { console.error(err); } };
  ws.onclose = () => {
    document.getElementById('status').textContent = 'Disconnected — reconnecting in ' + (reconnectDelay/1000) + 's';
    setTimeout(connect, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 30000);
  };
  ws.onerror = () => ws.close();
}

document.addEventListener('keydown', (e) => {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  const focused = document.querySelector('.pane.focused');
  if (!focused) return;
  const pane_id = focused.dataset.paneId;
  if (!pane_id) return;
  e.preventDefault();
  const text = e.key === 'Enter' ? '\n' : e.key.length === 1 ? e.key : '';
  if (text) ws.send(JSON.stringify({ pane_id, text }));
});

connect();
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_html_is_valid_structure() {
        assert!(VIEWER_HTML.contains("<!DOCTYPE html>"));
        assert!(VIEWER_HTML.contains("</html>"));
    }

    #[test]
    fn viewer_connects_to_ws_endpoint() {
        assert!(VIEWER_HTML.contains("/ws"));
    }

    #[test]
    fn viewer_uses_filltextnotinnerhtml_for_cells() {
        assert!(VIEWER_HTML.contains("fillText"));
        let innerhtml_count = VIEWER_HTML.matches("innerHTML").count();
        assert_eq!(innerhtml_count, 0, "found innerHTML — use fillText or textContent instead");
    }

    #[test]
    fn viewer_handles_reconnection() {
        assert!(VIEWER_HTML.contains("reconnect"));
    }
}
