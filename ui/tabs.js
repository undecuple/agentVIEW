'use strict';
// Pill tabs for the Settings page. Only changes which blocks are shown; no data or behaviour.
(() => {
  const panel=document.getElementById('settings');if(!panel)return;
  const groups=[['General',()=>[...document.querySelectorAll('#settings-form fieldset'),document.getElementById('settings-form')?.querySelector('.settings-save')]],
    ['Data',()=>[document.getElementById('bundle-box'),document.getElementById('backup-box')]],
    ['Notifications',()=>[document.getElementById('hook-box'),document.getElementById('hookcfg-box')]],
    ['About',()=>[panel.querySelector('.settings-about')]]];
  const bar=document.createElement('div');bar.className='pill-tabs';bar.setAttribute('role','tablist');bar.setAttribute('aria-label','Settings sections');
  const head=panel.querySelector('.panel-title');head.after(bar);
  function select(i){
    const form=document.getElementById('settings-form');
    groups.forEach(([,get],j)=>{for(const e of get())if(e)e.hidden=j!==i;});
    // The form wrapper itself stays visible only on the General tab.
    if(form)form.hidden=i!==0;
    [...bar.children].forEach((b,j)=>b.setAttribute('aria-selected',String(j===i)));
    panel.dataset.tab=groups[i][0].toLowerCase();
  }
  groups.forEach(([name],i)=>{const b=document.createElement('button');b.type='button';b.setAttribute('role','tab');b.dataset.settingsTab=name.toLowerCase();b.textContent=name;b.onclick=()=>select(i);bar.append(b);});
  select(0);
  window.AppSettingsTabs={select:name=>select(groups.findIndex(g=>g[0].toLowerCase()===name))};
})();
