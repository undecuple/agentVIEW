'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(cmd,args={})=>AppIPC.invoke(cmd,args);
  const clone=value=>JSON.parse(JSON.stringify(value));
  const fields={theme:'pref-theme',ui_scale_percent:'pref-ui-scale',density:'pref-density',default_page:'pref-default-page',preferred_provider:'pref-provider',remember_filters:'pref-remember',demo_mode:'pref-demo',terminal_font_size:'pref-font-size',terminal_scrollback:'pref-scrollback',terminal_cursor_blink:'pref-cursor-blink',usage_provider:'pref-usage-provider',usage_period:'pref-usage-period',usage_refresh_seconds:'pref-refresh'};
  const numeric=new Set(['ui_scale_percent','terminal_font_size','terminal_scrollback','usage_refresh_seconds']);
  const checks=new Set(['remember_filters','demo_mode','terminal_cursor_blink']);
  const node=(tag,cls,text)=>{const e=document.createElement(tag);if(cls)e.className=cls;if(text!==undefined)e.textContent=text;return e;};
  const recovery={corrupt_json:'Saved preferences are damaged. Restore app defaults to recover; the original record will be kept.',invalid_settings:'Saved preferences contain unsupported values. Restore app defaults to recover; the original record will be kept.',newer_version:'Preferences belong to a newer version. They are read-only in this build.',oversized:'Saved preferences exceed the safe size limit. They are read-only; the original record is preserved.',invalid_metadata:'Preference metadata is invalid. It is read-only; the original record is preserved.'};
  let draft=null,baseRevision=0,dirty=false,saving=false,aboutBusy=false;
  function notice(text){$('settings-notice').textContent=text;}
  function controls(){
    const snapshot=AppPrefs.snapshot,blocked=saving||!snapshot?.writable||!!snapshot.recovery;
    for(const e of $('settings-form').querySelectorAll('input,select,textarea,button'))e.disabled=blocked;
    $('settings-save').disabled=blocked;$('settings-reload').disabled=saving;
    $('settings-reset').disabled=saving||!snapshot?.writable;
    $('pref-add-prompt').disabled=blocked||(draft?.quick_prompts.length||0)>=8;
    const list=draft?.quick_prompts||[];
    list.forEach((_,i)=>{const row=$('settings-prompts').children[i];if(row?.dataset.preset){row.querySelector('[data-move=up]').disabled=blocked||i===0;row.querySelector('[data-move=down]').disabled=blocked||i===list.length-1;}});
  }
  function markDirty(){dirty=true;notice('Unsaved preferences. Save to apply them.');}
  function renderPrompts(){
    const container=$('settings-prompts');container.replaceChildren();
    if(!draft?.quick_prompts.length){container.append(node('p','muted','No preset buttons configured.'));return;}
    draft.quick_prompts.forEach((preset,index)=>{
      const row=node('div','settings-preset');row.dataset.preset=preset.id;
      const inputs=node('div',''),label=node('label','','Button label'),name=node('input','');name.value=preset.label;name.maxLength=48;name.required=true;
      name.oninput=()=>{preset.label=name.value;markDirty();};label.append(name);
      const textLabel=node('label','','Prompt (single line)'),text=node('textarea','');text.value=preset.prompt;text.maxLength=4000;text.rows=2;text.required=true;
      text.oninput=()=>{preset.prompt=text.value;markDirty();};text.onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();notice('Presets use one line. Wrapped text is fine.');}};textLabel.append(text);inputs.append(label,textLabel);
      const actions=node('div','settings-preset-controls');
      for(const [key,title,delta] of [['up','Move up',-1],['down','Move down',1]]){const b=node('button','',delta<0?'↑':'↓');b.type='button';b.dataset.move=key;b.title=title;b.setAttribute('aria-label',`${title}: ${preset.label}`);b.onclick=()=>{const target=index+delta;if(target<0||target>=draft.quick_prompts.length)return;[draft.quick_prompts[index],draft.quick_prompts[target]]=[draft.quick_prompts[target],draft.quick_prompts[index]];markDirty();renderPrompts();controls();};actions.append(b);}
      const remove=node('button','','Remove');remove.type='button';remove.onclick=()=>{draft.quick_prompts.splice(index,1);markDirty();renderPrompts();controls();};actions.append(remove);
      row.append(inputs,actions);container.append(row);
    });
  }
  function fill(snapshot){
    if(!snapshot)return;draft=clone(snapshot.settings);baseRevision=snapshot.revision;dirty=false;
    for(const [field,id]of Object.entries(fields)){if(checks.has(field))$(id).checked=draft[field];else $(id).value=draft[field];}
    renderPrompts();controls();$('settings-revision').textContent=`Saved revision ${baseRevision}`;
    notice(snapshot.recovery?recovery[snapshot.recovery]||'Preferences need recovery.':'Preferences are saved locally.');
  }
  function collect(){
    const next=clone(draft);
    for(const [field,id]of Object.entries(fields))next[field]=checks.has(field)?$(id).checked:numeric.has(field)?Number($(id).value):$(id).value;
    if(next.remember_filters){next.hierarchy_provider=$('provider').value;next.hierarchy_project=$('project-filter').value;next.hierarchy_status=$('status-filter').value;next.hierarchy_view=$('project-view').getAttribute('aria-pressed')==='true'?'project':'tree';next.live_provider_filter=$('live-filter').value;}
    return next;
  }
  window.addEventListener('agentview:preferences',()=>{const snapshot=AppPrefs.snapshot;if(!dirty&&!saving)fill(snapshot);else{controls();$('settings-revision').textContent=`Editing revision ${baseRevision} · stored ${snapshot.revision}`;}});
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='settings'){if(!dirty)fill(AppPrefs.snapshot);loadAbout();}});
  $('settings-form').addEventListener('input',markDirty);
  $('settings-form').onsubmit=async e=>{
    e.preventDefault();if(saving||!draft||!AppPrefs.snapshot?.writable||AppPrefs.snapshot.recovery)return;
    const next=collect();saving=true;controls();notice('Saving preferences…');
    try{fill(await AppPrefs.save(next,baseRevision));notice('Preferences saved.');loadAbout();}
    catch(error){notice(`Preferences were not saved: ${String(error)} Your draft is preserved. Reload saved preferences before retrying if another change was saved.`);}
    finally{saving=false;controls();}
  };
  $('settings-reload').onclick=async()=>{if(saving)return;try{fill(await AppPrefs.reload());notice(AppPrefs.snapshot.recovery?recovery[AppPrefs.snapshot.recovery]:'Saved preferences reloaded.');}catch(e){notice(String(e));}};
  $('settings-reset').onclick=async()=>{if(saving||!AppPrefs.snapshot?.writable)return;saving=true;controls();try{fill(await AppPrefs.reset(baseRevision));notice('App defaults restored. Projects, conversations and sessions are retained.');loadAbout();}catch(e){notice(`Defaults were not restored: ${String(e)}`);}finally{saving=false;controls();}};
  $('pref-add-prompt').onclick=()=>{if(!draft||draft.quick_prompts.length>=8)return;draft.quick_prompts.push({id:'p_'+crypto.randomUUID().replaceAll('-',''),label:'New preset',prompt:'Summarize the current project status.'});markDirty();renderPrompts();controls();};
  $('pref-reset-prompts').onclick=()=>{draft.quick_prompts=clone(AppPrefs.snapshot.defaults.quick_prompts);markDirty();renderPrompts();controls();};
  async function loadAbout(){
    if(aboutBusy)return;aboutBusy=true;
    try{const about=await invoke('app_about');$('settings-about').replaceChildren();
      for(const [key,value]of [['App',`agentVIEW ${about.app_version}`],['Platform',about.platform],['Local storage',about.data_directory],['Document allowlist',about.allowlist_file],['Saved CLI versions',about.saved_cli_versions.map(([p,v])=>`${p}: ${v}`).join(' · ')||'Unavailable — register an inspected CLI under Sessions']])$('settings-about').append(node('dt','',key),node('dd','',value));
      $('settings-report').textContent=JSON.stringify(about.report,null,2);
    }catch(e){$('settings-export-status').textContent=`Diagnostics unavailable: ${String(e)}`;}finally{aboutBusy=false;}
  }
  $('settings-export').onclick=async()=>{const b=$('settings-export');b.disabled=true;try{const result=await invoke('app_diagnostics_export');$('settings-export-status').textContent=`Report saved locally: ${result.path}`;$('settings-report').textContent=JSON.stringify(result.report,null,2);}catch(e){$('settings-export-status').textContent=`Report could not be exported: ${String(e)}`;}finally{b.disabled=false;}};
  AppPrefs.ready.then(snapshot=>{fill(snapshot);if(!snapshot){controls();notice('Preferences are unavailable. Reload saved preferences to retry.');}});
})();
