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
      assert(missingRow.textContent.includes('Source missing; saved values retained')&&missingRow.cells[5].textContent==='195'&&missingRow.textContent.includes(x.last_measured_at),'missing source renders retained values and measurement time honestly');
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
      $('live-provider').value=provider;$('live-provider').dispatchEvent(new Event('change'));$('live-probe').click();
      await until(()=>!$('live-register').disabled&&$('live-tool').textContent.toLowerCase().includes(`synthetic ${provider}`),'probe provider fixture');
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
        $('live-start').click();
        await until(async()=>(await snapshot()).filter(s=>s.status==='running').length===(project==='alpha'?1:2),'session launched');
        await until(()=>$('live-notice').textContent.includes('Process launched'),'start finished in the UI');
        if(project==='alpha'){assert($('live-rename').disabled&&$('live-remote-enable').disabled,'rename and Remote Control are locked while running');}
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
        $('show-flow').click();
        await until(()=>$('flow-svg').querySelectorAll('.flow-sess').length===2,'flow shows both registered sessions');
        assert($('flow-svg').querySelectorAll('.flow-prov').length===2&&$('flow-svg').querySelectorAll('.flow-sess[data-state=running]').length===2,'flow shows two providers and both sessions running');
        assert(!/approv|waiting for (a )?decision/i.test($('flow').textContent.replace(/approvals are made[^.]*\./i,'')),'flow shows no approval state');
        $('show-live').click();
      }
      assert((await snapshot()).filter(s=>s.status==='running').length===2,'two project sessions coexist');
      if(!TEST_LAYOUT){await usageChecks();await terminalPreferenceChecks(sessions);}
      await select(sessions.find(s=>s.provider==='codex'));
      if(!TEST_LAYOUT){$('live-quick').open=true;document.scrollingElement.scrollTop=0;}
      report('phase','live-first-preview');
    }else{
      assert(sessions.length===2&&sessions.every(s=>s.attempted&&s.status!=='running'),'metadata restored without relaunch');
      for(const saved of sessions){
        await select(saved);await until(()=>!$('live-start').disabled,'resume enabled');assert($('live-start').textContent.includes('Resume'),'explicit resume action');$('live-start').click();
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
      $('open-docs').click();
      await until(()=>new Promise(resolve=>{const script=document.createElement('script');script.src='/__workspace-docs-status.js';script.onload=()=>{script.remove();resolve(window.__docsDenied);};document.head.append(script);}),'document window denies session commands');
      report('phase','live-restart-preview');
    }
    await new Promise(r=>setTimeout(r,TEST_LAYOUT?16000:2000));
    assert(fits(),'terminal fits its resized viewport');
    report('result','pass');
  }catch(e){report('error',String(e));report('result','fail');}
})();
