'use strict';
// Documents page: folders and files shared with the app are managed here, then read here. Read-only: no command on this page writes to a shared file.
(() => {
  const $ = id => document.getElementById(id);
  const invoke = (c, a) => AppIPC.invoke(c, a);
  let data = null, checked = null, filesSig = '', busy = false;
  const say = t => { $('docs-notice').textContent = t; };
  const fail = (what, e) => say(window.friendly ? window.friendly(what, e) : `${what}. ${String(e)}`);

  function drawShared() {
    const ul = $('docs-shared'); ul.replaceChildren();
    for (const e of data.shared) {
      const li = document.createElement('li'); li.className = 'docs-entry';
      const main = document.createElement('div');
      const name = document.createElement('span'); name.className = 'mono'; name.textContent = e.short; main.append(name);
      const chip = document.createElement('span'); chip.className = 'chip'; chip.textContent = e.ok ? `${e.kind}${e.source === 'config file' ? ' · allowlist.toml' : ''}` : 'refused'; main.append(' ', chip);
      if (!e.ok && e.reason) { const r = document.createElement('p'); r.className = 'muted'; r.textContent = e.reason; main.append(r); }
      li.append(main);
      if (e.source === 'app') {
        const b = document.createElement('button'); b.type = 'button'; b.textContent = 'Stop sharing'; b.setAttribute('aria-label', `Stop sharing ${e.short}`);
        b.onclick = () => remove(e.path); li.append(b);
      }
      ul.append(li);
    }
    if (!data.shared.length) { const li = document.createElement('li'); li.className = 'muted'; li.textContent = 'Nothing shared yet.'; ul.append(li); }
  }

  function drawFiles() {
    const q = $('docs-filter').value.trim().toLowerCase();
    const ul = $('docs-files'); ul.replaceChildren();
    let shown = 0;
    for (const f of data.files) {
      if (q && !f.short.toLowerCase().includes(q)) continue;
      const li = document.createElement('li'); const b = document.createElement('button'); b.type = 'button'; b.className = 'docs-file';
      b.textContent = f.short; if (f.image) { b.disabled = true; b.title = 'Images are not shown on this page'; }
      else b.onclick = () => open(f.path);
      li.append(b); ul.append(li); shown++;
    }
    $('docs-empty').hidden = !!data.files.length || !!data.shared.some(e => e.ok);
    const more = $('docs-more'); more.hidden = !data.more && shown === data.files.length;
    more.textContent = data.more ? `Showing the first ${data.files.length} files. Share a smaller folder to see the rest.` : (q ? `${shown} of ${data.files.length} files match.` : '');
  }

  function draw() {
    const warn = [data.config_note && `allowlist.toml did not load: ${data.config_note}`, data.store_note].filter(Boolean).join(' ');
    $('docs-warning').hidden = !warn; $('docs-warning').textContent = warn;
    $('docs-status').textContent = `${data.shared.filter(e => e.ok).length} shared · ${data.files.length}${data.more ? '+' : ''} files`;
    $('docs-deny').textContent = data.deny.join('  ');
    drawShared(); drawFiles();
  }

  async function load() {
    try { data = await invoke('docs_list'); draw(); }
    catch (e) { say(window.friendly ? window.friendly('Could not load documents', e) : String(e)); }
  }

  async function open(path) {
    try {
      const d = await invoke('docs_read', { path });
      $('docs-view-name').textContent = d.name; $('docs-view-body').innerHTML = d.html;
      const note = $('docs-view-note'); note.hidden = !d.truncated; note.textContent = d.truncated ? 'Showing the first 256 KB only.' : '';
      $('docs-view').hidden = false; $('docs-view').scrollIntoView({ block: 'nearest' }); $('docs-view-close').focus();
    } catch (e) { say(window.friendly ? window.friendly('Could not open that file', e) : String(e)); }
  }

  async function remove(path) {
    if (busy) return; busy = true;
    try { await invoke('docs_remove', { path }); say('Stopped sharing. The files themselves were not touched.'); await load(); }
    catch (e) { fail('Could not stop sharing', e); } finally { busy = false; }
  }

  // Step one only looks: it resolves the typed path and shows where it really points. Nothing is saved until the second click.
  $('docs-add-form').onsubmit = async ev => {
    ev.preventDefault(); if (busy) return; busy = true; say(''); $('docs-confirm').hidden = true; checked = null;
    try {
      const c = await invoke('docs_check', { path: $('docs-path').value });
      if (c.already) { say(`${c.short} is already shared.`); return; }
      checked = { path: $('docs-path').value, real: c.real };
      $('docs-confirm-text').textContent = `Share the ${c.kind} ${c.real}? Everything inside it can be read on this page, except files whose names are blocked. Nothing is ever written.`;
      $('docs-confirm').hidden = false; $('docs-confirm-add').focus();
    } catch (e) { fail('Cannot share that', e); } finally { busy = false; }
  };
  $('docs-confirm-cancel').onclick = () => { checked = null; $('docs-confirm').hidden = true; $('docs-path').focus(); };
  $('docs-confirm-add').onclick = async () => {
    if (!checked || busy) return; busy = true;
    try {
      await invoke('docs_add', { path: checked.path, expect: checked.real });
      say(`Now sharing ${checked.real}.`); $('docs-path').value = ''; $('docs-confirm').hidden = true; checked = null; await load();
    } catch (e) { fail('Could not share it', e); } finally { busy = false; }
  };
  $('docs-view-close').onclick = () => { $('docs-view').hidden = true; $('docs-view-body').replaceChildren(); };
  $('docs-filter').oninput = () => { if (data) drawFiles(); };
  AppNav.onShow('docs', load);
})();
