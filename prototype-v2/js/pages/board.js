// 看板：七个阶段是一条从左到右的水流；「人工审核」「验收中」两列是你的关口
function boardCard(t){
  const p=t.projects.map(id=>`<span class="tag line">${id}</span>`).join('');
  return `<a class="tcard p-${t.prio}" draggable="true" data-key="${t.key}" href="#/ticket/${t.key}">
    <div class="tcard-top">${stamp(t.type)}<span class="key">${t.key}</span>${t.running?`<span class="live">${ripple}</span>`:''}<span class="grow"></span>${t.due?`<span class="due">${I('clock',11)}${t.due}</span>`:''}</div>
    <div class="tcard-title">${esc(t.title)}</div>
    <div class="tcard-meta">${p}</div>
    ${t.sub||t.issues||t.status==='ai_review'?`<div class="tcard-foot">${subChips(t)}${t.issues?`<span class="subchip cin"><b>${t.issues}</b><span>个问题待处理</span></span>`:''}${t.status==='ai_review'?`<span class="subchip tide">${ripple}<span>预审中 · 00:38</span></span>`:''}</div>`:''}
  </a>`;
}
function pageBoard(){
  const f=state.boardFilter;
  const list=TICKETS.filter(t=>f==='all'||t.projects.includes(f)||(f==='bug'&&t.type==='bug'));
  const review=TICKETS.filter(t=>t.status==='human_review'),acc=TICKETS.filter(t=>t.status==='acceptance');
  const cols=STAGES.map(s=>{
    const items=list.filter(t=>t.status===s.id);
    return `<section class="col ${s.mine?'mine':''}" data-stage="${s.id}">
      <header class="col-h"><span class="col-num">${s.num}</span><b>${s.name}</b><span class="col-n">${items.length}</span>${s.mine&&items.length?'<span class="col-flag">等你</span>':''}</header>
      <div class="col-b">${items.map(boardCard).join('')||`<div class="col-empty">${s.hint}</div>`}</div>
    </section>`}).join('');
  $('#view').innerHTML=`<div class="page flush board-page">
    <div class="today">
      <div class="today-l"><span class="today-date">十一月廿一 · 周六</span>
        <p>有 <b>${review.length+acc.length}</b> 张工单在等你：<a href="#/ticket/TW-42/review">${review.length} 张待审核</a>，<a href="#/ticket/TW-37/accept">${acc.length} 张待验收</a>。另有 <a href="#/runs">3 个 Agent 正在运行</a>。</p></div>
      <div class="row">
        <div class="seg" id="bf">${[['all','全部'],['shop-api','shop-api'],['shop-web','shop-web'],['bug','只看 Bug']].map(([k,n])=>`<button class="${f===k?'on':''}" data-f="${k}">${n}</button>`).join('')}</div>
        <button class="btn ghost" data-toast="演示：筛选条件（优先级、标签、截止日期）">${I('filter',14)}筛选</button>
      </div>
    </div>
    <div class="board">${cols}</div>
    <div class="board-hint">${I('drag',12)}拖动卡片到另一列即尝试流转；不合法的流转会说明原因。Bug 可从「草稿」直接拖到「开发中」。</div>
  </div>`;
  $('#bf').onclick=e=>{const b=e.target.closest('button');if(b){state.boardFilter=b.dataset.f;pageBoard()}};
  let dragKey=null;
  $$('.tcard').forEach(c=>{
    c.addEventListener('dragstart',e=>{dragKey=c.dataset.key;c.classList.add('dragging');e.dataTransfer.effectAllowed='move';
      const from=T(dragKey).status;$$('.col').forEach(col=>{const to=col.dataset.stage;if(to===from)return;col.classList.add(canMove(T(dragKey),to).ok?'can':'cannot')})});
    c.addEventListener('dragend',()=>{c.classList.remove('dragging');$$('.col').forEach(col=>col.classList.remove('can','cannot','over'))});
  });
  $$('.col').forEach(col=>{
    col.addEventListener('dragover',e=>{e.preventDefault();col.classList.add('over')});
    col.addEventListener('dragleave',()=>col.classList.remove('over'));
    col.addEventListener('drop',e=>{e.preventDefault();if(dragKey)tryMove(dragKey,col.dataset.stage)});
  });
}
function canMove(t,to){
  const from=t.status;if(from===to)return{ok:false,silent:true};
  const label=(FLOW[from]||{})[to];
  if(!label)return{ok:false,why:`「${STAGE[from].name}」不能直接到「${STAGE[to].name}」。`+({draft:'草稿需要先提交预审。',ai_review:'请等待预审完成。',human_review:'需要先定稿。',finalized:'定稿后只能开始开发，或发起变更回到审核。',developing:'需要全部子任务完成后进入验收。',acceptance:'验收只能通过或打回。',done:'已完成的工单不能再流转。'}[from]||'')};
  if(from==='draft'&&to==='developing'&&t.type!=='bug')return{ok:false,why:'只有 Bug 可以走快速通道跳过 PRD 审核，需求工单请先提交预审。'};
  if(from==='human_review'&&to==='finalized'&&t.key==='TW-42'&&ISSUES42.some(i=>i.sev==='blocker'&&i.state==='open'))return{ok:false,why:'还有未处理的阻塞问题，处理完才能定稿。',go:'#/ticket/TW-42/review'};
  if(from==='developing'&&to==='acceptance'&&t.sub&&t.sub.some(([,s])=>['running','queued','blocked'].includes(s)))return{ok:false,why:'还有子任务没跑完，全部完成后会自动进入验收。'};
  if(from==='acceptance'&&to==='done'&&t.sub&&t.sub.some(([,s])=>s!=='accepted'))return{ok:false,why:'还有子任务未验收通过，不能合并。',go:`#/ticket/${t.key}/accept`};
  return{ok:true,label};
}
function tryMove(key,to){
  const t=T(key),r=canMove(t,to);
  if(r.silent)return;
  if(!r.ok){toast(`${key} 无法流转：${r.why}`);if(r.go)setTimeout(()=>location.hash=r.go,900);return}
  const apply=()=>{t.status=to;t.updated='刚刚';if(to==='developing'&&!t.sub)t.sub=[[t.type==='bug'?'BUG':'BE','queued']];if(to==='ai_review')t.running=true;toast(`${key} → ${STAGE[to].name}（${r.label}）`);route()};
  if(to==='finalized'||to==='done')confirmBox(to==='done'?'验收通过并合并':'定稿',to==='done'?`将 ${key} 的分支 squash 合并到 main，并清理 worktree。`:`定稿后 ${key} 的 PRD 将锁定，再修改需要「发起变更」。`,r.label,apply);
  else apply();
}
