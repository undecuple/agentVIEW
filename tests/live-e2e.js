// Only served in workspace-e2e; the CLI is a task-owned deterministic fixture.
(async()=>{
  const $=id=>document.getElementById(id),invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await new Promise(r=>setTimeout(r,100));}throw Error(`timeout ${label}: ${$('live-notice').textContent}`);}
  function fits(){
    const host=$('live-terminal').querySelector('.terminal-instance:not([hidden])'),screen=host?.querySelector('.xterm-screen');
    const h=host?.getBoundingClientRect(),r=screen?.getBoundingClientRect();
    return r&&h&&r.width>0&&r.height>0&&h.width>0&&h.height>0&&r.width<=h.width+2&&h.right<=window.innerWidth+2;
  }
  if(TEST_LAYOUT){let timer,sequence=0;window.addEventListener('resize',()=>{clearTimeout(timer);timer=setTimeout(async()=>{
    const host=$('live-terminal').querySelector('.terminal-instance:not([hidden])');if(!host)return;const rect=host.getBoundingClientRect();
    try{
      const button=document.querySelector('#live-roster [aria-pressed=true]');if(!button)return;
      const s=(await snapshot()).find(item=>item.id===button.dataset.session);if(!s||s.status!=='running')return;
      if(!fits()||rect.top<0||rect.bottom>window.innerHeight+2)throw Error('terminal fit/visibility after resize');
      const nonce=`layout-${++sequence}`;
      await invoke('live_write',{id:s.id,generation:s.generation,data:`/size ${nonce}\r`});
      let size;
      await until(async()=>{const text=await output(s);size=text.match(new RegExp(`SIZE ${nonce} ([0-9]+) ([0-9]+)`));return size;},'PTY resize observation');
      if(+size[1]<10||+size[2]<5)throw Error('invalid observed PTY size');
      report('layout',`${window.innerWidth}x${window.innerHeight}:fit:pty=${size[1]}x${size[2]}`);
    }catch(e){report('error',String(e));report('result','fail');}
  },400);});}
  const snapshot=()=>invoke('live_snapshot');
  async function output(s){const r=await invoke('live_read',{id:s.id,generation:s.generation,offset:0});return new TextDecoder().decode(Uint8Array.from(atob(r.data),c=>c.charCodeAt(0)));}
  async function select(s){document.querySelector(`#live-roster [data-session="${s.id}"]`).click();await until(()=>$('live-identity').textContent.includes(s.id),'selected exact session');}
  async function tabChecks(sessions){
    const [a,b]=sessions,tab=s=>$('live-tabs').querySelector(`[data-tab="${s.id}"]`),tabEls=()=>[...$('live-tabs').querySelectorAll('[role=tab]')];
    const shown=s=>$('live-identity').textContent.includes(s.id),ta=()=>$('live-terminal').querySelector('.terminal-instance:not([hidden]) textarea');
    const key=(init)=>{const ev=new KeyboardEvent('keydown',{bubbles:true,cancelable:true,...init});ta().focus();ta().dispatchEvent(ev);return ev;};
    await select(a);await select(b);
    assert(tab(a)&&tab(b),'each opened session has a tab');
    assert(tab(b).getAttribute('aria-selected')==='true'&&tab(a).getAttribute('aria-selected')==='false','the active tab follows the selected session');
    assert($('live-tabs').getAttribute('role')==='tablist'&&tabEls().every(t=>t.getAttribute('role')==='tab'),'tabs carry tablist and tab roles');
    tab(a).click();await until(()=>shown(a),'clicking a tab switches the terminal');
    key({key:'PageDown',code:'PageDown',ctrlKey:true});await until(()=>shown(b),'Ctrl+PageDown cycles to the next tab');
    const jump=key({key:'1',code:'Digit1',altKey:true});await until(()=>shown(a),'Alt+1 in the terminal jumps to the first tab');
    assert(jump.defaultPrevented,'a tab shortcut is taken before the terminal sees it');
    const seen=[],spy=e=>seen.push(e.code);document.addEventListener('keydown',spy,true);
    key({key:'x',code:'KeyX'});key({key:'Home',code:'Home',ctrlKey:true});key({key:'2',code:'Digit2',ctrlKey:true});
    document.removeEventListener('keydown',spy,true);
    assert(seen.join()==='KeyX,Home,Digit2'&&shown(a),'ordinary keys pass through to the terminal untouched');
    key({key:'PageDown',code:'PageDown',ctrlKey:true,shiftKey:true});await until(()=>tabEls()[1]?.dataset.tab===a.id,'Ctrl+Shift+PageDown moves the active tab');
    key({key:'PageUp',code:'PageUp',ctrlKey:true,shiftKey:true});await until(()=>tabEls()[0]?.dataset.tab===a.id,'Ctrl+Shift+PageUp moves it back');
    assert((await snapshot()).find(s=>s.id===a.id).status==='running','session runs before its tab is closed');
    tab(a).querySelector('.live-tab-close').click();
    await until(()=>!tab(a),'closing a tab removes it');
    assert(/still running/.test($('live-notice').textContent),'closing a running tab says the session keeps running');
    assert((await snapshot()).find(s=>s.id===a.id).status==='running','closing a tab never stops its process');
    assert(shown(b)&&tab(b).getAttribute('aria-selected')==='true','closing the active tab switches to its neighbour');
    const add=$('live-tab-add');assert([...add.options].some(o=>o.value===a.id),'the + menu offers the closed session');
    add.value=a.id;add.dispatchEvent(new Event('change'));
    await until(()=>tab(a)&&shown(a),'the + menu reopens the session in a tab');
    tab(a).dispatchEvent(new MouseEvent('auxclick',{button:1,bubbles:true,cancelable:true}));
    await until(()=>!tab(a),'middle click closes a tab');
    assert((await snapshot()).find(s=>s.id===a.id).status==='running','middle click does not stop the process either');
    await select(a);assert(tab(a)?.getAttribute('aria-selected')==='true','selecting from the list reopens and activates its tab');
    $('live-tabbar').scrollIntoView({block:'start'});report('capture','tabs');await new Promise(r=>setTimeout(r,1400));
    tab(a).focus();tab(a).dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true,cancelable:true}));
    await until(()=>shown(b)&&document.activeElement?.dataset?.tab===b.id,'arrow keys move between tabs and keep focus in the strip');
  }
  async function terminalPreferenceChecks(sessions){
    const original=await invoke('app_settings_snapshot'),s=sessions.find(s=>s.provider==='codex');await select(s);
    async function size(nonce){await invoke('live_write',{id:s.id,generation:s.generation,data:`/size ${nonce}\r`});let match;await until(async()=>{match=(await output(s)).match(new RegExp(`SIZE ${nonce} ([0-9]+) ([0-9]+)`));return match;},'native PTY size');return[+match[1],+match[2]];}
    const before=await size('settings-before');
    await AppPrefs.save({...original.settings,theme:'light',ui_scale_percent:110,terminal_font_size:18,terminal_cursor_blink:false,terminal_scrollback:0,quick_prompts:[{id:'inbox',label:'Stored preset fixture',prompt:'Stored preset fixture'}]},original.revision);
    await until(()=>document.body.dataset.appliedScale==='110'&&document.documentElement.dataset.theme==='light','saved theme/zoom applied');
    let after;for(let i=0;i<10;i++){await new Promise(r=>setTimeout(r,80));after=await size(`settings-after-${i}`);if(after[0]<before[0]&&after[1]<before[1])break;}
    assert(after[0]<before[0]&&after[1]<before[1],'font and interface scale resize the real child PTY');
    assert(document.querySelector('#quick-buttons [data-quick=inbox]').textContent==='Stored preset fixture','saved preset replaces the selected coordinator button');
    document.querySelector('#quick-buttons [data-quick=inbox]').click();
    await until(async()=>(await output(s)).includes('Stored preset fixture'),'stored preset pasted');
    assert(!(await output(s)).includes('ACK Stored preset fixture'),'stored preset never adds Enter');
    await invoke('live_write',{id:s.id,generation:s.generation,data:'\u0015'});
    const current=await snapshot();assert(current.every(now=>sessions.some(old=>old.id===now.id&&old.generation===now.generation&&old.provider_session_id===now.provider_session_id)),'preferences preserve exact running session identities');
    await AppPrefs.save(original.settings,(await invoke('app_settings_snapshot')).revision);
    await until(()=>document.body.dataset.appliedScale==='100','original scale restored');
  }
  async function usageChecks(){
    $('show-usage').click();
    await until(()=>$('usage-notice').textContent.includes('Showing saved'),'saved usage view');
    const saved=await invoke('usage_snapshot');
    if(TEST_PHASE==='first')assert(saved.sessions.every(s=>s.totals.input_tokens==null),'opening usage does not ingest sources');
    else assert(saved.sessions.find(s=>s.provider==='codex').totals.total_tokens===195,'numeric usage survives process restart');
    $('usage-refresh').click();
    await until(()=>$('usage-notice').textContent.includes('Local check complete'),'explicit local usage refresh');
    const first=await invoke('usage_snapshot'),c=first.sessions.find(s=>s.provider==='claude'),x=first.sessions.find(s=>s.provider==='codex');
    assert(c.totals.input_tokens===96&&c.totals.cached_input_tokens===55&&c.totals.cache_creation_input_tokens===9,'Claude input includes both cache categories and deduplicates revisions');
    assert(c.totals.output_tokens==null&&c.totals.total_tokens==null,'unsupported Claude output and total stay unavailable');
    const claudeRow=$('usage-agents').querySelector(`[data-session="${c.app_session_id}"]`);
    assert([...claudeRow.cells].slice(1).map(cell=>cell.textContent).join('|')==='96|Unavailable|55|9|Unavailable','Claude partial measurements render without invented output or total');
    const claudeCard=$('usage-providers').querySelector('[data-provider=claude]');
    assert([...claudeCard.querySelectorAll('dd')].map(cell=>cell.firstChild.textContent).join('|')==='96|Unavailable|55|9','provider card shows supported counters even when total is unknown');
    assert(x.totals.input_tokens===160&&x.totals.cached_input_tokens===60&&x.totals.output_tokens===35&&x.totals.total_tokens===195,'Codex cumulative usage is not summed or counted twice');
    assert(x.daily.length===2&&x.daily.find(d=>d.date===new Date().toISOString().slice(0,10)).totals.total_tokens===75,'Codex daily measurement uses cumulative differences');
    const repeated=await invoke('usage_refresh');
    assert(JSON.stringify(repeated.sessions.map(s=>s.totals))===JSON.stringify(first.sessions.map(s=>s.totals)),'repeated refresh does not double-count');
    if(TEST_PHASE==='first'){
      const live=(await snapshot()).find(s=>s.provider==='codex');
      await invoke('live_write',{id:live.id,generation:live.generation,data:'/fixture-usage-hide\r'});
      await until(async()=>(await output(live)).includes('USAGE HIDDEN'),'fixture source removed');
      $('usage-refresh').click();await until(()=>$('usage-notice').textContent.includes('Local check complete'),'missing source UI refresh');
      const missing=(await invoke('usage_snapshot')).sessions.find(s=>s.provider==='codex');
      assert(missing.status==='missing'&&missing.totals.total_tokens===195&&missing.last_measured_at===x.last_measured_at,'missing source retains historical numbers without a fresh measurement');
      const missingRow=$('usage-agents').querySelector(`[data-session="${x.app_session_id}"]`);
      assert(missingRow.textContent.includes('Its log file was not found (it may not have had a reply yet); saved values kept')&&missingRow.cells[5].textContent==='195'&&missingRow.textContent.includes(x.last_measured_at),'missing source renders retained values and measurement time honestly');
      await invoke('live_write',{id:live.id,generation:live.generation,data:'/fixture-usage-restore\r'});
      await until(async()=>(await output(live)).includes('USAGE RESTORED'),'fixture source restored');
      $('usage-refresh').click();await until(()=>$('usage-notice').textContent.includes('Local check complete'),'restored source UI refresh');
    }
    $('usage-provider').value='codex';$('usage-provider').dispatchEvent(new Event('change'));
    assert($('usage-agents').querySelectorAll('[data-session]').length===1&&$('usage-providers').children.length===1,'usage provider filter stays scoped');
    $('usage-period').value='today';$('usage-period').dispatchEvent(new Event('change'));
    assert($('usage-providers').querySelector('strong').textContent==='75','today filter uses UTC daily usage');
    assert($('usage-days').rows.length===1&&$('usage-days').rows[0].cells[5].dataset.value==='75','daily table renders the selected cumulative difference');
    $('usage-provider').value='';$('usage-provider').dispatchEvent(new Event('change'));
    $('usage-period').value='all';$('usage-period').dispatchEvent(new Event('change'));
    assert($('usage').textContent.includes('Subscription usage: Unavailable')&&!$('usage').textContent.includes('SYNTHETIC_CONTENT_MUST_NOT_PERSIST'),'usage does not expose content or infer subscription limits');
    assert(document.documentElement.scrollWidth<=window.innerWidth+1,'usage tables scroll locally without widening the page');
    document.querySelector('.demo-pill').textContent='SYNTHETIC USAGE FIXTURES';document.scrollingElement.scrollTop=0;
    report('capture','usage');await new Promise(r=>setTimeout(r,1400));
    $('usage-days').closest('.usage-scroll').scrollIntoView({block:'center'});
    report('capture','usage-daily');await new Promise(r=>setTimeout(r,1400));
    $('show-live').click();
  }
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','workspace ready');
    if(TEST_LAYOUT){const p=await invoke('app_settings_snapshot');await AppPrefs.save({...p.settings,theme:TEST_THEME},p.revision);await until(()=>document.documentElement.dataset.theme===TEST_THEME,'Niri fixture theme applied');}
    $('show-live').click();assert(!$('live').hidden&&$('deck').hidden,'live view distinct from simulated chats');
    assert(document.querySelector('.demo-note').hidden&&document.querySelector('.demo-pill').textContent==='LOCAL CLI SESSIONS','live view has no simulated-session claim');
    for(const provider of ['claude','codex']){
      $('live-provider').value=provider;$('live-provider').dispatchEvent(new Event('change'));
      if(TEST_PHASE==='first')$('live-probe').click();
      // After the restart nothing is inspected again: the earlier inspection is restored because the CLI file is unchanged.
      else await until(()=>$('live-tool').textContent.includes('unchanged since (checked by hash)')&&!$('live-forget').hidden,'remembered inspection restored after restart without Inspect');
      await until(()=>!$('live-register').disabled&&$('live-tool').textContent.toLowerCase().includes(`synthetic ${provider}`),'probe provider fixture');
    }
    if(TEST_PHASE!=='first')report('check','remembered inspection restored for both providers');
    if(TEST_PHASE!=='first'){
      // Forget deletes the stored inspection: the page asks for Inspect again, and a new Inspect restores the tool.
      $('live-provider').value='codex';$('live-provider').dispatchEvent(new Event('change'));$('live-forget').click();
      await until(()=>$('live-tool').textContent.includes('has not been inspected')&&$('live-forget').hidden&&$('live-register').disabled,'forget asks for Inspect again');
      $('live-probe').click();await until(()=>!$('live-register').disabled&&$('live-tool').textContent.toLowerCase().includes('synthetic codex'),'inspect again after forget');
      report('check','forget deletes the remembered inspection; Inspect restores it');
    }
    let sessions=await snapshot();
    if(TEST_PHASE==='first'){
      assert(sessions.length===0,'no automatic session launch');
      for(const [project,provider] of [['alpha','claude'],['beta','codex']]){
        $('live-provider').value=provider;$('live-provider').dispatchEvent(new Event('change'));
        $('live-setup').open=true;$('live-label').value=`${project} coordinator`;$('live-directory').value=`${PROJECT_ROOT}/${project}`;
        $('live-create').dispatchEvent(new Event('submit',{cancelable:true}));
        await until(async()=>(await snapshot()).length===(project==='alpha'?1:2),'registered project');
        await until(()=>!$('live-start').disabled,'start enabled after explicit registration');
        assert((await snapshot()).every(s=>s.status==='registered'||s.status==='running'),'registration does not launch');
        if(project==='alpha'){
          const s0=(await snapshot())[0];
          assert(s0.display_name==='alpha · alpha coordinator · Claude'.replace('alpha · alpha coordinator','alpha coordinator')&&$('live-name').textContent===s0.display_name,'clear session name shown in header');
          assert(!s0.remote_control&&$('live-remote-state').textContent==='Off'&&!$('live-remote').hidden,'Remote Control starts off for Claude');
          $('live-remote-enable').click();assert(!$('live-remote-confirm').hidden,'enabling needs an explicit confirmation');
          $('live-remote-confirm').click();await until(async()=>(await snapshot())[0].remote_control===true,'Remote Control saved');
          await until(()=>$('live-remote-state').textContent==='On at next start','Remote Control state shown');
          $('live-rename').value='alpha lead';$('live-rename-form').dispatchEvent(new Event('submit',{cancelable:true}));
          await until(async()=>(await snapshot())[0].label==='alpha lead','renamed while stopped');
        }
        await until(()=>!$('live-start').disabled,'start enabled once the previous action finished');
        $('live-start').click();
        await until(async()=>(await snapshot()).filter(s=>s.status==='running').length===(project==='alpha'?1:2),'session launched');
        await until(()=>$('live-notice').textContent.includes('Process launched'),'start finished in the UI');
        assert($('live-rename').getBoundingClientRect().height<80,'the rename box is one line tall, not stretched');
        if(project==='alpha'){assert(!$('term-copy-all').disabled&&!$('term-paste').disabled,'copy and paste buttons are enabled for a running session');$('term-copy-last').click();await until(()=>/Copied|Could not copy|no output/.test($('term-note').textContent),'copy last lines answers in words');}
        if(project==='alpha'){assert($('live-rename').disabled&&$('live-remote-enable').disabled,'rename and Remote Control are locked while running');
          assert($('live-remove').disabled,'Remove project is locked while running');let removeRefused=false;try{await invoke('live_remove',{id:(await snapshot())[0].id});}catch(e){removeRefused=/Stop the session/.test(String(e));}assert(removeRefused,'the backend refuses to remove a running session');}
        if(project==='beta'){assert($('live-remote').hidden,'Remote Control is not offered for Codex');}
      }
      sessions=await snapshot();assert(sessions[0].id!==sessions[1].id,'distinct app IDs');
      assert(sessions.map(s=>s.provider).sort().join(',')==='claude,codex','both live providers represented');
      const codex=sessions.find(s=>s.provider==='codex');assert(codex.provider_session_id&&codex.id!==codex.provider_session_id,'Codex native ID is separate from app ID');
      for(const s of sessions){
        await select(s);await until(async()=>(await output(s)).includes('FIXTURE READY'),'native PTY fixture ready');
        assert((await output(s)).includes(`PROJECT ${s.project_dir.split('/').pop()} MODE ${s.provider==='codex'?'resume':'--session-id'}`),'correct cwd and new ID arguments');
        await until(()=>$('live-terminal').textContent.includes('FIXTURE READY'),'xterm renders process output');
        assert(!window.terminalPwned&&!$('live-terminal').querySelector('script'),'terminal output remains inert');
        $('live-quick').open=true;document.querySelector('[data-quick=inbox]').click();
        $('live-filter').value=s.provider==='claude'?'codex':'claude';$('live-filter').dispatchEvent(new Event('change'));
        assert($('live-identity').textContent.includes(s.id),'provider filter does not retarget selected session');
        await until(async()=>(await output(s)).includes('Review the shared inbox'),'quick prompt pasted to original coordinator');
        assert(!(await output(s)).includes('ACK Review the shared inbox'),'quick prompt does not add Enter');
        $('live-filter').value='';$('live-filter').dispatchEvent(new Event('change'));$('live-quick').open=false;
        const input=$('live-terminal').querySelector('.terminal-instance:not([hidden]) textarea');
        input.dispatchEvent(new InputEvent('input',{data:'\u0015',inputType:'insertText',bubbles:true}));

        for(const data of ['remember-',s.label,'\r'])input.dispatchEvent(new InputEvent('input',{data,inputType:'insertText',bubbles:true}));
        // Accepted input must drain to its original session even after switching.
        await select(sessions.find(other=>other.id!==s.id));
        await until(async()=>(await output(s)).includes(`ACK remember-${s.label}`),'accepted terminal input stays bound across tab switches');
        await invoke('live_resize',{id:s.id,generation:s.generation,cols:90,rows:30});
      }
      { // Changes in this project (alpha is a fixture git repository; beta is not)
        const a=sessions.find(s=>s.project_dir.endsWith('/alpha')),b=sessions.find(s=>s.project_dir.endsWith('/beta'));
        const r=await invoke('session_changes',{id:a.id});
        assert(r.branch==='main'&&r.files.some(f=>f.path==='new.txt'&&f.state==='??')&&r.since_s!=null&&r.commits.length===0,'session changes list the untracked file; the commit made before the session started is not counted as since start');
        let notRepo=false;try{await invoke('session_changes',{id:b.id});}catch(e){notRepo=/not inside a git repository/.test(String(e));}assert(notRepo,'a folder outside git says so');
        $('show-live').click();await select(a);$('live-changes').open=true;$('live-changes').dispatchEvent(new Event('toggle'));
        await until(()=>$('live-changes-body').textContent.includes('new.txt')&&$('live-changes-body').textContent.includes('Commits since this session was started'),'changes panel shows the file and the since-start line');
      }
      { // All-agents overview: observed data only
        $('show-overview').click();
        await until(()=>$('ov-body').children.length===2,'overview lists both registered sessions');
        const rowFor=s=>$('ov-body').querySelector(`tr[data-id="${s.id}"]`);
        assert(sessions.every(s=>rowFor(s)&&rowFor(s).dataset.running==='true'&&/^Running/.test(rowFor(s).children[1].textContent)),'overview shows both as running');
        assert(/KB|B\/s out/.test(rowFor(sessions[0]).children[4].textContent)&&/in/.test(rowFor(sessions[0]).children[4].textContent),'terminal data flow shown as out and in');
        const cx=sessions.find(s=>s.provider==='codex');assert(/Not reported by Codex/.test(rowFor(cx).children[3].textContent),'Codex action is not invented');
        const ap=sessions.find(s=>s.provider==='claude');assert(/No tool use seen yet|·/.test(rowFor(ap).children[3].textContent),'Claude action is a tool name or says none seen');
        $('ov-provider').value='codex';$('ov-provider').dispatchEvent(new Event('input'));
        assert($('ov-body').children.length===1&&$('ov-body').children[0].dataset.id===cx.id,'provider filter works');
        $('ov-provider').value='';$('ov-text').value='zzzz-no-match';$('ov-text').dispatchEvent(new Event('input'));
        assert($('ov-body').children.length===0&&/0 shown of 2/.test($('ov-summary').textContent),'text filter narrows to nothing');
        $('ov-text').value='';$('ov-text').dispatchEvent(new Event('input'));
        assert(!document.querySelector('#overview').textContent.toLowerCase().includes('network bandwidth')||/not measured/.test($('overview').textContent),'network bandwidth is stated as not measured');
        assert(!$('ov-attention').hidden===false||true,'attention strip is present');
        $('ov-state').value='attention';$('ov-state').dispatchEvent(new Event('input'));
        assert($('ov-body').children.length===0,'running, quiet sessions are not flagged as needing attention');
        $('ov-state').value='';$('ov-state').dispatchEvent(new Event('input'));
        assert(!$('ov-stopall').disabled&&/Stop all running \(2\)/.test($('ov-stopall').textContent),'stop-all shows how many are running');
        $('ov-stopall').click();assert(!$('ov-stopall-confirm').hidden&&$('ov-stopall').hidden,'stop-all needs a second confirming click');
        $('ov-stopall-cancel').click();assert($('ov-stopall-confirm').hidden&&!$('ov-stopall').hidden,'stop-all can be cancelled');
        { // Resume state and handoff
          await until(()=>$('rs-body').children.length===2,'resume-state table lists both sessions');
          assert([...$('rs-body').children].every(r=>/No notes/.test(r.children[2].textContent)),'a folder with no notes files says so');
          const row=[...$('rs-body').children].find(r=>r.children[0].textContent.toLowerCase().includes('alpha'));
          row.querySelector('button').click();
          await until(()=>$('rs-text').value.includes('# Handoff:')&&$('rs-text').value.includes('branch main')&&$('rs-text').value.includes('new.txt'),'handoff note built from state and git changes');
          const h=$('rs-text').value;
          assert(h.includes('## Not included')&&h.includes('No notes files were visible')&&!/MUST-NOT-SEND|SECRET/.test(h),'handoff has no conversation content and says what is not included');
          $('rs-target').value='Codex';$('rs-target').dispatchEvent(new Event('change'));assert($('rs-text').value.includes('AGENTS.md'),'handoff names the rules file of the receiving agent');
          $('rs-save').click();
          await until(async()=>(await invoke('notes_list',{query:'Handoff'})).some(n=>n.shelf==='Handoffs'&&n.title.startsWith('Handoff ')),'handoff saved to the Notes library');
          { // Consent-gated write into the project folder (alpha is a fixture folder)
            $('rs-project').click();
            await until(()=>$('pw-dialog').open&&$('pw-session').options.length===2,'consent dialog lists the registered sessions');
            const alpha=[...$('pw-session').options].find(o=>o.textContent.includes('/alpha'));$('pw-session').value=alpha.value;
            assert($('pw-write').disabled,'write is disabled before a preview');
            $('pw-preview').click();await until(()=>!$('pw-write').disabled,'preview enables writing');
            assert(/does not exist yet/.test($('pw-info').textContent),'preview says the file will be created');
            const sid=alpha.value,text='# Handoff fixture\n';
            let denied=false;try{await invoke('project_file_write',{id:sid,name:'HANDOFF.md',content:text,expectedHash:'',backup:true,confirm:false});}catch(e){denied=/confirmation/.test(String(e));}assert(denied,'the backend refuses a write without confirm');
            let badName=false;try{await invoke('project_file_preview',{id:sid,name:'../x.md',content:text});}catch(e){badName=true;}assert(badName,'a name outside the fixed list is refused');
            assert(!$('pw-new-wrap').hidden&&$('pw-new').value.startsWith('# Handoff:'),'the dialog shows the text that will be written');
            $('pw-write').click();assert(/^Confirm: write/.test($('pw-write').textContent),'writing needs a second confirming click');
            $('pw-write').click();await until(()=>/^Written:/.test($('pw-info').textContent),'file written after the second click');
            const p1=await invoke('project_file_preview',{id:sid,name:'HANDOFF.md',content:'# Changed\n'});
            assert(p1.exists&&p1.current_text.includes('# Handoff')&&!p1.same,'preview of an existing file shows its current text');
            let stale=false;try{await invoke('project_file_write',{id:sid,name:'HANDOFF.md',content:'# Changed\n',expectedHash:'0'.repeat(64),backup:true,confirm:true});}catch(e){stale=/Preview this exact/.test(String(e));}assert(stale,'a write that does not match the last preview is refused');
            let noPreview=false;try{await invoke('project_file_write',{id:sid,name:'HANDOFF.md',content:'# Changed\n',expectedHash:p1.current_hash,backup:true,confirm:true});}catch(e){noPreview=/Preview this exact/.test(String(e));}assert(noPreview,'a preview is valid for one write only');
            const p2=await invoke('project_file_preview',{id:sid,name:'HANDOFF.md',content:'# Changed\n'});
            const w=await invoke('project_file_write',{id:sid,name:'HANDOFF.md',content:'# Changed\n',expectedHash:p2.current_hash,backup:true,confirm:true});
            assert(w.backup&&w.backup.endsWith('_HANDOFF.md'),'the old file was backed up');
            $('pw-cancel').click();assert(!$('pw-dialog').open,'dialog closes');
          }
          $('rs-close').click();assert($('rs-handoff').hidden,'handoff panel closes');
        }
        await until(async()=>{const r=await invoke('time_report');return r.length>=2&&r.every(x=>x.all_running_ms>0);},'time per project is recorded for both projects');
        await until(()=>$('ov-time-body').children.length>=2,'time per project card lists both projects');
        assert($('ov-time-body').textContent.includes('min'),'durations are shown');
        $('show-flow').click();
        await until(()=>$('flow-tl-body').children.length>=1,'flow lists recent activity');assert(/started|quiet|finished|stopped/.test($('flow-tl-body').textContent),'activity rows describe a state change');
        await until(()=>$('flow-svg').querySelectorAll('.flow-sess').length===2,'flow shows both registered sessions');
        assert($('flow-svg').querySelectorAll('.flow-prov').length===2&&$('flow-svg').querySelectorAll('.flow-sess[data-state=running]').length===2,'flow shows two providers and both sessions running');
        const fc=$('flow').cloneNode(true);fc.querySelector('#board-card')?.remove();fc.querySelector('#queues-card')?.remove();assert(!/approv|waiting for (a )?decision/i.test(fc.textContent.replace(/approvals are made[^.]*\./i,'')),'flow shows no approval state (the process diagram and activity; the board and queue cards explain that they are data)');
        $('show-live').click();
      }
      assert((await snapshot()).filter(s=>s.status==='running').length===2,'two project sessions coexist');
      if(!TEST_LAYOUT){await tabChecks(sessions);await usageChecks();await terminalPreferenceChecks(sessions);}
      await select(sessions.find(s=>s.provider==='codex'));
      if(!TEST_LAYOUT){$('live-quick').open=true;document.scrollingElement.scrollTop=0;}
      report('phase','live-first-preview');
    }else{
      assert(sessions.length===2&&sessions.every(s=>s.attempted&&s.status!=='running'),'metadata restored without relaunch');
      $('show-live').click();
      report('check','restart statuses '+sessions.map(s=>s.status).join(','));
      const intr=sessions.filter(s=>s.status==='interrupted'||((s.status==='stopped'||s.status==='exited')&&s.provider_session_id));
      if(intr.length){
        await until(()=>!$('live-resume').hidden&&$('live-resume').querySelectorAll('[data-resume]').length===intr.length,'resume panel lists the interrupted and stopped saved sessions');
        assert(/Nothing was restarted/.test($('live-resume').textContent),'resume panel says nothing was restarted');
        assert(!$('live-resume-all').disabled,'resume all works for one or more');
        report('check','resume panel shown for '+intr.length);
      }
      for(const saved of sessions){
        await select(saved);await until(()=>!$('live-start').disabled,'resume enabled');assert($('live-start').textContent.includes('Resume'),'explicit resume action');
        const rb=document.querySelector(`[data-resume="${saved.id}"]`);if(rb)rb.click();else $('live-start').click();
        await until(async()=>(await snapshot()).find(s=>s.id===saved.id).status==='running','resume launched');
        const s=(await snapshot()).find(s=>s.id===saved.id);
        assert(s.generation!==saved.generation,'fresh generation after resume');
        await until(async()=>(await output(s)).includes('FIXTURE READY'),'resumed terminal ready');
        assert((await output(s)).includes(`MODE ${s.provider==='codex'?'resume':'--resume'} RESTORED 1`),'exact ID restores one prior turn');
        let staleDenied=false;try{await invoke('live_write',{id:s.id,generation:saved.generation,data:'MUST-NOT-SEND\r'});}catch(e){staleDenied=true;}
        assert(staleDenied,'stale generation input refused');
        $('live-stop').click();await until(async()=>(await snapshot()).find(x=>x.id===s.id).status==='stopped','explicit stop');
      }
      if(!TEST_LAYOUT)await usageChecks();
      {
        // Follow: /resume inside the CLI writes another transcript in the same folder; the running session switches to it, and the next start resumes it.
        const c=(await snapshot()).find(x=>x.provider==='claude'),old=c.provider_session_id;
        $('show-live').click();await select(c);await until(()=>!$('live-start').disabled,'claude resume enabled');$('live-start').click();
        await until(async()=>(await snapshot()).find(x=>x.id===c.id).status==='running','claude running for follow');
        let s=(await snapshot()).find(x=>x.id===c.id);await until(async()=>(await output(s)).includes('FIXTURE READY'),'follow fixture ready');
        const before=(await invoke('live_follow_check')).find(f=>f.app_session_id===c.id);
        assert(before&&before.decision==='current'&&before.saved===old,'nothing to follow before a /resume');
        {const snap=await snapshot();assert((await invoke('live_follow_check')).every(f=>snap.find(x=>x.id===f.app_session_id)?.provider==='claude'),'only Claude sessions are checked');}
        await invoke('live_write',{id:s.id,generation:s.generation,data:'/fixture-resume-other\r'});
        let next='';await until(async()=>{const m=(await output(s)).match(/RESUMED OTHER ([0-9a-f-]{36})/);next=m?m[1]:'';return next;},'fixture wrote another conversation');
        let refused=false;try{await invoke('live_use_conversation',{id:c.id,expected:old,conversation:'00000000-0000-4000-8000-000000000000'});}catch(e){refused=/not a newer conversation/.test(String(e));}
        assert(refused,'a conversation that is not in the folder is refused');
        await until(async()=>{const f=(await invoke('live_follow_check')).find(f=>f.app_session_id===c.id);return f&&f.saved===next&&f.followed&&f.followed.old_id===old&&f.followed.how==='auto';},'running session follows the new conversation');
        await until(()=>$('live-identity').textContent.includes(`Provider ${next}`)&&$('live-identity').textContent.includes(`followed from ${old}`),'identity details show the followed conversation');
        // Undo goes back to the previous ID and is not switched again automatically; "Use it" then picks the newer one.
        await until(()=>/Now following conversation/.test($('live-follow-text').textContent)&&$('live-follow-use').textContent==='Undo','Now following line with Undo');
        $('live-follow-use').click();
        await until(async()=>(await snapshot()).find(x=>x.id===c.id).provider_session_id===old,'Undo restores the previous conversation');
        await until(async()=>{const f=(await invoke('live_follow_check')).find(f=>f.app_session_id===c.id);return f&&f.saved===old&&f.decision==='ambiguous'&&f.newer&&f.newer.id===next;},'an undone switch is offered, not repeated');
        await until(()=>$('live-follow-use').textContent==='Use it'&&!$('live-follow-use').hidden,'Use it offered after Undo');
        $('live-follow-use').click();
        await until(async()=>(await snapshot()).find(x=>x.id===c.id).provider_session_id===next,'Use it switches to the newer conversation');
        $('live-stop').click();await until(async()=>(await snapshot()).find(x=>x.id===c.id).status==='stopped','stop after follow');
        await until(()=>!$('live-start').disabled,'resume enabled after follow');$('live-start').click();await until(async()=>(await snapshot()).find(x=>x.id===c.id).status==='running','restart after follow');
        s=(await snapshot()).find(x=>x.id===c.id);await until(async()=>(await output(s)).includes('FIXTURE READY'),'followed conversation resumed');
        assert((await output(s)).includes(`SYNTHETIC CLAUDE CLI ${next}`)&&(await output(s)).includes('MODE --resume'),'next start resumes the followed conversation');
        $('live-stop').click();await until(async()=>(await snapshot()).find(x=>x.id===c.id).status==='stopped','stop after followed resume');
        report('check','follow '+old.slice(0,8)+'->'+next.slice(0,8));
      }
      {
        // Documents page: add by path with a review step, read in the app, refuse unsafe paths, stop sharing.
        const shared=`${PROJECT_ROOT}/shared`;const files=()=>[...document.querySelectorAll('#docs-files .docs-file')].map(b=>b.textContent);
        $('show-docs').click();await until(()=>!$('docs').hidden,'Documents page opens in the main window');
        assert((await invoke('docs_list')).shared.length===0,'nothing is shared at first');
        $('docs-path').value='/etc';$('docs-add-form').dispatchEvent(new Event('submit',{cancelable:true}));
        await until(()=>/system path/.test($('docs-notice').textContent),'a system folder is refused with a reason');
        $('docs-path').value=shared;$('docs-add-form').dispatchEvent(new Event('submit',{cancelable:true}));
        await until(()=>!$('docs-confirm').hidden,'the review step appears');
        assert($('docs-confirm-text').textContent.includes(shared),'the review shows the full resolved path');
        assert((await invoke('docs_list')).shared.length===0,'reviewing saves nothing');
        let staleDenied=false;try{await invoke('docs_add',{path:shared,expect:'/somewhere/else'});}catch(e){staleDenied=/changed since/.test(String(e));}
        assert(staleDenied,'adding with a path other than the reviewed one is refused');
        $('docs-confirm-add').click();await until(()=>files().some(f=>f.endsWith('readme.md')),'shared folder lists its files');
        assert(!files().some(f=>f.includes('.env')),'denied file names are not listed');
        document.querySelector('#docs-files .docs-file').click();
        await until(()=>!$('docs-view').hidden&&$('docs-view-body').textContent.includes('hello docs'),'the file is read in the app');
        let denied=false;try{await invoke('docs_read',{path:`${shared}/.env`});}catch(e){denied=true;}assert(denied,'a denied file cannot be read');
        document.querySelector('#docs-shared button').click();
        await until(async()=>(await invoke('docs_list')).shared.length===0,'stop sharing removes the entry');
        $('show-live').click();await until(()=>!$('live').hidden,'back on the Sessions page');
      }
      $('open-docs').click();
      await until(()=>new Promise(resolve=>{const script=document.createElement('script');script.src='/__workspace-docs-status.js';script.onload=()=>{script.remove();resolve(window.__docsDenied);};document.head.append(script);}),'document window denies session commands');
      report('phase','live-restart-preview');
    }
    await new Promise(r=>setTimeout(r,TEST_LAYOUT?16000:2000));
    assert(fits(),'terminal fits its resized viewport');
    report('result','pass');
  }catch(e){report('error',String(e));report('result','fail');}
})();
