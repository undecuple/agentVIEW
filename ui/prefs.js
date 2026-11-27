'use strict';
(() => {
  window.AppIPC={invoke:(command,args={})=>window.__TAURI_INTERNALS__.invoke(command,args)};
  const invoke=(command,args={})=>AppIPC.invoke(command,args);
  const clone=value=>JSON.parse(JSON.stringify(value));
  let snapshot=null,queue=Promise.resolve(),appliedScale=null,scaleInFlight=false,scalePending=false,scaleError='';
  let pendingFilters={},filterTimer=null;
  const media=matchMedia('(prefers-color-scheme: light)');
  const banner=text=>{document.getElementById('preferences-notice').textContent=text;};
  function notice(){banner([snapshot?.recovery?'Preferences need attention. Open Settings to review recovery options.':'',scaleError].filter(Boolean).join(' '));}
  async function applyScale(){
    scalePending=true;if(scaleInFlight)return;scaleInFlight=true;
    let attempts=0;
    while(scalePending&&attempts++<3){
      scalePending=false;if(appliedScale===snapshot?.settings.ui_scale_percent)continue;
      try{
        const actual=await invoke('app_settings_apply');appliedScale=actual;scaleError='';document.body.dataset.appliedScale=String(actual);
        if(actual!==snapshot?.settings.ui_scale_percent)accept(await invoke('app_settings_snapshot'));
      }catch(_){scaleError='Preferences are saved, but interface scaling could not be applied. Reload saved preferences to retry.';scalePending=false;break;}
    }
    if(scalePending){scalePending=false;scaleError='Preferences changed during scaling. Reload saved preferences to retry.';}
    scaleInFlight=false;notice();
  }
  function appearance(){
    const s=snapshot?.settings;if(!s)return;
    const theme=s.theme==='system'?(media.matches?'light':'dark'):s.theme;
    document.documentElement.dataset.theme=theme;document.documentElement.dataset.density=s.density;
    document.querySelector('.brand-symbol').src=(theme==='light'||theme==='papercolor-light')?'/agentview-light.svg':'/agentview.svg';
    window.dispatchEvent(new CustomEvent('agentview:theme',{detail:theme}));
    if(appliedScale!==s.ui_scale_percent)applyScale();
  }
  function accept(next){snapshot=next;document.body.dataset.demo=String(!!next.settings.demo_mode);if(!next.settings.remember_filters){clearTimeout(filterTimer);pendingFilters={};}appearance();notice();window.dispatchEvent(new CustomEvent('agentview:preferences',{detail:clone(next.settings)}));return next;}
  function enqueue(action){const result=queue.then(action);queue=result.catch(()=>{});return result;}
  async function submit(settings,revision,reset=false){
    if(!snapshot?.writable)throw Error('Preferences are read-only or unavailable.');
    try{return accept(await invoke(reset?'app_settings_reset':'app_settings_save',reset?{expectedRevision:revision}:{expectedRevision:revision,settings}));}
    catch(e){try{accept(await invoke('app_settings_snapshot'));}catch(_){}throw e;}
  }
  const loaded=invoke('app_settings_snapshot');
  const dom=document.readyState!=='complete'?new Promise(resolve=>document.addEventListener('DOMContentLoaded',resolve,{once:true})):Promise.resolve();
  window.AppPrefs={
    get snapshot(){return snapshot?clone(snapshot):null;},get current(){return snapshot?clone(snapshot.settings):null;},
    save:(settings,revision)=>enqueue(()=>submit(settings,revision)),
    reset:revision=>enqueue(()=>submit(null,revision,true)),
    reload:()=>enqueue(async()=>accept(await invoke('app_settings_snapshot'))),
    patch:patch=>enqueue(()=>{if(!snapshot||snapshot.recovery)throw Error('Recover preferences in Settings before saving changes.');return submit({...snapshot.settings,...patch},snapshot.revision);}),
    remember(patch){if(snapshot?.settings.remember_filters&&!snapshot.recovery){Object.assign(pendingFilters,patch);clearTimeout(filterTimer);filterTimer=setTimeout(()=>{const next=pendingFilters;pendingFilters={};if(!snapshot?.settings.remember_filters)return;this.patch(next).catch(e=>banner(`Filters were not saved: ${String(e)}`));},200);}},
    ready:null,
  };
  const shown={};
  window.AppNav={
    // Runs fn every time `page` is shown, however it was opened (sidebar, keyboard, startup page, a link). Adding the same fn twice is ignored.
    onShow(page,fn){const set=(shown[page]||=new Set());if(set.has(fn))return;set.add(fn);window.addEventListener('agentview:navigate',e=>{if(e.detail===page)fn();});},
    setCount(name,n){const el=document.getElementById('nc-'+name);if(!el)return;const v=Number.isSafeInteger(n)&&n>0?n:0;el.textContent=String(v);el.hidden=!v;},navigate(page){
    if(!['agents','welcome','sessions','overview','flow','docs','notes','cc','git','review','usage','settings'].includes(page))return;
    if(page==='agents'&&document.body.dataset.demo!=='true')page='sessions';
    for(const [id,key] of [['deck','agents'],['live','sessions'],['overview','overview'],['flow','flow'],['welcome','welcome'],['docs','docs'],['notes','notes'],['cc','cc'],['git','git'],['review','review'],['usage','usage'],['settings','settings']])document.getElementById(id).hidden=page!==key;
    for(const selector of ['.page-heading','.summary','.demo-note'])document.querySelector(selector).hidden=page!=='agents';
    document.querySelector('.topbar strong').textContent={agents:'Agents',sessions:'Sessions',overview:'All agents',flow:'Flow',welcome:'Welcome',docs:'Documents',notes:'Notes',cc:'Claude config',git:'Git',review:'Pre-PR review',usage:'Usage',settings:'Settings'}[page];
    document.querySelector('.demo-pill').textContent={agents:'SIMULATED DEMO',sessions:'LOCAL CLI SESSIONS',overview:'LOCAL OBSERVATIONS',flow:'PROCESS STATUS',welcome:'GET STARTED',docs:'READ-ONLY DOCUMENTS',notes:'APP-OWNED NOTES',cc:'READ-ONLY',git:'READ-ONLY GIT',review:'MANUAL REVIEW',usage:'LOCAL USAGE RECORDS',settings:'APP PREFERENCES'}[page];
    for(const [id,key] of [['show-agents','agents'],['show-live','sessions'],['show-overview','overview'],['show-flow','flow'],['show-docs','docs'],['show-notes','notes'],['show-cc','cc'],['show-git','git'],['show-review','review'],['show-usage','usage'],['show-settings','settings']]){
      const button=document.getElementById(id);button.classList.toggle('active',page===key);if(page===key)button.setAttribute('aria-current','page');else button.removeAttribute('aria-current');
    }
    document.body.dataset.page=page;document.scrollingElement.scrollTop=0;
    window.dispatchEvent(new CustomEvent('agentview:navigate',{detail:page}));
  }};
  for(const [id,page] of [['show-agents','agents'],['show-live','sessions'],['show-overview','overview'],['show-flow','flow'],['show-docs','docs'],['show-notes','notes'],['show-cc','cc'],['show-git','git'],['show-review','review'],['show-usage','usage'],['show-settings','settings'],['back-demo','agents'],['close-usage','agents']])document.getElementById(id).onclick=()=>AppNav.navigate(page);
  media.addEventListener('change',appearance);
  AppPrefs.ready=Promise.all([loaded,dom]).then(([next])=>accept(next)).catch(()=>{banner('Preferences could not be loaded. Built-in defaults are in use.');return null;});
  // Register after other deferred scripts have attached their preference listeners.
  dom.then(()=>AppPrefs.ready.then(next=>{document.body.dataset.preferencesReady='true';AppNav.navigate(next?.settings.default_page||'agents');}));
})();

