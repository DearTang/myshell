// Connectivity probe: node fetch (ignores env proxy => direct) to api.github.com
try {
  const r = await fetch('https://api.github.com', { signal: AbortSignal.timeout(15000) });
  console.log('node直连 api.github.com:', r.status);
} catch (e) {
  console.log('node直连 api.github.com 失败:', e.message || e.cause?.message);
}
// Proxy path via curl is tested separately; here test uploads.github.com too
try {
  const r = await fetch('https://uploads.github.com', { signal: AbortSignal.timeout(15000) });
  console.log('node直连 uploads.github.com:', r.status);
} catch (e) {
  console.log('node直连 uploads.github.com 失败:', e.message || e.cause?.message);
}
