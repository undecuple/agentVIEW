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
    $('pref-add-prompt').disabled=blocked||(draft?.quick_prompts.length||0)>=24;$('pref-add-lib').disabled=blocked||(draft?.quick_prompts.length||0)>=24;
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
  // Ready-made prompts people can add to their own list (they become ordinary presets: editable, movable, removable). Plain single-line text, never auto-sent.
  const LIBRARY=[
    ['resume','Update my resume state','Update your resume-state notes now (the memory or project notes file your project rules name): current state, decisions made, open items and the next concrete step. Do not change anything else.'],
    ['handoff-codex','Handoff: Claude to Codex','Write a handoff for a Codex session that will continue this work: goal, current state, files changed, evidence and test results, open issues, and next actions. Put it where the project rules say, and name the file that Codex should read first (AGENTS.md).'],
    ['handoff-claude','Handoff: Codex to Claude','Write a handoff for a Claude Code session that will continue this work: goal, current state, files changed, evidence and test results, open issues, and next actions. Put it where the project rules say, and name the file that Claude should read first (CLAUDE.md).'],
    ['pickup','Pick up from a handoff','Read the handoff note for this project and the project rules, then tell me in five lines what you understand the state to be and what you will do first. Do not change anything until I confirm.'],
    ['blockers','List blockers and decisions','List what is blocking you and each decision you need from me, most important first, with your recommendation for each.'],
    ['tests','Run tests and report','Run the project tests that apply to your last change and report the exact results. If something fails, say which test and why, and do not hide failures.'],
    ['review-diff','Review my uncommitted changes','Review the uncommitted changes in this project for bugs, missing tests and anything that breaks the project rules. List findings by severity with file and line, and do not edit anything.'],
    ['since','What changed since last time','Summarize what changed in this project since the last time we spoke: files, commits, test results and anything surprising.'],
    ['plan','Plan before editing','Before editing anything, give me a short plan: the files you will touch, the order, how you will check it works, and what could go wrong. Wait for my approval.'],
    ['failure','Explain the last failure','Explain the last failure you saw: what was run, the exact error, your best explanation, how sure you are, and what you would try next.'],
    ['rules','Check against project rules','Check your last change against the project rules and conventions, and list any place where it does not follow them.'],
    ['pause','Pause and ask me first','Pause here. Do not make further changes. Ask me whatever you need to continue safely.'],
    ['pm-status','Short status for the project manager','Write a short status for the project manager: what is done and verified, what is not verified, what is blocked, and what you need from them. No jargon.'],
  ];
  for(const r of (window.AgentRoles||[]).filter(x=>x.kind==='role'))LIBRARY.push(['role-'+r.key,'Onboard: '+r.label,r.prompt.replaceAll('{RULES}','the project rules file (CLAUDE.md or AGENTS.md)')]);
  (()=>{const sel=$('pref-lib');if(!sel)return;for(const [k,label] of LIBRARY){const o=document.createElement('option');o.value=k;o.textContent=label;sel.append(o);}})();
  $('pref-add-prompt').onclick=()=>{if(!draft||draft.quick_prompts.length>=24)return;draft.quick_prompts.push({id:'p_'+crypto.randomUUID().replaceAll('-',''),label:'New preset',prompt:'Summarize the current project status.'});markDirty();renderPrompts();controls();};
  $('pref-add-lib').onclick=()=>{
    if(!draft||draft.quick_prompts.length>=24)return;
    const e=LIBRARY.find(x=>x[0]===$('pref-lib').value);if(!e)return;
    if(draft.quick_prompts.some(p=>p.prompt===e[2])){$('settings-revision').textContent=`"${e[1]}" is already in your list.`;return;}
    draft.quick_prompts.push({id:('lib_'+e[0].replaceAll('-','_')+'_'+Math.random().toString(36).slice(2,6)).slice(0,40),label:e[1],prompt:e[2]});markDirty();renderPrompts();controls();
  };
  $('pref-reset-prompts').onclick=()=>{draft.quick_prompts=clone(AppPrefs.snapshot.defaults.quick_prompts);markDirty();renderPrompts();controls();};
  async function loadAbout(){
    if(aboutBusy)return;aboutBusy=true;
    try{const about=await invoke('app_about');$('settings-about').replaceChildren();
      for(const [key,value]of [['App',`agentVIEW ${about.app_version}`],['Build',about.build],['Platform',about.platform],['Local storage',about.data_directory],['Document allowlist',about.allowlist_file],['Saved CLI versions',about.saved_cli_versions.map(([p,v])=>`${p}: ${v}`).join(' · ')||'Unavailable — register an inspected CLI under Sessions']])$('settings-about').append(node('dt','',key),node('dd','',value));
      $('settings-report').textContent=JSON.stringify(about.report,null,2);
    }catch(e){$('settings-export-status').textContent=`Diagnostics unavailable: ${String(e)}`;}finally{aboutBusy=false;}
  }
  $('settings-export').onclick=async()=>{const b=$('settings-export');b.disabled=true;try{const result=await invoke('app_diagnostics_export');$('settings-export-status').textContent=`Report saved locally: ${result.path}`;$('settings-report').textContent=JSON.stringify(result.report,null,2);}catch(e){$('settings-export-status').textContent=`Report could not be exported: ${String(e)}`;}finally{b.disabled=false;}};
  AppPrefs.ready.then(snapshot=>{fill(snapshot);if(!snapshot){controls();notice('Preferences are unavailable. Reload saved preferences to retry.');}});
})();
