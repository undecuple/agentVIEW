'use strict';
(() => {
  const $=id=>document.getElementById(id), invoke=(cmd,args={})=>AppIPC.invoke(cmd,args);
  let sessions=[],selected=null,busy=false,polling=false,refreshing=false,inspected=false,mutation=0;
  const terminals=new Map(),tools=new Map();
  let confirming=false,prefsInitialized=false,lastPreferred=null,rosterSig='',wakePoll=()=>{},sleeper=null;
  const providerName=value=>value==='codex'?'Codex':'Claude Code';
  const notice=text=>{$('live-notice').textContent=text;};
  const current=()=>sessions.find(s=>s.id===selected);
  const active=s=>s&&s.status==='running';
  function render(){
    const roster=$('live-roster');roster.replaceChildren();
    for(const s of sessions.filter(s=>!$('live-filter').value||s.provider===$('live-filter').value)){
      const button=document.createElement('button');button.type='button';button.className='live-session';button.dataset.session=s.id;button.setAttribute('aria-pressed',String(selected===s.id));
      const name=document.createElement('strong');name.textContent=s.display_name||s.label;
      const state=document.createElement('small');state.textContent=`${providerName(s.provider)} · Coordinator · ${s.status}`;
      button.append(name,state);button.onclick=()=>select(s.id);roster.append(button);
    }
    const s=current();
    $('live-name').textContent=s?(s.display_name||s.label):'Select a registered project';
    $('live-project').textContent=s?s.project_dir:'';$('live-project').title=s?s.project_dir:'';
    $('live-identity').textContent=s?`${s.project_dir} · App ${s.id} · Provider ${s.provider_session_id||'not established'} · ${s.version} · ${s.executable}`:'Session IDs and launch recipes are saved. Terminal content is kept in memory only.';
    $('live-start').textContent=s&&!tools.has(s.provider)?`Inspect ${providerName(s.provider)} first`:s&&(s.attempted||s.bootstrap_attempted)&&!s.provider_session_id?'Identity needs review':s&&s.attempted?'Resume saved ID':'Start new session';
    $('live-start').disabled=busy||!s||active(s)||!tools.has(s.provider)||((s.attempted||s.bootstrap_attempted)&&!s.provider_session_id);$('live-stop').disabled=busy||!active(s);
    $('live-state').textContent=s?s.status:'Not started';
    renderExtra(s);
    $('live-quick-target').textContent=s?`Paste target: ${providerName(s.provider)} · ${s.label}`:'Select a running coordinator';
    document.querySelectorAll('[data-quick],#live-paste-custom').forEach(button=>button.disabled=busy||!active(s));
    for(const [key,t] of terminals)t.host.hidden=!s||key!==`${s.id}:${s.generation}`;
    if(s){const t=terminal(s);t.host.hidden=false;fit(t,s);}
  }
  function renderExtra(s){
    const stopped=!!s&&!active(s)&&!busy,claude=!!s&&s.provider==='claude';
    $('live-extra').hidden=!s;
    if(document.activeElement!==$('live-rename'))$('live-rename').value=s?s.label:'';
    $('live-rename').disabled=!stopped;$('live-rename-save').disabled=!stopped;
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
  function select(id){selected=id;notice('');render();const s=current();if(s){const t=terminal(s);t.term.focus();t.host.scrollIntoView({block:'nearest',inline:'nearest'});}}
  async function refresh(){if(refreshing)return;refreshing=true;const epoch=mutation;try{const next=await invoke('live_snapshot');if(epoch!==mutation)return;const sig=JSON.stringify(next);const same=sig===rosterSig;rosterSig=sig;sessions=next;if(!selected&&sessions.length)selected=sessions[0].id;if(!same)render();}catch(e){notice(String(e));}finally{refreshing=false;}}
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
  $('live-start').onclick=async()=>{const s=current();if(!s||busy)return;busy=true;mutation++;render();notice(`Starting ${providerName(s.provider)}; waiting for native session setup. Closing cancels startup.`);try{const started=await invoke('live_start',{id:s.id});sessions=sessions.map(item=>item.id===s.id?started:item);$('live-setup').open=false;notice('Process launched. Follow native provider prompts; no prompt was sent automatically.');}catch(e){notice(`Launch failed: ${String(e)}`);await refresh();}finally{busy=false;render();if(current()?.id===s.id&&active(current())){const t=terminal(current());t.term.focus();t.host.scrollIntoView({block:'nearest',inline:'nearest'});}}};
  $('live-stop').onclick=async()=>{const s=current();if(!s||busy)return;busy=true;mutation++;render();try{await invoke('live_stop',{id:s.id,generation:s.generation});await refresh();notice('Process stopped. Resume will use this saved ID; unsaved terminal input is not replayed.');}catch(e){notice(String(e));}finally{busy=false;render();}};
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
  document.addEventListener('visibilitychange',()=>{if(!document.hidden)wakePoll();});
  wakePoll=()=>{const s=current(),t=s&&terminals.get(`${s.id}:${s.generation}`);if(t)t.quiet=0;if(sleeper)sleeper();};
  (async function loop(){for(;;){const ms=(await pollTerminal().catch(()=>500))||120;await new Promise(r=>{sleeper=r;setTimeout(r,ms);});}})();
})();
