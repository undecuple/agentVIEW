'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  const NS='http://www.w3.org/2000/svg',IDLE_MS=10*60*1000;
  let timer=null;
  const el=(n,attrs={},text)=>{const e=document.createElementNS(NS,n);for(const [k,v] of Object.entries(attrs))e.setAttribute(k,v);if(text!=null)e.textContent=text;return e;};
  // Process status only (what the app observes); never an approval state.
  function state(a){
    if(!a.running){return a.status==='exited'&&a.exit_info&&a.exit_info!=='exit 0'?['error','stopped with '+a.exit_info]:a.status==='interrupted'?['error','was running when the app closed']:['stopped',a.status];}
    if(a.idle_ms!=null&&a.idle_ms>IDLE_MS)return ['quiet',`no output for ${Math.round(a.idle_ms/60000)} min`];
    return ['running','running'];
  }
  const short=s=>s.length>26?s.slice(0,25)+'…':s;
  function draw(rows){
    const svg=$('flow-svg');svg.replaceChildren();const fl=$('flow-list');fl.replaceChildren();for(const a of rows){const li=document.createElement('li');li.textContent=`${a.display_name}${a.project?' ('+a.project+')':''}: ${state(a)[1]}`;fl.append(li);}
    const provs=['claude','codex'].map(p=>[p,rows.filter(r=>r.provider===p)]).filter(x=>x[1].length);
    $('flow-empty').hidden=rows.length>0;svg.hidden=!rows.length;if(!rows.length)return;
    const rowH=46,top=20,count=rows.length;
    const H=Math.max(count*rowH+top*2,120);svg.setAttribute('viewBox',`0 0 900 ${H}`);svg.style.height=H+'px';
    const youY=H/2,provY={};
    let idx=0;for(const [p,list] of provs){provY[p]=top+rowH*(idx+(list.length-1)/2)+rowH/2-6;idx+=list.length;}
    const edge=(x1,y1,x2,y2,cls)=>svg.append(el('path',{d:`M${x1} ${y1} C ${(x1+x2)/2} ${y1}, ${(x1+x2)/2} ${y2}, ${x2} ${y2}`,class:'flow-edge '+(cls||''),fill:'none'}));
    const corner=parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--r-ctl'))||0;
    const box=(x,y,w,label,sub,cls,tip)=>{const g=el('g',{class:'flow-node '+(cls||'')});g.append(el('title',{},tip||label));g.append(el('rect',{x,y:y-18,width:w,height:36,rx:corner}));g.append(el('text',{x:x+12,y:y-2,class:'flow-label'},label));if(sub)g.append(el('text',{x:x+12,y:y+12,class:'flow-sub'},sub));svg.append(g);return g;};
    for(const [p] of provs){edge(150,youY,300,provY[p]);}
    let i=0;for(const [p,list] of provs){for(const a of list){const sy=top+rowH*i+rowH/2-6;edge(450,provY[p],600,sy,state(a)[0]);i++;}}
    box(20,youY,130,'You','decides in the terminal','flow-you');
    for(const [p,list] of provs)box(300,provY[p],150,p==='claude'?'Claude Code':'Codex',`${list.length} session${list.length===1?'':'s'}`,'flow-prov');
    i=0;for(const [p,list] of provs){for(const a of list){const sy=top+rowH*i+rowH/2-6;const [k,why]=state(a);
      const g=box(600,sy,280,short(a.display_name),short(a.project||''),'flow-sess','');g.dataset.state=k;g.dataset.id=a.id;g.querySelector('title').textContent=`${a.display_name}: ${why}`;
      g.append(el('circle',{cx:866,cy:sy-4,r:6,class:'flow-dot '+k}));i++;}}
  }
  async function load(){
    let rows;try{rows=await invoke('live_activity');}catch(e){$('flow-note').textContent=String(e);return;}
    rows.sort((a,b)=>a.provider.localeCompare(b.provider)||a.display_name.localeCompare(b.display_name));
    $('flow-note').textContent='Process status only, from what the app can observe. Decisions and approvals are made in each terminal, and sub-agents are not visible here.';
    draw(rows);
    try{
      const tl=await AppIPC.invoke('timeline_list');const body=$('flow-tl-body');body.replaceChildren();
      $('flow-tl-empty').hidden=tl.length>0;$('flow-tl').hidden=!tl.length;
      for(const e of tl.slice(0,60)){const tr=document.createElement('tr');for(const v of [new Date(e.t*1000).toLocaleTimeString(),e.name,e.event]){const td=document.createElement('td');td.textContent=v;tr.append(td);}body.append(tr);}
    }catch{}
  }
  const start=()=>{stop();load();timer=setInterval(()=>{if(!document.hidden)load();},5000);};
  const stop=()=>{if(timer){clearInterval(timer);timer=null;}};
  $('flow-reload').onclick=load;
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='flow')start();else stop();});
})();
