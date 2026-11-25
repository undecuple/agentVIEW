// Test-only app-owned preferences: synthetic data, no provider process or login.
(async()=>{
  const $=id=>document.getElementById(id),invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(key,value)=>{new Image().src=`/__workspace-report?${key}=${encodeURIComponent(value)}`;};
  const assert=(ok,label)=>{if(!ok)throw Error(label);report('check',label);};
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await new Promise(r=>setTimeout(r,100));}throw Error(`timeout ${label}: ${$('settings-notice').textContent}`);}
  const input=(id,value)=>{const e=$(id);if(e.type==='checkbox')e.checked=value;else e.value=value;e.dispatchEvent(new Event('input',{bubbles:true}));};
  const snapshot=()=>invoke('app_settings_snapshot');
  async function save(){const before=(await snapshot()).revision;$('settings-form').dispatchEvent(new Event('submit',{cancelable:true}));await until(async()=>(await snapshot()).revision>before,'settings saved');await until(()=>$('settings-notice').textContent==='Preferences saved.','save UI completed');}
  async function capture(name){report('capture',name);await new Promise(r=>setTimeout(r,1400));}
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','all startup listeners ready');
    if(TEST_PHASE==='first'){
      assert((await snapshot()).revision===0,'missing preferences load without a write');
      input('provider','claude');input('project-filter','Documentation');input('status-filter','paused');$('project-view').click();
      $('show-live').click();input('live-filter','codex');$('live-filter').dispatchEvent(new Event('change'));
      $('show-settings').click();
      input('pref-theme','light');input('pref-ui-scale',110);input('pref-density','compact');input('pref-default-page','usage');input('pref-provider','codex');input('pref-remember',true);
      input('pref-font-size',16);input('pref-scrollback',1000);input('pref-cursor-blink',false);input('pref-usage-provider','codex');input('pref-usage-period','today');input('pref-refresh',60);
      const row=$('settings-prompts').querySelector('[data-preset=inbox]');row.querySelector('input').value='Inbox fixture';row.querySelector('input').dispatchEvent(new Event('input',{bubbles:true}));
      row.querySelector('textarea').value='DIAGNOSTIC_SECRET_MARKER Review the shared inbox.';row.querySelector('textarea').dispatchEvent(new Event('input',{bubbles:true}));
      row.querySelector('[data-move=down]').click();
      const nativeApplyInvoke=AppIPC.invoke;let failedApply=false;
      AppIPC.invoke=(command,args)=>{if(command==='app_settings_apply'&&!failedApply){failedApply=true;return Promise.reject('synthetic scale failure');}return nativeApplyInvoke(command,args);};
      await save();await until(()=>$('preferences-notice').textContent.includes('scaling could not be applied'),'failed appearance remains visible after successful persistence');
      AppIPC.invoke=nativeApplyInvoke;$('settings-reload').click();
      await until(()=>document.body.dataset.appliedScale==='110','native scale applied');
      assert(!$('preferences-notice').textContent,'retry clears the scale error only after successful apply');
      const saved=await snapshot();assert(saved.settings.theme==='light'&&saved.settings.preferred_provider==='codex','appearance/provider preferences persisted');
      assert(getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()==='#ffffff','matching Cyberdream light palette applied');
      assert(saved.settings.quick_prompts[1].id==='inbox'&&saved.settings.quick_prompts[1].label==='Inbox fixture','preset edit and reorder persisted');
      assert((await invoke('live_snapshot')).length===0,'settings and About do not launch/register a CLI');
      let conflict=false;try{await invoke('app_settings_save',{expectedRevision:0,settings:saved.settings});}catch(_){conflict=true;}assert(conflict,'stale settings revision cannot overwrite a newer save');
      input('pref-font-size',18);
      await invoke('app_settings_save',{expectedRevision:saved.revision,settings:{...saved.settings,terminal_font_size:17}});
      $('settings-form').dispatchEvent(new Event('submit',{cancelable:true}));
      await until(()=>$('settings-notice').textContent.includes('draft is preserved'),'stale UI draft surfaced');
      assert($('pref-font-size').value==='18'&&(await snapshot()).settings.terminal_font_size===17,'stale form preserves draft and stored newer value');
      $('settings-reload').click();await until(()=>$('pref-font-size').value==='17','explicit reload');input('pref-font-size',16);await save();
      $('settings-export').click();await until(()=>$('settings-export-status').textContent.includes('Report saved locally:'),'native diagnostic export');
      assert(!$('settings-report').textContent.includes('DIAGNOSTIC_SECRET_MARKER')&&!$('settings-report').textContent.includes('/tmp/'),'diagnostics exclude preset text and filesystem paths');
      assert(document.documentElement.scrollWidth<=window.innerWidth+1,'settings fit the current viewport');
      document.scrollingElement.scrollTop=0;await capture('settings-light');
      input('pref-theme','dark');await save();document.scrollingElement.scrollTop=0;await capture('settings-dark');
      input('pref-theme','system');await save();assert(document.documentElement.dataset.theme===(matchMedia('(prefers-color-scheme: light)').matches?'light':'dark'),'System theme follows the OS preference');
      input('pref-theme','papercolor-light');await save();
      assert(document.documentElement.dataset.theme==='papercolor-light'&&getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()==='#eeeeee'&&document.querySelector('.brand-symbol').src.endsWith('/agentview-light.svg'),'PaperColor Light applies with the light mark');
      input('pref-theme','papercolor-dark');await save();
      assert(document.documentElement.dataset.theme==='papercolor-dark'&&getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()==='#1c1c1c','PaperColor Dark applies');
      input('pref-theme','light');await save();
    }else if(TEST_PHASE==='restart'){
      await until(()=>!$('usage').hidden&&$('usage-notice').textContent.includes('Showing saved'),'saved default Usage page loads on fast startup');
      assert(document.documentElement.dataset.theme==='light'&&document.body.dataset.appliedScale==='110','theme and native scale restored on restart');
      assert($('usage-provider').value==='codex'&&$('usage-period').value==='today','usage defaults restored before rendering');
      assert($('provider').value==='claude'&&$('project-filter').value==='Documentation'&&$('status-filter').value==='paused'&&$('project-view').getAttribute('aria-pressed')==='true','hierarchy filters and mode restored');
      $('show-live').click();assert($('live-provider').value==='codex'&&$('live-filter').value==='codex','preferred provider and session filter restored');
      assert([...document.querySelectorAll('#quick-buttons button')][1].textContent==='Inbox fixture','custom quick buttons restored after restart');
      assert((await invoke('live_snapshot')).length===0,'opening saved Sessions page does not launch a CLI');
      // Advance only the optional usage timer; real IPC/storage still run.
      const nativeTimeout=window.setTimeout,nativeFocus=document.hasFocus,nativeInvoke=AppIPC.invoke;
      const callbacks=[];let refreshes=0;document.hasFocus=()=>true;
      window.setTimeout=(fn,delay,...args)=>{if(delay===60000){callbacks.push(fn);return nativeTimeout.call(window,()=>{},60000);}return nativeTimeout.call(window,fn,delay,...args);};
      AppIPC.invoke=(command,args)=>{if(command==='usage_refresh')refreshes++;return nativeInvoke(command,args);};
      $('show-usage').click();await until(()=>callbacks.length>0&&$('usage-notice').textContent.includes('Showing saved'),'opt-in timer armed after saved snapshot loaded');
      await callbacks.pop()();assert(refreshes===1,'optional timer performs a real local refresh');
      const pending=callbacks.pop();$('show-settings').click();if(pending)await pending();assert(refreshes===1,'hidden Usage does not refresh from an old timer callback');
      window.setTimeout=nativeTimeout;document.hasFocus=nativeFocus;AppIPC.invoke=nativeInvoke;
      $('pref-reset-prompts').click();await save();assert((await snapshot()).settings.quick_prompts[0].label==='Review shared inbox','restore default presets works separately');
      { // tabs and the issue helper
        const vis=id=>!$(id).hidden;
        window.AppSettingsTabs.select('general');assert(!vis('hook-box')&&!vis('codexcfg-box')&&!vis('limits-box')&&!vis('desk-box')&&!vis('ih-box'),'notification and about cards are hidden on General');
        window.AppSettingsTabs.select('notifications');assert(vis('hook-box')&&vis('codexcfg-box')&&vis('limits-box')&&vis('desk-box')&&!vis('ih-box'),'all notification cards show on the Notifications tab only');
        window.AppSettingsTabs.select('about');assert(vis('ih-box')&&!vis('desk-box'),'the issue helper shows on the About tab');
        const set=(id,v)=>{$(id).value=v;$(id).dispatchEvent(new Event('input'));};
        await until(()=>$('ih-version').value.startsWith('agentVIEW '),'issue helper knows the version');
        set('ih-title','Terminal blank after resume');set('ih-what','It stays blank. Log in /'+'home/someone/proj/x.log');
        assert($('ih-copy-link').disabled&&/distribution/.test($('ih-note').textContent),'bug link needs the system field');
        set('ih-system','Arch Linux, WebKitGTK 2.46');
        assert(!$('ih-copy-link').disabled,'link enabled when the form is complete');
        const u=new URL($('ih-link').value);
        assert(u.origin+u.pathname==='https://github.com/undecuple/agentVIEW/issues/new'&&u.searchParams.get('template')==='bug_report.yml'&&u.searchParams.get('build').startsWith('agentVIEW ')&&u.searchParams.get('system')==='Arch Linux, WebKitGTK 2.46','link targets the bug form with its field ids');
        assert(u.searchParams.get('what').includes('~/proj/x.log')&&!u.searchParams.get('what').includes('/home/'),'home-folder paths are shortened');
        set('ih-what','token ghp_'+'abcdefghijklmnopqrstuvwxyz0123456789');
        assert($('ih-copy-link').disabled&&$('ih-copy-what').disabled&&/token/.test($('ih-note').textContent),'a token-shaped string blocks copying');
        set('ih-what','x');$('ih-kind').value='feature';$('ih-kind').dispatchEvent(new Event('input'));
        assert(new URL($('ih-link').value).searchParams.get('template')==='feature_request.yml'&&!new URL($('ih-link').value).searchParams.has('build'),'feature link uses the feature form');
        window.AppSettingsTabs.select('general');
      }
      assert([...$('pref-lib').options].some(o=>o.value==='role-worker'&&o.textContent==='Onboard: Worker'),'role onboarding prompts are in the library');
      $('pref-lib').value='pickup';$('pref-add-lib').click();$('pref-lib').value='pickup';$('pref-add-lib').click();await save();
      { const q=(await snapshot()).settings.quick_prompts;assert(q.length===4&&q[3].label==='Pick up from a handoff'&&q.filter(x=>x.label==='Pick up from a handoff').length===1&&/^lib_pickup_/.test(q[3].id),'a library prompt is added once and persists as an ordinary preset'); }
      $('open-docs').click();await until(()=>new Promise(resolve=>{const s=document.createElement('script');s.src='/__workspace-docs-status.js';s.onload=()=>{s.remove();resolve(window.__docsDenied);};document.head.append(s);}), 'Documents denies Settings and export commands');
      report('phase','settings-restart');
    }else if(TEST_PHASE==='corrupt'){
      assert((await snapshot()).recovery==='corrupt_json','damaged saved preferences have visible recovery');$('show-settings').click();
      assert($('settings-save').disabled&&!$('settings-reset').disabled,'damaged settings require explicit reset');
      $('settings-reset').click();await until(()=>$('settings-notice').textContent.includes('App defaults restored'),'explicit recovery completed');
      assert((await snapshot()).recovery===null&&(await snapshot()).settings.theme==='dark','recovery restores valid defaults');
    }else if(TEST_PHASE==='future'){
      const future=await snapshot();assert(future.recovery==='newer_version'&&!future.writable,'future settings remain read-only');$('show-settings').click();
      assert($('settings-save').disabled&&$('settings-reset').disabled,'future settings cannot be overwritten by controls');
      let refused=false;try{await invoke('app_settings_reset',{expectedRevision:future.revision});}catch(_){refused=true;}assert(refused,'native reset also refuses a future format');
    }
    report('phase',`settings-${TEST_PHASE}-complete`);await new Promise(r=>setTimeout(r,1400));report('result','pass');
  }catch(e){report('error',String(e));report('result','fail');}
})();
