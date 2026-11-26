'use strict';
// Count badges in the sidebar and links on the Overview tiles. Counts come from commands the app already has; nothing here changes anything.
(() => {
  const invoke = (c, a) => AppIPC.invoke(c, a);
  const set = (name, n) => { if (window.AppNav) AppNav.setCount(name, n); };
  async function tick() {
    if (document.hidden) return;
    try { const s = await invoke('live_snapshot'); set('live', s.filter(x => x.status === 'running').length); } catch {}
    try { const n = await invoke('notes_list', { query: '' }); set('notes', n.length); } catch {}
  }
  async function docsCount() {
    try { const d = await invoke('docs_list'); set('docs', d.shared.filter(e => e.ok).length); } catch {}
  }
  window.addEventListener('agentview:navigate', e => { if (e.detail === 'docs') docsCount(); });
  document.addEventListener('click', e => {
    const t = e.target.closest('[data-go]'); if (t && window.AppNav) AppNav.navigate(t.dataset.go);
  });
  document.addEventListener('keydown', e => {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    const t = e.target.closest && e.target.closest('.stat-link[data-go]'); if (t && window.AppNav) { e.preventDefault(); AppNav.navigate(t.dataset.go); }
  });
  (AppPrefs.ready || Promise.resolve()).then(() => { tick(); docsCount(); setInterval(tick, 20000); });
})();
