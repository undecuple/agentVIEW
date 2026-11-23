'use strict';
(() => {
  const $=id=>document.getElementById(id),invoke=(c,a={})=>AppIPC.invoke(c,a);
  let prev=new Map(),rows=[],tokens=new Map(),timer=null;
  const fmtB=n=>n<1024?`${Math.round(n)} B`:n<1048576?`${(n/1024).toFixed(1)} KB`:`${(n/1048576).toFixed(1)} MB`;
  const fmtN=n=>n==null?'—':n>=1e6?`${(n/1e6).toFixed(2)}M`:n>=1e3?`${(n/1e3).toFixed(1)}k`:String(n);
  const ago=ms=>ms==null?'—':ms<1500?'now':ms<60000?`${Math.round(ms/1000)}s ago`:ms<3600000?`${Math.round(ms/60000)} min ago`:`${Math.round(ms/3600000)} h ago`;
  const agoIso=s=>{if(!s)return null;const t=Date.parse(s);return Number.isNaN(t)?null:Math.max(0,Date.now()-t);};
  function cell(text,cls){const td=document.createElement('td');if(cls)td.className=cls;td.textContent=text;return td;}
  function sample(list){
    const now=performance.now(),next=new Map();
    for(const a of list){
      const p=prev.get(a.id);let out=0,inn=0;
      if(p&&a.running&&now>p.t){const dt=(now-p.t)/1000;out=Math.max(0,a.output_total-p.o)/dt;inn=Math.max(0,a.input_total-p.i)/dt;}
      next.set(a.id,{t:now,o:a.output_total,i:a.input_total,out,inn});
    }
    prev=next;
  }
  const IDLE_ATTENTION_MS=10*60*1000;
  function attention(a){
    if(a.status==='exited'&&a.exit_info&&a.exit_info!=='exit 0')return `ended with ${a.exit_info}`;
    if(a.status==='interrupted')return 'was running when the app closed';
    if(a.status==='bootstrap_unknown')return 'setup was interrupted';
    if(a.running&&a.idle_ms!=null&&a.idle_ms>IDLE_ATTENTION_MS)return `no output for ${Math.round(a.idle_ms/60000)} min`;
    return '';
  }
  function render(){
    const prov=$('ov-provider').value,state=$('ov-state').value,text=$('ov-text').value.trim().toLowerCase(),sort=$('ov-sort').value;
    let list=rows.filter(a=>(!prov||a.provider===prov)&&(!state||(state==='attention'?!!attention(a):(state==='running')===a.running))&&(!text||(a.display_name+' '+a.project).toLowerCase().includes(text)));
    const rate=a=>{const r=prev.get(a.id);return r?r.out+r.inn:0;},tok=a=>tokens.get(a.id)??-1;
    list.sort((a,b)=>sort==='name'?a.display_name.localeCompare(b.display_name):sort==='flow'?rate(b)-rate(a):sort==='tokens'?tok(b)-tok(a):(a.idle_ms??1e12)-(b.idle_ms??1e12));
    const body=$('ov-body');body.replaceChildren();
    for(const a of list){
      const tr=document.createElement('tr');tr.dataset.id=a.id;tr.dataset.running=String(a.running);
      tr.append(cell(a.display_name,'ov-name'),cell(a.running?`Running${a.idle_ms==null?'':' · output '+ago(a.idle_ms)}`:'Not running',a.running?'ov-run':'ov-stop'),
        cell(a.project?`${a.label} · ${a.project}`:a.label));
      const act=a.action?`${a.action}${a.action_at?' · '+ago(agoIso(a.action_at)):''}`:(a.provider==='codex'?'Not reported by Codex':a.running?'No tool use seen yet':'—');
      tr.append(cell(act));
      const r=prev.get(a.id)||{out:0,inn:0};
      tr.append(cell(a.running?`${fmtB(r.out)}/s out · ${fmtB(r.inn)}/s in`:'—'));
      tr.append(cell(fmtN(tokens.get(a.id))));
      body.append(tr);
    }
    const need=rows.map(a=>[a,attention(a)]).filter(x=>x[1]);
    const box=$('ov-attention');box.replaceChildren();
    if(!need.length){const p=document.createElement('p');p.className='muted';p.textContent='Nothing needs attention.';box.append(p);}
    for(const [a,why] of need){const p=document.createElement('p');const b=document.createElement('strong');b.textContent=a.display_name;p.append(b,' '+why);box.append(p);}
    {const runningRows=rows.filter(a=>a.running);let out=0;for(const a of runningRows)out+=(prev.get(a.id)||{out:0}).out;
     let tk=0,any=false;for(const a of rows){const v=tokens.get(a.id);if(v!=null){tk+=v;any=true;}}
     $('st-running').textContent=String(runningRows.length);$('st-running-sub').textContent=`of ${rows.length} registered`;
     $('st-attention').textContent=String(need.length);$('st-attention').dataset.bad=String(need.length>0);
     $('st-flow').textContent=`${fmtB(out)}/s`;$('st-tokens').textContent=any?fmtN(tk):'—';}
    const running=rows.filter(a=>a.running).length;
    $('ov-stopall').disabled=!running;$('ov-stopall').textContent=running?`Stop all running (${running})`:'Stop all running';
    $('ov-summary').textContent=rows.length?`${list.length} shown of ${rows.length} registered · ${running} running`:'No sessions are registered yet. Add one on the Sessions page.';
  }
  async function poll(){
    if(document.hidden||$('overview').hidden)return;
    try{
      const [act,use]=await Promise.all([invoke('live_activity'),invoke('usage_snapshot').catch(()=>({sessions:[]}))]);
      rows=act;tokens=new Map((use.sessions||[]).map(s=>[s.app_session_id,s.totals&&s.totals.total_tokens]));
      sample(rows);render();$('ov-updated').textContent='Updated '+new Date().toLocaleTimeString();
    }catch(e){$('ov-summary').textContent=String(e);}
  }
  $('ov-stopall').onclick=()=>{$('ov-stopall').hidden=true;$('ov-stopall-confirm').hidden=false;$('ov-stopall-cancel').hidden=false;};
  const resetStop=()=>{$('ov-stopall').hidden=false;$('ov-stopall-confirm').hidden=true;$('ov-stopall-cancel').hidden=true;};
  $('ov-stopall-cancel').onclick=resetStop;
  $('ov-stopall-confirm').onclick=async()=>{resetStop();try{const n=await invoke('live_stop_all');$('ov-summary').textContent=`Stopped ${n} running session${n===1?'':'s'}.`;}catch(e){$('ov-summary').textContent=String(e);}await poll();};
  function start(){clearInterval(timer);poll();timer=setInterval(poll,2000);}
  function stop(){clearInterval(timer);timer=null;}
  for(const id of ['ov-provider','ov-state','ov-text','ov-sort'])$(id).addEventListener('input',render);
  window.addEventListener('agentview:navigate',e=>{if(e.detail==='overview')start();else stop();});
  document.addEventListener('visibilitychange',()=>{if(!document.hidden&&!$('overview').hidden)poll();});
})();
