// Test-only layout audit for the real embedded UI: synthetic data, no provider process.
// For each view and interface scale, report text boxes that overlap other text boxes and text clipped by its box.
(async()=>{
  const $=id=>document.getElementById(id),invoke=(c,a={})=>window.__TAURI_INTERNALS__.invoke(c,a);
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  async function until(fn,label){for(let i=0;i<200;i++){if(await fn())return;await sleep(100);}throw Error(`timeout ${label}`);}
  const visible=e=>{const s=getComputedStyle(e);if(s.visibility==='hidden'||s.display==='none'||+s.opacity===0)return false;for(let p=e;p;p=p.parentElement)if(p.hidden||getComputedStyle(p).display==='none')return false;return true;};
  function audit(){
    const boxes=[];const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);
    while(walker.nextNode()){
      const n=walker.currentNode;if(!n.nodeValue.trim())continue;const el=n.parentElement;
      if(!el||['SCRIPT','STYLE','OPTION'].includes(el.tagName)||el.closest('.xterm,pre,#live-terminal')||!visible(el))continue;
      const r=document.createRange();r.selectNodeContents(n);
      for(const q0 of r.getClientRects()){
        if(q0.width<2||q0.height<4)continue;
        // Intersect with every clipping ancestor and the viewport: scrolled-out text is not visible.
        let L=Math.max(q0.left,0),T=Math.max(q0.top,0),R=Math.min(q0.right,innerWidth),B=Math.min(q0.bottom,innerHeight);
        for(let p=el;p&&p!==document.documentElement;p=p.parentElement){
          const s=getComputedStyle(p);if(s.overflowX!=='visible'||s.overflowY!=='visible'){const c=p.getBoundingClientRect();L=Math.max(L,c.left);T=Math.max(T,c.top);R=Math.min(R,c.right);B=Math.min(B,c.bottom);}
        }
        if((R-L)*(B-T)<0.6*q0.width*q0.height)continue;
        // Text hidden behind an opaque element drawn on top (for example the sticky save bar) is not an overlap.
        const top=document.elementFromPoint((L+R)/2,(T+B)/2);if(top&&!top.contains(el)&&!el.contains(top))continue;
        boxes.push({el,n,x:L,y:T,w:R-L,h:B-T,t:n.nodeValue.trim().slice(0,30),q:q0});
      }
    }
    const clipped=[];const hits=[];
    for(let i=0;i<boxes.length;i++)for(let j=i+1;j<boxes.length;j++){
      const a=boxes[i],b=boxes[j];if(a.el===b.el||a.el.contains(b.el)&&a.n===b.n)continue;
      const ox=Math.min(a.x+a.w,b.x+b.w)-Math.max(a.x,b.x),oy=Math.min(a.y+a.h,b.y+b.h)-Math.max(a.y,b.y);
      if(ox>2&&oy>3&&ox*oy>12)hits.push(`${a.t}|${b.t}`);
    }
    for(const e of document.querySelectorAll('button,label,th,td,strong,small,h1,h2,h3,span,p,legend,summary,a')){
      if(!visible(e))continue;const s=getComputedStyle(e);
      if(e.scrollWidth>e.clientWidth+1&&e.clientWidth>0&&['hidden','clip'].includes(s.overflowX)&&s.textOverflow!=='ellipsis')clipped.push((e.textContent||'').trim().slice(0,30));
    }
    for(const b of boxes){
      const p=b.el.parentElement;if(!p||b.el===document.body)continue;
      const s=getComputedStyle(p);if(['auto','scroll','hidden','clip'].includes(s.overflowX)||s.display==='contents'||s.display==='inline')continue;
      {const own=getComputedStyle(b.el);if(['hidden','clip'].includes(own.overflowX)&&own.textOverflow==='ellipsis')continue;} // an ellipsised element is meant to be cut
      const c=p.getBoundingClientRect();if(c.width===0)continue;
      if(b.q.right>c.right+1.5||b.q.left<c.left-1.5)clipped.push('SPILL '+b.el.tagName+'#'+(b.el.id||b.el.className)+' in '+p.tagName+'#'+(p.id||p.className)+' '+b.t);
    }
    return {hits,clipped,scrollX:document.documentElement.scrollWidth>window.innerWidth+1};
  }
  const out=[];
  async function check(view,scale){
    await sleep(400);const r=audit();
    report('audit',`${view}@${scale}% w=${innerWidth} overlaps=${r.hits.length} clipped=${r.clipped.length} hscroll=${r.scrollX} ${r.hits.slice(0,6).join(' ;; ')} ${[...new Set(r.clipped)].slice(0,5).join(' ;; ')}`);
    return r;
  }
  const views=[['agents','show-agents'],['sessions','show-live'],['overview','show-overview'],['flow','show-flow'],['notes','show-notes'],['cc','show-cc'],['git','show-git'],['review','show-review'],['usage','show-usage'],['settings','show-settings']];
  async function setScale(v){
    $('show-settings').click();
    const e=$('pref-ui-scale');e.value=String(v);e.dispatchEvent(new Event('input',{bubbles:true}));
    const rev=(await invoke('app_settings_snapshot')).revision;
    $('settings-form').dispatchEvent(new Event('submit',{cancelable:true}));
    await until(async()=>(await invoke('app_settings_snapshot')).revision>rev,'saved');
    await until(()=>document.body.dataset.appliedScale===String(v),'applied');await sleep(500);
  }
  // Long synthetic values the real views render (paths, labels, session IDs, big counters).
  function stress(){
    const long='/home/user/projects/a-very-long-project-directory-name/with/many/nested/folders/and-a-long-final-name';
    const roster=$('live-roster');
    if(roster&&!roster.querySelector('[data-stress]')){for(const l of ['Example long-named coordinator with a long label','demo','example-coord']){
      const b=document.createElement('button');b.type='button';b.className='live-session';b.dataset.stress='1';
      const n=document.createElement('strong');n.textContent=l;const s=document.createElement('small');s.textContent='Claude Code · Coordinator · running';b.append(n,s);roster.append(b);}}
    if($('live-project')){$('live-project').textContent=long;$('live-name').textContent='Claude Code · Example long-named coordinator with a long label';
      $('live-identity').textContent=`${long} · App 0123456789abcdef0123456789abcdef · Provider 0123456789abcdef0123456789abcdef-0123 · 2.1.99 (Claude Code) · /nix/store/abcdefghijklmnopqrstuvwxyz0123456789-claude/bin/claude`;}
    for(const id of ['usage-agents','usage-days']){const tb=$(id);if(tb&&!tb.querySelector('[data-stress]')){
      const tr=document.createElement('tr');tr.dataset.stress='1';for(const v of [id==='usage-agents'?'Example long-named coordinator / session 0123456789abcdef':'2026-11-23','123,456,789','98,765,432','1,234,567,890','12,345,678','2,469,135,780'])
        {const td=document.createElement('td');td.textContent=v;tr.append(td);}tb.append(tr);}}
  }
  try{
    await until(()=>document.body.dataset.ready==='true'&&document.body.dataset.preferencesReady==='true','ready');
    for(const scale of [100,125,150,80]){
      await setScale(scale);
      for(const [name,btn] of views){
        $(btn).click();await sleep(300);stress();
        await check(name,scale);
        if(name==='cc'){for(const tb of ['settings','skills','memory','history','projects']){window.AppCcTabs.select(tb);await check('cc-'+tb,scale);}}
        if(name==='settings'){for(const tab of ['data','notifications','about']){window.AppSettingsTabs.select(tab);await check('settings-'+tab,scale);}window.AppSettingsTabs.select('general');}
        if(name==='flow'){await sleep(1500);await until(()=>$('tasks-body').children.length>=3&&$('wfruns-body').children.length>=1,'seeded tasks and workflow rows');
          report('tasks',`rows=${$('tasks-body').children.length} runs=${$('wfruns-body').children.length}`);$('tasks-table').closest('.card').scrollIntoView({block:'start'});await sleep(300);
          // Synthetic sessions with subagents (the fixture has no live sessions): long names, every state, more than four recent.
          const sid=i=>`0000000${i}-1c2d-4e5f-8a9b-0c1d2e3f4a5b`,mk=(i,state,age)=>({id:'a'+i,kind:i%2?'general-purpose':'Explore',description:'Review the very long synthetic description of what this subagent was asked to do here',state,depth:1+(i%3),age,tokens:i%3?null:{input:1234567,output:89012,cache_creation:3456,cache_read:9876543},tokens_why:'running; counted when it finishes'});
          const rows=[0,1,2].map(i=>({id:sid(i),display_name:'Example long-named coordinator '+i,label:'',project:'a-very-long-synthetic-project-directory-name',provider:'claude',status:'running',running:true,idle_ms:0}));
          window.AppFlow.show(rows,{note:'',sessions:[{session:sid(0),skipped:2,truncated:true,more_files:true,search_truncated:false,found:true,subs:[mk(1,'running',5),mk(2,'running',30),mk(3,'finished',120),mk(4,'stale',900),mk(5,'finished',1800),mk(6,'finished',7200)]},{session:sid(1),skipped:0,truncated:false,subs:[mk(7,'finished',60)]},{session:sid(2),skipped:0,truncated:false,subs:[]}]});
          await sleep(200);if($('flow-svg').querySelectorAll('.flow-subnode').length!==5)throw new Error('subagent nodes');
          // Pan and zoom with synthetic wheel, pointer and key events (real mouse input cannot run headless).
          {const svg=$('flow-svg'),F=window.AppFlow,r=svg.getBoundingClientRect(),cx=r.left+r.width/2,cy=r.top+r.height/2,num=()=>F.view().split(' ').map(Number);
           const fitV=F.view();if(Math.abs(F.zoom()-1)>1e-6)throw new Error('starts fitted');
           svg.dispatchEvent(new WheelEvent('wheel',{deltaY:-300,clientX:cx,clientY:cy,bubbles:true,cancelable:true}));if(!(F.zoom()>1.4))throw new Error('wheel zoom '+F.zoom());
           const before=num();svg.dispatchEvent(new PointerEvent('pointerdown',{pointerId:7,button:0,buttons:1,clientX:cx,clientY:cy,bubbles:true}));
           svg.dispatchEvent(new PointerEvent('pointermove',{pointerId:7,buttons:1,clientX:cx+80,clientY:cy+40,bubbles:true}));
           if(!svg.classList.contains('dragging'))throw new Error('grabbing cursor');
           svg.dispatchEvent(new PointerEvent('pointerup',{pointerId:7,button:0,clientX:cx+80,clientY:cy+40,bubbles:true}));
           const after=num();if(!(after[0]<before[0]&&after[1]<before[1])||after[2]!==before[2])throw new Error('drag pan '+before+' -> '+after);if(svg.classList.contains('dragging'))throw new Error('drag ended');
           const kept=F.view();window.AppFlow.show(rows,{note:'',sessions:[{session:sid(0),skipped:2,truncated:true,more_files:true,search_truncated:false,found:true,subs:[mk(1,'running',5),mk(2,'running',30),mk(3,'finished',120),mk(4,'stale',900),mk(5,'finished',1800),mk(6,'finished',7200)]},{session:sid(1),skipped:0,truncated:false,subs:[mk(7,'finished',60)]},{session:sid(2),skipped:0,truncated:false,subs:[]}]});
           if(F.view()!==kept)throw new Error('view kept across a redraw');
           await sleep(20); // the click that ends a drag is swallowed only within the same task
           // A press that moves 2 px and is released outside the diagram ends the drag: a later move with no button must not pan.
           {const v=F.view();svg.dispatchEvent(new PointerEvent('pointerdown',{pointerId:9,button:0,buttons:1,clientX:cx,clientY:cy,bubbles:true}));
            svg.dispatchEvent(new PointerEvent('pointermove',{pointerId:9,buttons:1,clientX:cx+2,clientY:cy,bubbles:true}));
            document.body.dispatchEvent(new PointerEvent('pointerup',{pointerId:9,button:0,buttons:0,clientX:5,clientY:5,bubbles:true}));
            svg.dispatchEvent(new PointerEvent('pointermove',{pointerId:9,buttons:0,clientX:cx+120,clientY:cy+60,bubbles:true}));
            if(F.view()!==v||svg.classList.contains('dragging'))throw new Error('release outside ends the drag');
            svg.dispatchEvent(new PointerEvent('pointerdown',{pointerId:10,button:0,buttons:1,clientX:cx,clientY:cy,bubbles:true}));
            svg.dispatchEvent(new PointerEvent('pointermove',{pointerId:10,buttons:0,clientX:cx+120,clientY:cy+60,bubbles:true}));
            svg.dispatchEvent(new PointerEvent('pointermove',{pointerId:10,buttons:0,clientX:cx+200,clientY:cy+90,bubbles:true}));
            if(F.view()!==v)throw new Error('a move with no button pressed must not pan');}
           let clicked=0;const node=svg.querySelector('.flow-sess');node.addEventListener('click',()=>clicked++,{once:true});
           svg.dispatchEvent(new PointerEvent('pointerdown',{pointerId:8,button:0,buttons:1,clientX:cx,clientY:cy,bubbles:true}));svg.dispatchEvent(new PointerEvent('pointerup',{pointerId:8,button:0,clientX:cx+1,clientY:cy,bubbles:true}));
           node.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true}));if(clicked!==1)throw new Error('a click without movement reaches the node');
           for(let i=0;i<40;i++)svg.dispatchEvent(new WheelEvent('wheel',{deltaY:-500,ctrlKey:true,clientX:cx,clientY:cy,bubbles:true,cancelable:true}));if(F.zoom()>4+1e-6)throw new Error('zoom max');
           for(let i=0;i<40;i++)$('flow-zoom-out').click();if(F.zoom()<0.25-1e-6)throw new Error('zoom min');
           svg.focus();svg.dispatchEvent(new KeyboardEvent('keydown',{key:'0',bubbles:true,cancelable:true}));if(F.view()!==fitV)throw new Error('0 fits');
           const k0=num();svg.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true,cancelable:true}));if(!(num()[0]>k0[0]))throw new Error('arrow pans');
           svg.dispatchEvent(new KeyboardEvent('keydown',{key:'+',bubbles:true,cancelable:true}));if(!(F.zoom()>1.2))throw new Error('plus zooms');
           $('flow-reset').click();if(Math.abs(F.zoom()-1)>1e-6||num()[0]!==0)throw new Error('reset');
           $('flow-fit').click();if(F.view()!==fitV)throw new Error('fit button');
           report('panzoom','ok');}if($('subs-body').children.length!==7)throw new Error('subagent rows');
          report('subs',`nodes=${$('flow-svg').querySelectorAll('.flow-subnode').length} rows=${$('subs-body').children.length}`);
          await check('flow-subs',scale);$('flow-svg').scrollIntoView({block:'start'});await sleep(300);report('capture',`flowsubs-${scale}`);await sleep(1300);
          $('tasks-table').closest('.card').scrollIntoView({block:'start'});await sleep(300);}
        report('capture',`${name}-${scale}`);await sleep(1300);
      }
    }
    report('result','pass');
  }catch(e){report('result',`fail ${e.message}`);}
})();
