'use strict';
(() => {
  const $=id=>document.getElementById(id), invoke=(cmd,args={})=>AppIPC.invoke(cmd,args);
  let sessions=[],selected=null,busy=false,polling=false,refreshing=false,inspected=false,mutation=0;
  const terminals=new Map(),tools=new Map();
  let removeFor=null,confirming=false,prefsInitialized=false,lastPreferred=null,rosterSig='',wakePoll=()=>{},sleeper=null;
  const providerName=value=>value==='codex'?'Codex':'Claude Code';
  const notice=text=>{$('live-notice').textContent=text;};
  const current=()=>sessions.find(s=>s.id===selected);
  const active=s=>s&&s.status==='running';
  // Opt-in resume after a restart: lists sessions that are interrupted, or stopped with a saved conversation, and resumes only the ones a person confirms, by exact saved ID. Nothing is resent.
  let resumeAll=false,armedFocus=false;
  // Konsole-style tabs: one tab per opened session. Closing a tab never stops its process. Open tabs and their order are a per-viewer
  // convenience kept in localStorage (session IDs only); without it the page still works and opens tabs for running sessions.
  const TABKEY='agentview.liveTabs',okId=v=>typeof v==='string'&&/^[0-9a-f-]{36}$/i.test(v);
  let tabs=[],tabsLoaded=false;
  const savedTabs=(()=>{try{const v=JSON.parse(localStorage.getItem(TABKEY)||'null');return Array.isArray(v)?v.filter(okId).slice(0,64):null;}catch{return null;}})();
  function saveTabs(){try{localStorage.setItem(TABKEY,JSON.stringify(tabs));}catch{}}
  function openTab(id){if(okId(id)&&!tabs.includes(id)){tabs.push(id);saveTabs();}}
  function closeTab(id){
    const i=tabs.indexOf(id);if(i<0)return;const s=sessions.find(x=>x.id===id);
    const msg=s&&active(s)?`Tab closed. ${s.display_name||s.label} is still running; open it again from the list or the + menu.`:'';
    tabs.splice(i,1);saveTabs();
    if(selected===id){const next=tabs[Math.min(i,tabs.length-1)];if(next)select(next);else{selected=null;render();}}else render();
    notice(msg);
  }
  function moveTab(id,d){const i=tabs.indexOf(id),j=i+d;if(i<0||j<0||j>=tabs.length)return;[tabs[i],tabs[j]]=[tabs[j],tabs[i]];saveTabs();render();}
  function renderTabs(){
    const strip=$('live-tabs'),add=$('live-tab-add');strip.replaceChildren();
    if(tabsLoaded){const before=tabs.length;tabs=tabs.filter(id=>sessions.some(s=>s.id===id));if(tabs.length!==before)saveTabs();}
    if(selected&&!tabs.includes(selected))openTab(selected);
    tabs.forEach((id,i)=>{
      const s=sessions.find(x=>x.id===id);if(!s)return;const name=s.display_name||s.label,run=active(s);
      const tab=document.createElement('div');tab.className='live-tab';tab.setAttribute('role','tab');tab.dataset.tab=id;
      tab.setAttribute('aria-selected',String(id===selected));tab.setAttribute('aria-controls','live-terminal');tab.tabIndex=id===selected?0:-1;
      tab.title=`${name} · ${s.status}${i<9?` · Alt+${i+1}`:''}`;
      const dot=document.createElement('span');dot.className='dot '+String(s.status).replace(/[^a-z-]/g,'');dot.setAttribute('aria-hidden','true');
      const label=document.createElement('span');label.className='live-tab-name';label.textContent=name;
      const x=document.createElement('button');x.type='button';x.className='live-tab-close';x.tabIndex=-1;x.textContent='×';
      x.setAttribute('aria-label',run?`Close tab ${name} (the session keeps running)`:`Close tab ${name}`);
      x.title=run?'Close tab. The session keeps running; Stop process ends it.':'Close tab';
      x.onclick=e=>{e.stopPropagation();closeTab(id);};
      tab.onclick=()=>select(id);
      tab.onmousedown=e=>{if(e.button===1)e.preventDefault();};
      tab.onauxclick=e=>{if(e.button===1){e.preventDefault();closeTab(id);}};
      tab.onkeydown=e=>{
        const k=tabs.indexOf(id);let j=-1;
        if(e.key==='ArrowRight')j=(k+1)%tabs.length;else if(e.key==='ArrowLeft')j=(k-1+tabs.length)%tabs.length;else if(e.key==='Home')j=0;else if(e.key==='End')j=tabs.length-1;
        else if(e.key==='Delete'){e.preventDefault();closeTab(id);$('live-tabs').querySelector('[aria-selected=true]')?.focus();return;}
        else if(e.key==='Enter'||e.key===' '){e.preventDefault();select(id);return;}
        if(j>=0){e.preventDefault();select(tabs[j],true);}
      };
      tab.append(dot,label,x);strip.append(tab);
    });
    const closed=sessions.filter(s=>!tabs.includes(s.id));add.replaceChildren();
    const head=document.createElement('option');head.value='';head.textContent='+';add.append(head);
    for(const s of closed){const o=document.createElement('option');o.value=s.id;o.textContent=`${s.display_name||s.label} · ${s.status}`;add.append(o);}
    add.value='';add.disabled=!closed.length;add.title=closed.length?'Open a session in a new tab':'Every registered session has a tab';
  }
  function renderResume(){
    const box=$('live-resume'),list=sessions.filter(s=>s.status==='interrupted'||((s.status==='stopped'||s.status==='exited')&&s.provider_session_id));
    box.hidden=!list.length;if(!list.length){resumeAll=false;return;}
    const head=document.createElement('p');head.textContent=`${list.length} saved session${list.length===1?' is':'s are'} not running. You can pick up where ${list.length===1?'it':'each'} left off. Nothing was restarted or resent. Each one resumes its saved conversation in its own folder, with your usual permission mode (${[...new Set(list.map(s=>s.provider==='claude'?'claude --resume':'codex resume'))].join(' and ')}).`;
    const row=document.createElement('div');row.className='inline-form';
    for(const s of list){const b=document.createElement('button');b.type='button';b.dataset.resume=s.id;const why=!tools.has(s.provider)?`Inspect ${providerName(s.provider)} first (Provider & project setup)`:((s.attempted||s.bootstrap_attempted)&&!s.provider_session_id)?'Saved session ID is missing':'';b.disabled=busy||!!why;b.title=why;b.textContent=`Resume ${s.label||s.id}`;b.onclick=()=>{selected=s.id;render();$('live-start').click();};row.append(b);}
    const all=document.createElement('button');all.type='button';all.id='live-resume-all';all.disabled=busy||!list.length;all.textContent=resumeAll?`Confirm: resume all ${list.length}`:`Resume all (${list.length})`;
    all.onclick=async()=>{
      if(!resumeAll){resumeAll=true;armedFocus=true;renderResume();return;}
      resumeAll=false;if(busy)return;busy=true;mutation++;render();let ok=0,fail=0;
      for(const s of list){try{const started=await invoke('live_start',{id:s.id});sessions=sessions.map(x=>x.id===s.id?started:x);ok++;}catch{fail++;}}
      busy=false;mutation++;rosterSig='';notice(`Resumed ${ok} session${ok===1?'':'s'}${fail?`, ${fail} could not start (open it to see why)`:''}. No prompt was sent automatically.`);await refresh();render();
    };
    row.append(all);box.replaceChildren(head,row);
    if(armedFocus){armedFocus=false;all.focus();}
  }
  function render(){
    renderResume();renderTabs();
    const roster=$('live-roster');roster.replaceChildren();
    for(const s of sessions.filter(s=>!$('live-filter').value||s.provider===$('live-filter').value)){
      const button=document.createElement('button');button.type='button';button.className='live-session';button.dataset.session=s.id;button.setAttribute('aria-pressed',String(selected===s.id));
      const dot=document.createElement('span');dot.className='dot '+String(s.status).replace(/[^a-z-]/g,'');dot.setAttribute('aria-hidden','true');
      const name=document.createElement('strong');name.append(dot,document.createTextNode(s.display_name||s.label));
      const state=document.createElement('small');const chip=document.createElement('span');chip.className='chip';chip.textContent=providerName(s.provider);state.append(chip,document.createTextNode(` ${s.status}`));
      button.append(name,state);button.onclick=()=>select(s.id);roster.append(button);
    }
    const s=current();
    $('live-name').textContent=s?(s.display_name||s.label):'Select a registered project';
    $('live-project').textContent=s?s.project_dir:'';$('live-project').title=s?s.project_dir:'';
    $('live-identity').textContent=s?`${s.project_dir} · App ${s.id} · Provider ${s.provider_session_id||'not established'} · ${s.version} · ${s.executable}`:'Session IDs and launch recipes are saved. Terminal content is kept in memory only.';
    $('live-start').textContent=s&&!tools.has(s.provider)?`Inspect ${providerName(s.provider)} first`:s&&(s.attempted||s.bootstrap_attempted)&&!s.provider_session_id?'Identity needs review':s&&s.attempted?'Resume saved ID':'Start new session';
    $('live-start').disabled=busy||!s||active(s)||!tools.has(s.provider)||((s.attempted||s.bootstrap_attempted)&&!s.provider_session_id);$('live-stop').disabled=busy||!active(s);
    $('live-hint').textContent=!s?'Select or register a project to start a session.':active(s)?'Running. Use Stop process to end it.':!tools.has(s.provider)?`Press "Inspect ${providerName(s.provider)} CLI" in Provider & project setup first.`:((s.attempted||s.bootstrap_attempted)&&!s.provider_session_id)?'This session needs its saved identity reviewed before it can resume.':'';
    $('live-state').textContent=s?s.status:'Not started';$('live-state').dataset.state=s?s.status:'';
    {const b=$('live-banner');let m='';
      if(s&&s.status==='exited')m=`This session ended${s.exit_info?' ('+s.exit_info+')':''}${s.exit_at?' at '+new Date(s.exit_at).toLocaleTimeString():''}. Nothing was resent. Start it to resume the saved conversation.`;
      else if(s&&s.status==='interrupted')m='This session was running when the app last closed. Nothing was resent. Start it to resume.';
      else if(s&&s.status==='bootstrap_unknown')m='Setup of this session was interrupted. Check it in the provider before starting.';
      b.textContent=m;b.hidden=!m;b.dataset.bad=String(!!s&&s.status==='exited'&&!!s.exit_info&&s.exit_info!=='exit 0');}
    renderExtra(s);
    $('live-quick-target').textContent=s?`Paste target: ${providerName(s.provider)} · ${s.label}`:'Select a running coordinator';
    document.querySelectorAll('[data-quick],#live-paste-custom').forEach(button=>button.disabled=busy||!active(s));
    for(const [key,t] of terminals)t.host.hidden=!s||key!==`${s.id}:${s.generation}`;
    if(s){const t=terminal(s);t.host.hidden=false;fit(t,s);}
  }
  function renderExtra(s){
    const stopped=!!s&&!active(s)&&!busy,claude=!!s&&s.provider==='claude';
    $('live-extra').hidden=!s;
    if(removeFor!==(s?s.id:null)){removeFor=s?s.id:null;removeUi(false);}
    if(document.activeElement!==$('live-rename'))$('live-rename').value=s?s.label:'';
    $('live-rename').disabled=!stopped;$('live-rename-save').disabled=!stopped;
    $('live-remove').disabled=!stopped;
    for(const id of ['term-copy-sel','term-copy-last','term-copy-all'])$(id).disabled=!s;$('term-paste').disabled=!s||!active(s);
    $('live-remote').hidden=!claude;
    if(!claude)return;
    const on=!!s.remote_control;
    $('live-remote-state').textContent=on?(active(s)?'On for this run (started with it)':'On at next start'):('Off');
    if(!confirming){
      $('live-remote-enable').hidden=on;$('live-remote-disable').hidden=!on;$('live-remote-confirm').hidden=true;$('live-remote-cancel').hidden=true;
    }
    $('live-remote-enable').disabled=!stopped;$('live-remote-disable').disabled=!stopped;
  }
  function fit(t,s){
    if($('live').hidden||t.host.hidden)return;
    try{const geometry=`${t.host.clientWidth}:${t.host.clientHeight}`,changed=t.geometry!==geometry;t.geometry=geometry;
      t.fit.fit();if(changed&&document.activeElement===t.term.textarea)requestAnimationFrame(()=>t.host.scrollIntoView({block:'nearest',inline:'nearest'}));if(active(s))invoke('live_resize',{id:s.id,generation:s.generation,cols:t.term.cols,rows:t.term.rows}).catch(e=>notice(String(e)));}catch(e){notice(`Terminal resize failed: ${String(e)}`);}
  }
  function terminal(s){
    const key=`${s.id}:${s.generation}`;if(terminals.has(key))return terminals.get(key);
    // Discard prior launch scrollback when this exact session gets a new generation.
    for(const [old,t] of terminals){if(t.id===s.id){t.term.dispose();t.host.remove();terminals.delete(old);}}
    const host=document.createElement('div');host.className='terminal-instance';$('live-terminal').append(host);
    const p=AppPrefs.current;
    const term=new Terminal({cursorBlink:p?.terminal_cursor_blink??true,scrollback:p?.terminal_scrollback??2000,fontSize:p?.terminal_font_size??13,theme:terminalTheme(),allowProposedApi:false,linkHandler:{activate:()=>notice('Copy this URL into your browser to open it.')}});
    const fit=new FitAddon.FitAddon();term.loadAddon(fit);term.open(host);
    // No link addon, clipboard integration, title handler or HTML interpretation.
    term.parser.registerOscHandler(52,()=>true);
    // Copy and paste are explicit keys or buttons. The terminal itself never writes to the clipboard on its own (OSC 52 stays blocked above).
    term.attachCustomKeyEventHandler(ev=>{
      if(ev.type==='keydown'&&ev.ctrlKey&&ev.shiftKey&&!ev.altKey&&!ev.metaKey){
        if(ev.code==='KeyC'){ev.preventDefault();copySelection();return false;}
        if(ev.code==='KeyV'){ev.preventDefault();pasteFromClipboard();return false;}
      }
      return true;
    });
    term.onSelectionChange(()=>{if($('term-copy-on-select').checked&&term.hasSelection())copyText(term.getSelection()).then(ok=>termNote(ok?'Copied the selection.':'Could not copy; press Ctrl+Shift+C.'));});
    term.parser.registerOscHandler(8,()=>true);
    const t={id:s.id,generation:s.generation,term,fit,host,offset:0,input:Promise.resolve(),pending:0};terminals.set(key,t);
    term.onData(data=>{
      if(!active(current())||selected!==t.id||current().generation!==t.generation)return;
      if(new TextEncoder().encode(data).length>16384||t.pending>=64){notice('Input too large or pending. Paste smaller chunks.');return;}
      t.pending++;t.quiet=0;wakePoll();
      t.input=t.input.then(async()=>{
        const now=sessions.find(item=>item.id===t.id);
        if(!now||now.generation!==t.generation||!active(now)){notice('Pending input cancelled because its session stopped or changed. It was not replayed.');return;}
        try{const count=await invoke('live_write',{id:t.id,generation:t.generation,data});if(count!==new TextEncoder().encode(data).length)notice('Only part of the input was accepted. Delivery is uncertain; inspect the original session before typing again.');}
        catch(e){notice(`Input not confirmed; it will not be retried: ${String(e)}`);}
      }).finally(()=>t.pending--);
    });
    return t;
  }
  function select(id,focusTab){selected=id;openTab(id);notice('');render();const s=current();if(s){const t=terminal(s);$('live-tabs').querySelector('[aria-selected=true]')?.scrollIntoView({block:'nearest',inline:'nearest'});if(focusTab)$('live-tabs').querySelector('[aria-selected=true]')?.focus();else{t.term.focus();t.host.scrollIntoView({block:'nearest',inline:'nearest'});}}}
  async function refresh(){if(refreshing)return;refreshing=true;const epoch=mutation;try{const next=await invoke('live_snapshot');if(epoch!==mutation)return;const sig=JSON.stringify(next);const same=sig===rosterSig;rosterSig=sig;sessions=next;if(!tabsLoaded){tabsLoaded=true;tabs=(savedTabs??sessions.filter(active).map(s=>s.id)).filter(id=>sessions.some(s=>s.id===id));if(!selected)selected=tabs[0]??sessions[0]?.id??null;render();}else if(!same)render();}catch(e){notice(String(e));}finally{refreshing=false;}}
  function terminalTheme(){
    const css=getComputedStyle(document.documentElement),color=name=>css.getPropertyValue(name).trim();
    return {background:color('--bg'),foreground:color('--text'),cursor:color('--accent'),black:color('--surface'),red:color('--red'),green:color('--green'),yellow:color('--yellow'),blue:color('--accent'),magenta:color('--magenta'),cyan:color('--cyan'),white:color('--text'),brightBlack:color('--muted'),brightRed:color('--red'),brightGreen:color('--green'),brightYellow:color('--yellow'),brightBlue:color('--accent'),brightMagenta:color('--purple'),brightCyan:color('--cyan'),brightWhite:color('--text')};
  }
  function applyTerminalPrefs(){
    const p=AppPrefs.current;if(!p)return;
    for(const t of terminals.values()){t.term.options.fontSize=p.terminal_font_size;t.term.options.scrollback=p.terminal_scrollback;t.term.options.cursorBlink=p.terminal_cursor_blink;t.term.options.theme=terminalTheme();}
    requestAnimationFrame(()=>{const s=current();if(s)fit(terminal(s),s);});
  }
  function applyPreferences(p){
    if(lastPreferred!==p.preferred_provider){lastPreferred=p.preferred_provider;$('live-provider').value=p.preferred_provider;showTool();}
    if(!prefsInitialized){$('live-filter').value=p.remember_filters?p.live_provider_filter:'';prefsInitialized=true;}
    $('quick-buttons').replaceChildren();
    for(const preset of p.quick_prompts){const b=document.createElement('button');b.type='button';b.dataset.quick=preset.id;b.textContent=preset.label;$('quick-buttons').append(b);}
    applyTerminalPrefs();render();
  }
  window.addEventListener('agentview:preferences',e=>applyPreferences(e.detail));
  window.addEventListener('agentview:theme',applyTerminalPrefs);
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='sessions'){inspected=true;refresh();render();}});
  function showTool(){
    const provider=$('live-provider').value,tool=tools.get(provider);
    $('live-probe').textContent=`Inspect ${providerName(provider)} CLI`;
    $('live-tool').textContent=tool?`${providerName(provider)} · ${tool.executable} · ${tool.version}. Review this tool and project before starting.`:`${providerName(provider)} has not been inspected. No process starts automatically.`;
    $('live-register').disabled=!tool;render();
  }
  $('live-provider').onchange=showTool;
  $('live-filter').onchange=()=>{render();AppPrefs.remember({live_provider_filter:$('live-filter').value});};
  $('live-probe').onclick=async()=>{const provider=$('live-provider').value;try{const tool=await invoke('live_probe',{provider});tools.set(provider,tool);showTool();inspected=true;await refresh();}catch(e){notice(String(e));}};
  $('live-create').onsubmit=async event=>{event.preventDefault();if(busy)return;busy=true;mutation++;try{const s=await invoke('live_create',{provider:$('live-provider').value,label:$('live-label').value,projectDir:$('live-directory').value});selected=s.id;await refresh();notice('Project registered. Start explicitly after reviewing the directory and CLI above.');}catch(e){notice(String(e));}finally{busy=false;render();}};
  // Changes in the selected session's repository: files changed now and commits since the app started or resumed the session. Read-only git on the Rust side.
  let chgTimer=null,chgFor=null;
  async function loadChanges(){
    const s=current(),box=$('live-changes-body'),note=$('live-changes-note');
    if(!$('live-changes').open)return;
    if(!s){box.replaceChildren();note.textContent='Select a session.';return;}
    try{
      const r=await invoke('session_changes',{id:s.id});if(current()?.id!==s.id)return;
      const frag=document.createDocumentFragment();
      const h=document.createElement('p');h.textContent=`${r.repo} · branch ${r.branch||'unknown'} · ${r.total} changed file${r.total===1?'':'s'}${r.hidden_by_policy?` (${r.hidden_by_policy} hidden by the deny rules)`:''}`;frag.append(h);
      if(r.files.length){const ul=document.createElement('ul');for(const f of r.files.slice(0,100)){const li=document.createElement('li');li.textContent=`${f.state}  ${f.path}`;ul.append(li);}frag.append(ul);}
      const c=document.createElement('p');c.textContent=r.since_s!=null?`Commits since this session was started in this app run (${Math.max(1,Math.round(r.since_s/60))} min ago): ${r.commits.length}`:`Start time unknown after a restart; the latest ${r.commits.length} commits:`;frag.append(c);
      if(r.commits.length){const ul=document.createElement('ul');for(const k of r.commits){const li=document.createElement('li');li.textContent=`${k.id}  ${k.date}  ${k.subject}`;ul.append(li);}frag.append(ul);}
      box.replaceChildren(frag);note.textContent=r.note;
    }catch(e){box.replaceChildren();note.textContent=String(e);}
  }
  $('live-changes').addEventListener('toggle',()=>{clearInterval(chgTimer);chgTimer=null;if($('live-changes').open){loadChanges();chgTimer=setInterval(()=>{if(!document.hidden&&!$('live-changes').closest('[hidden]'))loadChanges();},10000);}});
  $('live-changes-reload').onclick=loadChanges;
  new MutationObserver(()=>{if(current()?.id!==chgFor){chgFor=current()?.id;loadChanges();}}).observe($('live-roster'),{childList:true,subtree:true,attributes:true});
  $('live-start').onclick=async()=>{const s=current();if(!s||busy)return;busy=true;mutation++;render();notice(`Starting ${providerName(s.provider)}; waiting for native session setup. Closing cancels startup.`);try{const started=await invoke('live_start',{id:s.id});sessions=sessions.map(item=>item.id===s.id?started:item);$('live-setup').open=false;notice('Process launched. Follow native provider prompts; no prompt was sent automatically.');}catch(e){notice(`Launch failed: ${String(e)}`);await refresh();}finally{busy=false;render();if(current()?.id===s.id&&active(current())){const t=terminal(current());t.term.focus();t.host.scrollIntoView({block:'nearest',inline:'nearest'});}}};
  $('live-stop').onclick=async()=>{const s=current();if(!s||busy)return;busy=true;mutation++;render();try{await invoke('live_stop',{id:s.id,generation:s.generation});await refresh();notice('Process stopped. Resume will use this saved ID; unsaved terminal input is not replayed.');}catch(e){notice(String(e));}finally{busy=false;render();}};
  async function copyText(text){
    try{await navigator.clipboard.writeText(text);return true;}catch{}
    const t=document.createElement('textarea');t.value=text;t.setAttribute('readonly','');t.style.cssText='position:fixed;left:-9999px;top:0';document.body.append(t);t.select();
    let ok=false;try{ok=document.execCommand('copy');}catch{}t.remove();return ok;
  }
  let termNoteTimer=null;
  function termNote(text){const n=$('term-note');n.textContent=text;clearTimeout(termNoteTimer);termNoteTimer=setTimeout(()=>{n.textContent='';},4000);}
  // The text of the terminal buffer; a line the terminal wrapped is joined back into one line so copied commands and paths stay whole.
  function bufferText(term,lastLines){
    const b=term.buffer.active,n=b.length,start=lastLines?Math.max(0,n-lastLines):0,out=[];
    for(let i=start;i<n;i++){const l=b.getLine(i);if(!l)continue;const t=l.translateToString(true);if(l.isWrapped&&out.length)out[out.length-1]+=t;else out.push(t);}
    while(out.length&&!out[out.length-1].trim())out.pop();
    return out.join('\n');
  }
  async function copyWith(get,empty,done){
    const s=current();if(!s)return;const t=terminal(s);const text=get(t.term);
    if(!text){termNote(empty);return;}
    termNote(await copyText(text)?done:'Could not copy. Select the text and press Ctrl+Shift+C.');t.term.focus();
  }
  const copySelection=()=>copyWith(term=>term.hasSelection()?term.getSelection():'','Select some text in the terminal first.','Copied the selection.');
  async function pasteFromClipboard(){
    const s=current();if(!s||!active(s))return;const t=terminal(s);let text='';
    try{text=await navigator.clipboard.readText();}catch{termNote('The clipboard cannot be read here. Press Ctrl+V inside the terminal instead.');t.term.focus();return;}
    if(text)t.term.paste(text);t.term.focus();
  }
  $('term-copy-sel').onclick=copySelection;
  $('term-copy-last').onclick=()=>copyWith(term=>bufferText(term,100),'There is no output yet.','Copied the last 100 lines.');
  $('term-copy-all').onclick=()=>copyWith(term=>bufferText(term,0),'There is no output yet.','Copied all output kept in the terminal.');
  $('term-paste').onclick=pasteFromClipboard;
  try{$('term-copy-on-select').checked=localStorage.getItem('agentview.copyOnSelect')==='1';}catch{}
  $('term-copy-on-select').onchange=()=>{try{localStorage.setItem('agentview.copyOnSelect',$('term-copy-on-select').checked?'1':'0');}catch{}};
  function pastePrompt(text){
    const s=current();if(!active(s)||busy){notice('Select a running coordinator first.');return;}
    if(!text.trim()||text.length>4000||/[\u0000-\u001f\u007f-\u009f\u061c\u200b\u200e\u200f\u2060\u2028-\u202e\u2066-\u2069\ufeff]/.test(text)){notice('Use a single-line prompt without control characters.');return;}
    const t=terminal(s);t.term.focus();t.host.scrollIntoView({block:'nearest',inline:'nearest'});t.term.paste(text);
    notice(`Pasted into ${providerName(s.provider)} · ${s.label}. Review it in the native prompt; Enter was not added.`);
  }
  $('quick-buttons').onclick=e=>{const button=e.target.closest('[data-quick]');if(button){const p=AppPrefs.current?.quick_prompts.find(p=>p.id===button.dataset.quick);if(p)pastePrompt(p.prompt);}};
  $('live-paste-custom').onclick=()=>pastePrompt($('live-custom').value);
  const mutate=async(command,args,done)=>{if(busy)return;busy=true;mutation++;try{const next=await invoke(command,args);sessions=sessions.map(x=>x.id===next.id?next:x);rosterSig='';notice(done);}catch(e){notice(String(e));}finally{busy=false;confirming=false;render();}};
  $('live-rename-form').onsubmit=e=>{e.preventDefault();const s=current();if(s)mutate('live_rename',{id:s.id,label:$('live-rename').value},'Renamed. Claude Code shows the new name the next time the session starts.');};
  function removeUi(on){$('live-remove').hidden=on;$('live-remove-confirm').hidden=!on;$('live-remove-cancel').hidden=!on;}
  $('live-remove').onclick=()=>{const s=current();if(!s)return;$('live-remove-confirm').textContent=`Confirm: remove ${s.label||'this project'} from this app`;removeUi(true);};
  $('live-remove-cancel').onclick=()=>removeUi(false);
  $('live-remove-confirm').onclick=async()=>{
    const s=current();if(!s||busy)return;busy=true;mutation++;
    try{await invoke('live_remove',{id:s.id});for(const [key,t] of terminals){if(t.id===s.id){t.term.dispose();t.host.remove();terminals.delete(key);}}
      sessions=sessions.filter(x=>x.id!==s.id);selected=sessions.length?sessions[0].id:null;rosterSig='';notice(`Removed ${s.label||'the project'} from this app. Your project folder and its history were not touched.`);}
    catch(e){notice(String(e));}
    finally{busy=false;removeUi(false);render();}};
  $('live-remote-enable').onclick=()=>{confirming=true;$('live-remote-enable').hidden=true;$('live-remote-confirm').hidden=false;$('live-remote-cancel').hidden=false;};
  $('live-remote-cancel').onclick=()=>{confirming=false;render();};
  $('live-remote-confirm').onclick=()=>{const s=current();if(s)mutate('live_set_remote',{id:s.id,enabled:true},'Remote Control will be on the next time this session starts.');};
  $('live-remote-disable').onclick=()=>{const s=current();if(s)mutate('live_set_remote',{id:s.id,enabled:false},'Remote Control is off for the next start.');};
  new ResizeObserver(()=>{const s=current();if(s)fit(terminal(s),s);}).observe($('live-terminal'));
  setInterval(()=>{if(!document.hidden&&inspected&&!$('live').hidden&&!busy)refresh();},1500);
  // Adaptive terminal poll: 120 ms while output flows, backing off to 1 s when idle; paused while the window is hidden.
  async function pollTerminal(){
    const s=current();if(document.hidden||polling||!s||$('live').hidden||!s.generation)return 500;polling=true;const t=terminal(s),key=`${s.id}:${s.generation}`;
    try{const result=await invoke('live_read',{id:s.id,generation:s.generation,offset:t.offset});
      if(selected!==s.id||current()?.generation!==s.generation||terminals.get(key)!==t)return;
      if(result.truncated){t.term.reset();notice('Older terminal output was dropped from the bounded memory buffer.');}
      const bin=atob(result.data),bytes=new Uint8Array(bin.length);for(let i=0;i<bin.length;i++)bytes[i]=bin.charCodeAt(i);
      t.quiet=bytes.length?0:(t.quiet||0)+1;if(bytes.length)t.term.write(bytes);t.offset=result.next_offset;
      if(!result.running&&active(s))await refresh();
    }catch(e){notice(`Terminal unavailable: ${String(e)}`);}finally{polling=false;}
    const q=terminals.get(key)?.quiet||0;return q<5?120:q<20?300:1000;
  }
  // Tab keys (Live page only). Ctrl+PageUp/PageDown cycle, Ctrl+Shift+PageUp/PageDown move the active tab, Alt+1..9 jump. These combos are
  // taken before the terminal sees them; every other key reaches the terminal unchanged. Alt+1..9 jumps tabs only while the focus is in
  // the terminal or the tab strip; elsewhere it keeps opening sidebar pages.
  window.addEventListener('keydown',ev=>{
    if($('live').hidden||ev.metaKey||!tabs.length)return;
    const i=tabs.indexOf(selected);let handled=false;
    if(ev.ctrlKey&&!ev.altKey&&(ev.key==='PageUp'||ev.key==='PageDown')){
      const d=ev.key==='PageUp'?-1:1;handled=true;
      if(ev.shiftKey){if(i>=0)moveTab(selected,d);}else select(tabs[((i<0?0:i)+d+tabs.length)%tabs.length]);
    }else if(ev.altKey&&!ev.ctrlKey&&!ev.shiftKey&&/^Digit[1-9]$/.test(ev.code)&&ev.target.closest?.('.xterm,#live-terminal,#live-tabbar')){
      handled=true;const n=Number(ev.code.slice(5))-1;if(n<tabs.length)select(tabs[n]);
    }
    if(handled){ev.preventDefault();ev.stopPropagation();}
  },true);
  $('live-tab-add').onchange=()=>{const id=$('live-tab-add').value;if(okId(id))select(id);};
  document.addEventListener('visibilitychange',()=>{if(!document.hidden)wakePoll();});
  wakePoll=()=>{const s=current(),t=s&&terminals.get(`${s.id}:${s.generation}`);if(t)t.quiet=0;if(sleeper)sleeper();};
  (async function loop(){for(;;){const ms=(await pollTerminal().catch(()=>500))||120;await new Promise(r=>{sleeper=r;setTimeout(r,ms);});}})();
})();
