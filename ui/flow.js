'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const NS='http://www.w3.org/2000/svg',IDLE_MS=10*60*1000,SUB_RECENT_S=3600,SUB_SHOWN=4,SUB_H=22;
  let timer=null;
  // Pan and zoom work on the SVG viewBox. `content` is the drawn size, `view` the visible part; zoom 1 means 900 units across.
  const W=900,ZMIN=0.25,ZMAX=4;
  let content={w:W,h:120},view=null,moved=false,lastIds=[];
  const svgEl=()=>$('flow-svg');
  const apply=()=>{if(view)svgEl().setAttribute('viewBox',`${view.x} ${view.y} ${view.w} ${view.h}`);};
  const fit=()=>{view={x:0,y:0,w:content.w,h:content.h};moved=false;apply();};
  const reset=()=>{const r=svgEl().getBoundingClientRect();const h=r.width>0?W*r.height/r.width:content.h;view={x:0,y:0,w:W,h};moved=true;apply();};
  // Point in diagram units under a client position (handles the letterboxing of preserveAspectRatio).
  const toSvg=(cx,cy)=>{const m=svgEl().getScreenCTM();if(!m)return {x:view.x+view.w/2,y:view.y+view.h/2};const p=new DOMPoint(cx,cy).matrixTransform(m.inverse());return {x:p.x,y:p.y};};
  function zoomAt(f,cx,cy){
    if(!view)fit();
    const z=W/view.w,nz=Math.min(ZMAX,Math.max(ZMIN,z*f));if(nz===z)return;const k=z/nz;
    const p=cx==null?{x:view.x+view.w/2,y:view.y+view.h/2}:toSvg(cx,cy);
    view={x:p.x-(p.x-view.x)*k,y:p.y-(p.y-view.y)*k,w:view.w*k,h:view.h*k};moved=true;apply();
  }
  function panBy(dx,dy){if(!view)fit();view={...view,x:view.x+dx,y:view.y+dy};moved=true;apply();}
  const el=(n,attrs={},text)=>{const e=document.createElementNS(NS,n);for(const [k,v] of Object.entries(attrs))e.setAttribute(k,v);if(text!=null)e.textContent=text;return e;};
  // Process status only (what the app observes); never an approval state.
  function state(a){
    if(!a.running){return a.status==='exited'&&a.exit_info&&a.exit_info!=='exit 0'?['error','stopped with '+a.exit_info]:a.status==='interrupted'?['error','was running when the app closed']:['stopped',a.status];}
    if(a.idle_ms!=null&&a.idle_ms>IDLE_MS)return ['quiet',`no output for ${Math.round(a.idle_ms/60000)} min`];
    return ['running','running'];
  }
  const short=(s,n=26)=>s.length>n?s.slice(0,n-1)+'…':s;
  const ago=s=>s==null?'?':s<60?`${s}s ago`:s<3600?`${Math.round(s/60)} min ago`:s<86400?`${Math.round(s/3600)} h ago`:`${Math.round(s/86400)} d ago`;
  // Subagents drawn under a session: running ones and those active in the last hour, at most four, then "+N more".
  const shown=list=>{const recent=(list||[]).filter(s=>s.state==='running'||(s.age!=null&&s.age<=SUB_RECENT_S));return [recent.slice(0,SUB_SHOWN),Math.max(0,recent.length-SUB_SHOWN)];};
  function draw(rows,subs){
    const svg=$('flow-svg');svg.replaceChildren();const fl=$('flow-list');fl.replaceChildren();
    for(const a of rows){const li=document.createElement('li');const [vis,more]=shown(subs[a.id]);li.textContent=`${a.display_name}${a.project?' ('+a.project+')':''}: ${state(a)[1]}${vis.length?`, ${vis.length+more} recent subagent${vis.length+more===1?'':'s'}`:''}`;fl.append(li);}
    const provs=['claude','codex'].map(p=>[p,rows.filter(r=>r.provider===p)]).filter(x=>x[1].length);
    $('flow-empty').hidden=rows.length>0;$('flow-wrap').hidden=!rows.length;if(!rows.length){lastIds=[];return;}
    const rowH=46,top=20;
    // Each session row is taller by one line per subagent shown under it.
    const lay=new Map();let y=top;for(const [,list] of provs)for(const a of list){const [vis,more]=shown(subs[a.id]);const h=rowH+(vis.length+(more?1:0))*SUB_H;lay.set(a.id,{sy:y+rowH/2-6,vis,more});y+=h;}
    const H=Math.max(y+top,120);content={w:W,h:H};
    // Keep the user's view across refreshes; refit when nothing was moved yet, or when the session set changed a lot.
    const ids=rows.map(a=>a.id),prev=new Set(lastIds),changed=ids.filter(i=>!prev.has(i)).length+lastIds.filter(i=>!ids.includes(i)).length;
    if(!view||!moved||changed>Math.max(2,lastIds.length/2))fit();else apply();lastIds=ids;
    const youY=H/2,provY={};
    for(const [p,list] of provs){const ys=list.map(a=>lay.get(a.id).sy);provY[p]=(Math.min(...ys)+Math.max(...ys))/2;}
    const edge=(x1,y1,x2,y2,cls)=>svg.append(el('path',{d:`M${x1} ${y1} C ${(x1+x2)/2} ${y1}, ${(x1+x2)/2} ${y2}, ${x2} ${y2}`,class:'flow-edge '+(cls||''),fill:'none'}));
    const corner=parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--r-ctl'))||0;
    const box=(x,y,w,label,sub,cls,tip)=>{const g=el('g',{class:'flow-node '+(cls||'')});g.append(el('title',{},tip||label));g.append(el('rect',{x,y:y-18,width:w,height:36,rx:corner}));g.append(el('text',{x:x+12,y:y-2,class:'flow-label'},label));if(sub)g.append(el('text',{x:x+12,y:y+12,class:'flow-sub'},sub));svg.append(g);return g;};
    for(const [p] of provs){edge(210,youY,300,provY[p]);}
    for(const [p,list] of provs){for(const a of list)edge(450,provY[p],600,lay.get(a.id).sy,state(a)[0]);}
    box(20,youY,190,'You','decides in the terminal','flow-you');
    for(const [p,list] of provs)box(300,provY[p],150,p==='claude'?'Claude Code':'Codex',`${list.length} session${list.length===1?'':'s'}`,'flow-prov');
    for(const [,list] of provs){for(const a of list){const {sy,vis,more}=lay.get(a.id);const [k,why]=state(a);
      const g=box(600,sy,280,short(a.display_name),short(a.project||''),'flow-sess','');g.dataset.state=k;g.dataset.id=a.id;g.querySelector('title').textContent=`${a.display_name}: ${why}`;
      g.append(el('circle',{cx:866,cy:sy-4,r:6,class:'flow-dot '+k}));
      // Subagents: small square nodes branching from the session node's left edge.
      vis.forEach((s,i)=>{const yy=sy+18+SUB_H*(i+1)-8;svg.append(el('path',{d:`M616 ${sy+18} V ${yy} H 636`,class:'flow-edge '+(s.state==='running'?'running':''),fill:'none'}));
        const n=el('g',{class:'flow-subnode '+s.state});n.append(el('title',{},`${s.kind}, depth ${s.depth}, ${s.state}, ${ago(s.age)}: ${s.description}`));
        n.append(el('rect',{x:636,y:yy-9,width:244,height:18,rx:corner}));n.append(el('text',{x:644,y:yy+4,class:'flow-subtext'},short(`${s.kind} · ${s.state} · ${s.description}`,33)));svg.append(n);});
      if(more){const yy=sy+18+SUB_H*(vis.length+1)-8;svg.append(el('text',{x:644,y:yy+4,class:'flow-subtext'},`+${more} more (see Subagents below)`));}
    }}
  }
  const tok=s=>s.tokens?`${(s.tokens.input+s.tokens.output+s.tokens.cache_creation).toLocaleString()} (+${s.tokens.cache_read.toLocaleString()} cache read)`:'n/a';
  function table(rows,rep){
    const body=$('subs-body');body.replaceChildren();const name=new Map(rows.map(a=>[a.id,a.display_name]));let n=0,skipped=0,trunc=false,many=false,unsearched=0;
    for(const ss of rep.sessions){skipped+=ss.skipped;trunc=trunc||ss.truncated;many=many||ss.more_files;if(ss.search_truncated)unsearched++;for(const s of ss.subs){if(n>=150)break;n++;const tr=document.createElement('tr');
      for(const [v,t] of [[name.get(ss.session)||'?'],[s.kind],[s.description],[s.state],[String(s.depth)],[ago(s.age)],[tok(s),s.tokens?'':s.tokens_why]]){const td=document.createElement('td');td.textContent=v;if(t)td.title=t;tr.append(td);}body.append(tr);}}
    $('subs-empty').hidden=n>0;$('subs-table').hidden=!n;
    $('subs-note').textContent=[skipped?`${skipped} file${skipped===1?'':'s'} skipped (not the expected shape, too big, or a symlink).`:'',trunc?'Only the newest 200 per session are listed.':'',many?'A subagents folder has more than 20,000 files; only the first 20,000 in folder order were looked at.':'',unsearched?`${unsearched} session${unsearched===1?' was':'s were'} not found because Claude's projects folder has more than 5,000 folders.`:'',n>=150?'Showing the first 150.':''].filter(Boolean).join(' ');
    $('subs-note').title=rep.note||'';
  }
  async function load(){
    let rows;try{rows=await invoke('live_activity');}catch(e){$('flow-note').textContent=String(e);return;}
    rows.sort((a,b)=>a.provider.localeCompare(b.provider)||a.display_name.localeCompare(b.display_name));
    $('flow-note').textContent='Process status from what the app can observe, and the subagents Claude Code recorded for each session. Decisions and approvals are made in each terminal.';
    let rep={sessions:[],note:''};
    const ids=rows.filter(a=>a.provider==='claude').map(a=>a.id).slice(0,32);
    if(ids.length){try{rep=await invoke('subagents_report',{sessions:ids});}catch(e){$('subs-note').textContent=String(e);}}
    const subs={};for(const ss of rep.sessions)subs[ss.session]=ss.subs;
    draw(rows,subs);table(rows,rep);
    try{
      const tl=await AppIPC.invoke('timeline_list');const body=$('flow-tl-body');body.replaceChildren();
      $('flow-tl-empty').hidden=tl.length>0;$('flow-tl').hidden=!tl.length;
      for(const e of tl.slice(0,60)){const tr=document.createElement('tr');for(const v of [new Date(e.t*1000).toLocaleTimeString(),e.name,e.event]){const td=document.createElement('td');td.textContent=v;tr.append(td);}body.append(tr);}
    }catch{}
  }
  const start=()=>{stop();load();timer=setInterval(()=>{if(!document.hidden)load();},5000);};
  const stop=()=>{if(timer){clearInterval(timer);timer=null;}};
  $('flow-reload').onclick=load;
  // Wheel (and Ctrl+wheel, which is how a touchpad pinch arrives) zooms around the pointer, only over the diagram.
  const svg0=svgEl();
  svg0.addEventListener('wheel',e=>{e.preventDefault();const d=e.deltaY*(e.deltaMode===1?16:e.deltaMode===2?400:1);zoomAt(Math.exp(-d*(e.ctrlKey?0.01:0.0015)),e.clientX,e.clientY);},{passive:false});
  // Left-button drag pans. Nothing is captured until the pointer moves 4 px, so a plain click still reaches nodes and tooltips.
  let drag=null,swallow=false;
  svg0.addEventListener('pointerdown',e=>{if(e.button!==0)return;if(!view)fit();const m=svgEl().getScreenCTM();drag={x:e.clientX,y:e.clientY,v:{...view},a:m?m.a:1,id:e.pointerId,live:false};});
  svg0.addEventListener('pointermove',e=>{if(!drag)return;if(e.buttons===0){end();return;}const dx=e.clientX-drag.x,dy=e.clientY-drag.y;
    if(!drag.live){if(Math.hypot(dx,dy)<4)return;drag.live=true;svg0.classList.add('dragging');try{svg0.setPointerCapture(drag.id);}catch{}}
    view={...drag.v,x:drag.v.x-dx/drag.a,y:drag.v.y-dy/drag.a};moved=true;apply();});
  const end=()=>{if(!drag)return;if(drag.live){swallow=true;setTimeout(()=>{swallow=false;},0);}try{svg0.releasePointerCapture(drag.id);}catch{}drag=null;svg0.classList.remove('dragging');};
  svg0.addEventListener('pointerup',end);svg0.addEventListener('pointercancel',end);svg0.addEventListener('lostpointercapture',end);
  // A button released outside the diagram before the drag started (nothing captured yet) still ends it.
  window.addEventListener('pointerup',end);window.addEventListener('pointercancel',end);window.addEventListener('blur',end);
  svg0.addEventListener('click',e=>{if(swallow){e.stopPropagation();e.preventDefault();}},true);
  svg0.addEventListener('keydown',e=>{const step=view?view.w*0.1:90;const k={'+':()=>zoomAt(1.25),'=':()=>zoomAt(1.25),'-':()=>zoomAt(0.8),'_':()=>zoomAt(0.8),'0':fit,
    ArrowLeft:()=>panBy(-step,0),ArrowRight:()=>panBy(step,0),ArrowUp:()=>panBy(0,-step),ArrowDown:()=>panBy(0,step)}[e.key];if(k){e.preventDefault();k();}});
  $('flow-zoom-in').onclick=()=>zoomAt(1.25);$('flow-zoom-out').onclick=()=>zoomAt(0.8);$('flow-fit').onclick=fit;$('flow-reset').onclick=reset;
  // Layout tests draw synthetic rows here; it stops the refresh timer until the page is opened again.
  window.AppFlow={show(rows,rep){stop();const subs={};for(const ss of rep.sessions)subs[ss.session]=ss.subs;draw(rows,subs);table(rows,rep);},view:()=>svgEl().getAttribute('viewBox'),zoom:()=>view?W/view.w:1};
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='flow')start();else stop();});
})();
