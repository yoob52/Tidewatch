// 小工具与共享片段
const $=(s,r=document)=>r.querySelector(s);
const $$=(s,r=document)=>[...r.querySelectorAll(s)];
const esc=s=>String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const state={theme:localStorage.getItem('tw-theme')||'light',limit:3,boardFilter:'all',listFilter:'all',note:true};

function toast(msg){const box=$('#toasts'),el=document.createElement('div');el.className='toast';el.textContent=msg;while(box.children.length>1)box.firstChild.remove();box.append(el);setTimeout(()=>el.remove(),3200)}
function stamp(type,lg){const t=TYPES[type];return `<span class="stamp ${t.cls}${lg?' lg':''}" title="${t.name}">${t.ch}</span>`}
function prio(p){return `<span class="prio ${p}"><i></i>${PRIO[p]}</span>`}
function agentTag(a){const g=AGENTS[a];return `<span class="agent"><span class="glyph ${g.g}">${g.short}</span>${g.name}</span>`}
function agentGlyph(a){const g=AGENTS[a];return `<span class="agent" title="${g.name}"><span class="glyph ${g.g}">${g.short}</span></span>`}
const ripple='<span class="ripple"><i></i><i></i><i></i><i></i></span>';
function fmtDur(s){const m=Math.floor(s/60),x=s%60;return `${String(m).padStart(2,'0')}:${String(x).padStart(2,'0')}`}
function fmtTok(n){return n>=1e6?(n/1e6).toFixed(1)+'M':n>=1e3?(n/1e3).toFixed(1)+'k':String(n)}
const SUB={queued:['排队','dim'],blocked:['等依赖','dim'],running:['运行中','tide'],review:['待验收','cin'],accepted:['已通过','moss'],changes:['已打回','amber'],merged:['已合并','moss'],pending:['待开始','dim']};
function subChips(t){
  if(!t.sub)return '';
  return t.sub.map(([k,s])=>{const[n,c]=SUB[s];
    const mark=s==='running'?ripple:s==='accepted'||s==='merged'?I('check',11):s==='review'?'<span class="dot"></span>':s==='changes'?I('undo',11):'';
    return `<span class="subchip ${c}" title="${k} · ${n}"><b>${k}</b>${mark}<span>${n}</span></span>`}).join('');
}
// 极简 Markdown：标题、列表、表格之外的段落、行内 code / 加粗
function inline(s){return esc(s).replace(/`([^`]+)`/g,'<code>$1</code>').replace(/\*\*([^*]+)\*\*/g,'<strong>$1</strong>')}
function md(src,marks=[]){
  const out=[];let ul=false;
  const hl=s=>{let h=inline(s);marks.forEach(m=>{const k=esc(m.loc);if(h.includes(k))h=h.replace(k,`<mark class="m-${m.sev} ${m.state!=='open'&&m.state!=='pending'?'closed':''}" data-issue="${m.id}">${k}<sup>${m.id.slice(2)}</sup></mark>`)});return h};
  src.split('\n').forEach(l=>{
    if(/^- /.test(l)){if(!ul){out.push('<ul>');ul=true}out.push('<li>'+hl(l.slice(2))+'</li>');return}
    if(ul){out.push('</ul>');ul=false}
    if(/^### /.test(l))out.push('<h4>'+hl(l.slice(4))+'</h4>');
    else if(/^## /.test(l))out.push('<h3>'+hl(l.slice(3))+'</h3>');
    else if(/^# /.test(l))out.push('<h2>'+hl(l.slice(2))+'</h2>');
    else if(l.trim())out.push('<p>'+hl(l)+'</p>');
  });
  if(ul)out.push('</ul>');
  return out.join('');
}
function counts(){
  const running=RUNS.filter(r=>r.status==='running').length,queued=RUNS.filter(r=>r.status==='queued').length;
  const mine=TICKETS.filter(t=>STAGE[t.status].mine).length;
  return{running,queued,mine};
}
function wave(level){ // level 0..1 → 水面高度
  const y=40-level*30,W='q15 -5 30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0';
  return `<svg viewBox="0 0 186 46" preserveAspectRatio="none"><line class="cap" x1="0" x2="186" y1="8" y2="8"/><path class="w1" d="M0 ${y+2} ${W} V46 H0Z"/><path class="w2" d="M-20 ${y+5} ${W} V46 H-20Z"/></svg>`;
}
function shell(active,title,crumb){
  const c=counts();
  const nav=(id,href,icon,name,n,live)=>`<a href="${href}" class="${active===id?'on':''}">${I(icon)}<span>${name}</span>${n!==undefined?`<span class="n ${live?'live':''}">${live?'<span class="pulse"></span>':''}${n}</span>`:''}</a>`;
  return `<div class="app">
  <aside class="side">
    <a class="brand" href="#/board"><span class="seal">观澜</span><span><b>观澜</b><small>Tidewatch</small></span></a>
    <nav class="nav">
      <div class="nav-group">工作</div>
      ${nav('board','#/board','board','看板',c.mine+' 待办')}
      ${nav('tickets','#/tickets','list','工单',TICKETS.length)}
      ${nav('runs','#/runs','runs','运行',c.running,c.running>0)}
      <div class="nav-group">配置</div>
      ${nav('projects','#/projects','folder','项目',PROJECTS.length)}
      ${nav('providers','#/providers','plug','Provider',PROVIDERS.length)}
      ${nav('stats','#/stats','chart','统计')}
      ${nav('settings','#/settings','gear','设置')}
    </nav>
    <div class="side-foot">
      <a class="gauge" href="#/runs" title="运行队列">
        <div class="gauge-head"><span>潮位 · 并发</span><b>${c.running}<i> / ${state.limit}</i></b></div>
        ${wave(Math.min(1,c.running/state.limit))}
        <div class="gauge-rows"><div><span>排队</span><b>${c.queued}</b></div><div><span>今日费用</span><b>$ 2.58</b></div></div>
      </a>
      <div class="env" title="环境检测：全部可用"><span><i></i>git</span><span><i></i>claude</span><span><i></i>codex</span><span><i></i>node</span></div>
    </div>
  </aside>
  <div class="main">
    <header class="top">
      ${crumb?`<span class="crumb">${crumb}</span>`:''}<h1>${title}</h1>
      <span class="spacer"></span>
      <button class="search" data-act="cmd">${I('search',14)}<span>搜索工单、项目，或输入命令</span><kbd>Ctrl K</kbd></button>
      <a class="btn primary" href="#/new">${I('plus',14)}新建工单</a>
      <a class="runchip" href="#/runs" title="打开运行面板">${c.running?'<span class="pulse"></span>':''}运行中 <b>${c.running}/${state.limit}</b></a>
      <button class="iconbtn" data-act="notes" title="通知">${I('bell')}${state.note?'<span class="pip"></span>':''}</button>
      <button class="iconbtn" data-act="theme" title="切换亮色 / 暗色">${I(state.theme==='dark'?'sun':'moon')}</button>
      <div class="winctl"><span>${I('min',14)}</span><span>${I('max',12)}</span><span>${I('x',14)}</span></div>
    </header>
    <main class="view" id="view"></main>
  </div></div>`;
}
