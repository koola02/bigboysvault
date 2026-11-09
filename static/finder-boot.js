// Starts the card finder (Rust compiled to WebAssembly). Until it's ready, or if it can't run, the server's page works as normal.
const slot = document.getElementById('finder');

async function run() {
  try {
    const finder = await import('/static/wasm/finder.js?v=9');
    await finder.default({ module_or_path: '/static/wasm/finder_bg.wasm?v=9' });
    await finder.start('finder', '/cards.tsv');
    // The Search tab means "I want to type": put the cursor in the box.
    if (location.pathname === '/search' && !location.search) document.querySelector('form.search input')?.focus({ preventScroll: true });
  } catch (e) {
    console.debug('Card finder unavailable, using the server page', e);
    try { navigator.sendBeacon('/oops', `card finder: ${String(e?.stack ?? e?.message ?? e).slice(0, 220)} | ${navigator.userAgent.slice(0, 90)}`); } catch (_) {}
  }
}

if (slot && typeof WebAssembly === 'object' && !navigator.connection?.saveData) {
  if (document.prerendering) document.addEventListener('prerenderingchange', run, { once: true });
  else run();
}
