'use strict';
// First run: one skippable page that asks what to do first. It is shown once, only when nothing exists yet (no session, no shared folder, no note).
(() => {
  const $ = id => document.getElementById(id);
  const KEY = 'agentview.welcomeSeen';
  const seen = () => { try { return localStorage.getItem(KEY) === '1'; } catch { return false; } };
  const mark = () => { try { localStorage.setItem(KEY, '1'); } catch {} };
  const go = page => { mark(); if (window.AppNav) AppNav.navigate(page); };
  function show() { if (window.AppNav) AppNav.navigate('welcome'); }
  async function firstRun() {
    if (seen()) return;
    try {
      const [s, d, n] = await Promise.all([AppIPC.invoke('live_snapshot'), AppIPC.invoke('docs_list'), AppIPC.invoke('notes_list', { query: '' })]);
      if (s.length || d.shared.length || n.length) { mark(); return; }
    } catch { return; }
    show();
  }
  for (const b of document.querySelectorAll('[data-welcome]')) b.onclick = () => go(b.dataset.welcome);
  $('welcome-skip').onclick = () => go('sessions');
  window.AppWelcome = { show, firstRun };
  (AppPrefs.ready || Promise.resolve()).then(firstRun);
})();