;(()=>{ // Sidebar collapse: a per-viewer convenience kept in localStorage (may be unavailable; the page works without it)
  const key='agentview.rail';let pref=null;const narrow=matchMedia('(max-width:1000px)');
  try{const v=localStorage.getItem(key);pref=v==='collapsed'||v==='expanded'?v:null;}catch{}
  // A saved choice wins; with none, the sidebar folds to icons in a narrow window.
  const isOn=()=>pref?pref==='collapsed':narrow.matches;
  function apply(){const on=isOn();
    document.body.dataset.rail=on?'collapsed':'';
    const b=document.getElementById('rail-toggle');if(!b)return;
    b.textContent=on?'»':'«';b.setAttribute('aria-pressed',String(on));
    const l=on?'Expand sidebar':'Collapse sidebar';b.setAttribute('aria-label',l);b.title=l;
  }
  window.addEventListener('DOMContentLoaded',()=>{apply();document.getElementById('rail-toggle').onclick=()=>{pref=isOn()?'expanded':'collapsed';try{localStorage.setItem(key,pref);}catch{}apply();};narrow.addEventListener('change',apply);});
})();

;(()=>{ // Keyboard: Alt+1..9 opens the nth sidebar page, Alt+0 Settings; Esc closes the open section or leaves the field. Not inside a terminal.
  document.addEventListener('keydown',e=>{
    if(e.target.closest&&e.target.closest('.xterm,#live-terminal'))return;
    if(e.altKey&&!e.ctrlKey&&!e.metaKey&&/^[1-9]$/.test(e.key)){
      const items=[...document.querySelectorAll('.rail .nav-item')].filter(b=>b.tagName==='BUTTON'&&!b.hidden&&b.offsetParent!==null);
      const b=items[Number(e.key)-1];if(b){e.preventDefault();b.click();}
    }else if(e.altKey&&!e.ctrlKey&&!e.metaKey&&e.key==='0'){e.preventDefault();AppNav.navigate('settings');
    }else if(e.key==='Escape'&&!e.altKey&&!e.ctrlKey&&!e.metaKey){
      const d=e.target.closest?.('details[open]');
      if(d&&d.id!=='live-setup-never'){d.open=false;d.querySelector('summary')?.focus();}else if(e.target.blur&&e.target!==document.body)e.target.blur();
    }
  });
})();
