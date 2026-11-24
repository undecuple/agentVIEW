// Test-only diagnostic: reports real layout facts (run on a real display to compare with the virtual one).
(async()=>{
  const report=(k,v)=>{new Image().src=`/__workspace-report?${k}=${encodeURIComponent(v)}`;};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  try{
    await sleep(6000);
    const cs=(sel,props)=>{const e=document.querySelector(sel);if(!e)return sel+':none';const s=getComputedStyle(e),r=e.getBoundingClientRect();return `${sel}: x=${r.x|0} w=${r.width|0} h=${r.height|0} `+props.map(p=>p+'='+s.getPropertyValue(p)).join(' ');};
    report('check','viewport inner='+innerWidth+'x'+innerHeight+' client='+document.documentElement.clientWidth+' outer='+outerWidth+' dpr='+devicePixelRatio+' vv='+(visualViewport?visualViewport.width+'x'+visualViewport.scale:'n/a'));
    report('check',cs('.rail',['position','width','display']));
    report('check',cs('main',['margin-left','width','display','position']));
    report('check',cs('body',['display','width']));
    report('check','media800='+matchMedia('(max-width:800px)').matches+' media1200='+matchMedia('(max-width:1200px)').matches);
    report('check','sheets='+[...document.styleSheets].map(s=>{let n='?';try{n=s.cssRules.length;}catch{}return (s.href||'inline').split('/').pop()+':'+n;}).join(','));
    report('check',cs('.nav-label',['display']));
    report('result','pass');
  }catch(e){report('result','fail:'+e.message);}
})();
