// 工单详情：概览 / PRD / 审核 / 子任务 / 验收 / 运行记录
const SEV={blocker:['阻塞','cin'],major:['重要','amber'],minor:['次要','tide'],suggestion:['建议','']};
const ISTATE={open:'待处理',adopted:'已采纳',ignored:'已忽略',pending:'待确认',manual:'已手动修改'};
function rail(t){ // 七段流程轨
  const idx=STAGES.findIndex(s=>s.id===t.status);
  return `<div class="rail">${STAGES.map((s,i)=>`<div class="rail-s ${i<idx?'past':i===idx?'now':''}"><i>${i<idx?I('check',10):s.num}</i><span>${s.name}</span></div>`).join('')}</div>`;
}
function pageTicket(key,tab){
  const t=T(key);if(!t){location.hash='#/board';return}
  const rich=key==='TW-42'?'review':key==='TW-39'?'dev':null;
  const reviewing=rich==='review'&&t.status==='human_review';
  const openIssues=ISSUES42.filter(i=>i.state==='open').length;
  const toAccept=rich==='dev'?TASKS39.filter(k=>k.status==='review').length:0;
  tab=tab||(t.status==='human_review'?'review':t.status==='acceptance'?'accept':'overview');
  const tabs=[['overview','概览'],['prd','PRD',rich==='review'?'v'+VERSIONS42[0].v:''],['review','审核',reviewing&&openIssues?openIssues:'',1],['tasks','子任务',t.sub?t.sub.length:''],['accept','验收',toAccept||'',1],['runs','运行记录']];
  const next={draft:['提交预审','ai_review'],human_review:['定稿…','finalized'],finalized:['开始开发','developing'],acceptance:['验收通过并合并','done']}[t.status];
  $('#view').innerHTML=`<div class="page flush">
    <div class="thead">
      <div class="thead-1">${stamp(t.type,true)}<div class="grow"><div class="row"><span class="key">${t.key}</span><span class="tag ${STAGE[t.status].mine?'cin':'tide'}">${STAGE[t.status].name}</span>${prio(t.prio)}${t.due?`<span class="dim">截止 ${t.due}</span>`:''}</div>
        <h2>${esc(t.title)}</h2></div>
        <div class="row">${['finalized','developing'].includes(t.status)?`<button class="btn" data-toast="演示：填写变更原因后，基于定稿版本生成新草稿并回到人工审核">${I('undo',14)}发起变更</button>`:''}
        ${next?`<button class="btn primary" data-next="${next[1]}">${next[0]}</button>`:''}<button class="iconbtn" data-toast="演示：取消工单 / 复制链接 / 导出 Markdown">${I('more')}</button></div></div>
      ${rail(t)}
    </div>
    <nav class="tabs">${tabs.map(([id,n,b,hot])=>`<a href="#/ticket/${key}/${id}" class="${tab===id?'on':''}">${n}${b?`<em class="${hot?'hot':''}">${b}</em>`:''}</a>`).join('')}</nav>
    <div class="tbody" id="tbody"></div></div>`;
  const nb=$('[data-next]');if(nb)nb.onclick=()=>{if(nb.dataset.next==='finalized'&&key==='TW-42')openFinalize();else tryMove(key,nb.dataset.next)};
  ({overview:tabOverview,prd:tabPrd,review:tabReview,tasks:tabTasks,accept:tabAccept,runs:tabRuns}[tab]||tabOverview)(t,rich);
}
// 没有专门演示数据的工单（含定稿后的 TW-42）：按 t.sub 生成子任务
function genericTasks(t){
  return (t.sub||[]).map(([k,s])=>{const sp=t.key==='TW-42'?SPLIT42.find(x=>x.kind===k):null,proj=sp?sp.proj:t.projects[k==='FE'?t.projects.length-1:0],agent=sp?sp.agent:PROJ[proj].agent,provider=sp?sp.provider:PROJ[proj].provider;
    return{id:k,kind:k==='FE'?'前端':k==='BUG'?'Bug':'后端',cls:k==='FE'?'fe':k==='BUG'?'bug':'be',proj,title:sp?sp.title:t.title,status:s,statusName:SUB[s][0],agent,provider,model:PROV[provider].model,perm:'workspace_write',branch:`tw/${t.key}-${k.toLowerCase()}`,wt:`D:\\tidewatch\\worktrees\\${proj}\\${t.key}-${k.toLowerCase()}`,runs:0,cost:'—',dur:'—',files:0,add:0,del:0,dep:sp&&sp.dep?sp.dep:(k==='FE'&&t.sub.length>1?'依赖 BE 的 API 文档':'')}});
}
const empty=(icon,title,body,btn='')=>`<div class="empty">${I(icon,28)}<b>${title}</b><p>${body}</p>${btn}</div>`;

function tabOverview(t,rich){
  const tl=rich==='review'?TIMELINE42:rich!=='dev'?[[t.updated,'你',`工单进入「${STAGE[t.status].name}」`,'arrow']]:[['刚刚','Codex','开始运行 R-121（前端子任务）','runs'],['今天 13:12','系统','检测到 docs/api/TW-39.md，已解锁前端子任务','link'],['今天 13:10','Claude Code','后端子任务运行完成，测试 42 通过，等待验收','check'],['今天 12:58','你','确认拆分为 BE + FE 两个子任务并定稿（v4）','lock'],['昨天 17:40','AI 预审','完成，发现 4 个问题','spark']];
  const tasks=rich==='dev'?TASKS39:t.sub?genericTasks(t):null;
  const reviewing=rich==='review'&&t.status==='human_review',bl=blockersLeft(),handled=ISSUES42.filter(i=>i.state!=='open'&&i.state!=='pending').length;
  const wait=rich==='dev'?TASKS39.filter(k=>k.status==='review'):[];
  if(rich==='review'&&!reviewing&&tl[0][2].indexOf('定稿')<0)tl.unshift(['刚刚','你',`定稿 PRD v${VERSIONS42[0].v}，拆分为 BE + FE 两个子任务`,'lock']);
  $('#tbody').innerHTML=`<div class="page ov">
    <div class="ov-main">
      ${reviewing?`<div class="callout cin">${I('alert',15)}<div><b>${bl?`还差 ${bl} 个阻塞问题没处理，处理完才能定稿。`:'阻塞问题已处理完，可以定稿了。'}</b> AI 预审共发现 ${ISSUES42.length} 个问题，已处理 ${handled} 个。<a class="lnk" href="#/ticket/TW-42/review">去审核 →</a></div></div>`:''}
      ${wait.length?`<div class="callout cin">${I('check',15)}<div><b>${wait.map(k=>k.kind).join('、')}子任务已跑完，等你验收。</b> ${wait.map(k=>`${k.kind} ${k.files} 个文件 +${k.add} −${k.del}`).join('；')}。<a class="lnk" href="#/ticket/TW-39/accept">去验收 →</a></div></div>`:''}
      <div class="sec-title" style="margin-top:18px">子任务${tasks?`<small>后端先出 API 文档，前端依赖其完成</small>`:''}</div>
      ${tasks&&tasks.length?`<div class="tgrid">${tasks.map(k=>`<a class="tk" href="#/ticket/${t.key}/tasks"><div class="row"><span class="stamp ${k.cls}">${k.id==='BE'?'后':k.id==='FE'?'前':'虫'}</span><b class="grow ell">${esc(k.title)}</b><span class="tag ${k.status==='running'?'tide':k.status==='review'?'cin':''}">${k.status==='running'?ripple:''}${k.statusName}</span></div>
        <div class="row dim" style="margin-top:8px">${agentTag(k.agent)}<span class="mono">${I('branch',12)}</span><span class="mono ell">${k.branch}</span></div>${k.dep?`<div class="dep">${I('link',12)}${k.dep}</div>`:''}</a>`).join('')}</div>`
       :`<div class="card">${empty('branch','定稿后才会拆分子任务','定稿时 AI 会建议拆成后端 / 前端子任务，由你确认后创建分支与 worktree。')}</div>`}
      <div class="sec-title" style="margin-top:22px">时间线</div>
      <ol class="tl">${tl.map(([time,who,what,ic])=>`<li><i>${I(ic,12)}</i><div><b>${who}</b> ${what}</div><time>${time}</time></li>`).join('')}</ol>
    </div>
    <aside class="ov-side">
      <dl class="props">
        <dt>类型</dt><dd>${stamp(t.type)} ${TYPES[t.type].name}</dd>
        <dt>绑定项目</dt><dd>${t.projects.map(p=>`<a class="tag line" href="#/projects/${p}">${I('folder',11)}${p}<span class="dim">${PROJ[p].kindName}</span></a>`).join(' ')}</dd>
        <dt>来源</dt><dd>${esc(t.source)}</dd>
        <dt>当前 PRD</dt><dd>${rich==='review'?`v${VERSIONS42[0].v} · ${reviewing?'未定稿':'已定稿 '+I('lock',11)}`:rich==='dev'?'v4 · 已定稿 '+I('lock',11):'v1'}</dd>
        <dt>默认 Agent</dt><dd>${agentTag(PROJ[t.projects[0]].agent)}</dd>
        <dt>Provider</dt><dd>${PROV[PROJ[t.projects[0]].provider].name}</dd>
        <dt>标签</dt><dd>${(t.labels||[]).map(l=>`<span class="tag">${l}</span>`).join(' ')||'<span class="dim">无</span>'}</dd>
        <dt>累计费用</dt><dd class="mono">${rich==='dev'?'$ 2.21 · 15 分 32 秒':rich==='review'?'$ 0.06 · 预审 1 次':'—'}</dd>
      </dl>
    </aside></div>`;
}

function tabPrd(t,rich,show){
  if(rich!=='review'&&t.body){$('#tbody').innerHTML=`<div class="page prd2"><article class="doc card"><div class="doc-bar"><span class="tag tide">v1 · 当前</span><span class="dim">${t.updated} · 你 · ${esc(t.source)}</span></div><div class="prose">${md(t.body)}</div></article></div>`;return}
  if(rich!=='review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('file','演示数据未包含这张工单的 PRD','请打开 TW-42 查看 PRD 版本与对比。',`<a class="btn" href="#/ticket/TW-42/prd">打开 TW-42</a>`)}</div></div>`;return}
  const idx=Math.max(0,VERSIONS42.findIndex(v=>v.v===show)),cur=VERSIONS42[idx],prev=VERSIONS42[idx+1],locked=t.status!=='human_review';
  $('#tbody').innerHTML=`<div class="page prd2">
    <article class="doc card"><div class="doc-bar"><span class="tag tide">v${cur.v}${idx?'':' · 当前'}</span><span class="dim">${cur.time} · ${cur.who} · ${cur.src}</span>${locked&&!idx?`<span class="tag">${I('lock',11)}已定稿，只读</span>`:''}<span class="grow"></span>
      ${idx||locked?'':`<button class="btn sm" data-toast="演示：进入编辑，保存后生成 v${cur.v+1}">${I('edit',13)}编辑</button>`}${prev?`<button class="btn sm" id="cmp">${I('code',13)}与 v${prev.v} 对比</button>`:''}</div>
      <div class="prose" id="prose">${md(cur.text,[],prev?prev.text:null)}</div></article>
    <aside><div class="sec-title">版本<small>点击查看</small></div><ol class="vers">${VERSIONS42.map(v=>`<li class="${v.v===cur.v?'on':''}" data-v="${v.v}"><b>v${v.v}</b><div><span>${v.src}</span><small>${v.who} · ${v.time}</small><small>${v.note}</small></div></li>`).join('')}</ol>
      <div class="callout tide" style="margin-top:14px">${I('info',15)}<div>每次保存都会生成一个不可变的新版本。定稿后 PRD 只读，改动需要「发起变更」。</div></div></aside></div>`;
  const c=$('#cmp');if(c)c.onclick=()=>{const on=$('#prose').classList.toggle('diffing');toast(on?`高亮显示 v${cur.v} 相对 v${prev.v} 新增或改写的内容`:'已关闭对比')};
  $$('[data-v]').forEach(li=>li.onclick=()=>tabPrd(t,rich,+li.dataset.v));
}

function tabReview(t,rich){
  if(rich==='review'&&t.status!=='human_review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('lock',`PRD v${VERSIONS42[0].v} 已定稿`,`预审发现的 ${ISSUES42.length} 个问题已处理完。需要再改需求时，请「发起变更」，工单会回到人工审核。`,`<a class="btn" href="#/ticket/TW-42/prd">查看定稿版本</a>`)}</div></div>`;return}
  if(rich!=='review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${t.type==='bug'?empty('zap','Bug 走快速通道，跳过了审核','描述太模糊时，也可以手动把它送去 AI 预审。',`<button class="btn" data-toast="演示：已提交预审">送去 AI 预审</button>`):empty('check','这张工单的问题已全部处理','PRD 已定稿。请打开 TW-42 查看进行中的审核。',`<a class="btn" href="#/ticket/TW-42/review">打开 TW-42</a>`)}</div></div>`;return}
  const blockers=blockersLeft(),pend=ISSUES42.filter(i=>i.state==='pending').length,ver=VERSIONS42[0].v;
  const left=i=>i.state==='open'||(i.state==='pending'&&i.sev==='blocker');
  const issue=i=>{const[sn,sc]=SEV[i.sev];const closed=i.state!=='open'&&i.state!=='pending',pending=i.state==='pending';
    return `<article class="iss ${closed?'closed':''} s-${i.sev}" data-id="${i.id}">
      <header><span class="tag ${sc}">${sn}</span><span class="key">${i.id}</span><span class="dim">${i.cat}</span><span class="grow"></span>${closed?`<span class="tag ${i.state==='adopted'||i.state==='manual'?'moss':''}">${ISTATE[i.state]}</span><button class="btn ghost sm" data-ia="open">撤销</button>`:pending?`<span class="tag amber">待确认${i.sev==='blocker'?' · 仍阻塞定稿':''}</span>`:''}</header>
      <blockquote>${esc(i.loc)}</blockquote>
      <p>${inline(i.problem)}</p>
      ${!closed?`${i.q?`<p class="q">${I('msg',13)}<span>需确认：${esc(i.q)}</span></p>`:''}
      <div class="fix"><span>建议改为</span><div>${inline(i.fix)}</div></div>
      <footer><button class="btn primary sm" data-ia="adopted">${I('check',13)}采纳建议</button><button class="btn sm" data-ia="manual">手动修改</button>${pending?`<button class="btn sm" data-ia="open">取消待确认</button>`:`<button class="btn sm" data-ia="pending">待确认</button>`}<button class="btn ghost sm" data-ia="ignored">忽略…</button></footer>`
      :i.reason?`<p class="dim">忽略理由：${esc(i.reason)}</p>`:''}
    </article>`};
  const groups=['blocker','major','minor','suggestion'].map(s=>{const l=ISSUES42.filter(i=>i.sev===s);return l.length?`<div class="iss-g"><span>${SEV[s][0]}</span><i>${l.filter(left).length} / ${l.length} 待处理</i></div>${l.map(issue).join('')}`:''}).join('');
  $('#tbody').innerHTML=`<div class="rv">
    <div class="rv-bar">
      <div class="rv-sum"><b>AI 预审</b><span class="dim">DeepSeek · 用时 52 秒 · $ 0.06</span><span class="q1">「需求方向清楚，但计算顺序和数据结构没写明白，直接开发大概率返工。」</span></div>
      <span class="grow"></span>
      <button class="btn sm" data-toast="演示：只审查改动部分和未关闭的问题">${I('resume',13)}重新预审</button>
      <button class="btn sm" id="exportq">${I('msg',13)}导出待确认问题${pend?` (${pend})`:''}</button>
      <button class="btn primary sm" id="fin" ${blockers?'disabled':''} title="${blockers?`还有 ${blockers} 个阻塞问题未处理`:'锁定 PRD 并拆分子任务'}">${I('lock',13)}定稿${blockers?` · 还差 ${blockers} 个阻塞`:''}</button>
    </div>
    <div class="rv-cols">
      <section class="rv-doc"><div class="doc-bar"><span class="tag tide">PRD v${ver}</span><span class="dim">点击高亮处可定位到对应问题</span><span class="grow"></span><button class="btn ghost sm" data-toast="演示：进入编辑，保存后生成新版本">${I('edit',13)}编辑</button></div>
        <div class="prose">${md(prd42(),ISSUES42)}
          <h3>验收标准 <span class="tag tide" style="vertical-align:2px">AI 起草 · 勾选后纳入定稿</span></h3>
          <ul class="ac">${AC42.map(([s,on],i)=>`<li data-ac="${i}"><span class="check ${on?'on':''}">${I('check',11)}</span>${esc(s)}</li>`).join('')}</ul></div></section>
      <section class="rv-iss" id="isslist">${groups}</section>
    </div></div>`;
  $$('[data-ia]').forEach(b=>b.onclick=e=>{e.stopPropagation();const id=b.closest('.iss').dataset.id,i=ISSUES42.find(x=>x.id===id),to=b.dataset.ia;
    const done=()=>{const from=i.state;let text=prd42(),msg;
      // 先撤掉旧状态对正文的影响，再应用新状态；正文有变化就存成新版本
      if(from==='adopted')text=text.replace(i.to,i.find);
      if(from==='ignored')text=text.split('\n').filter(l=>!l.startsWith(`- 不处理 ${id}（`)).join('\n');
      if(to==='adopted'){text=text.replace(i.find,i.to);if(id==='I-5')AC42.forEach(a=>a[1]=true)}
      if(to==='ignored'){if(!text.includes('## 不做范围'))text+='\n\n## 不做范围';text+=`\n- 不处理 ${id}（${i.cat}：${i.loc}）：${i.reason}`}
      i.state=to;
      if(text!==prd42()){prd42Save(text,{adopted:'采纳 AI 建议',ignored:'忽略并记录理由'}[to]||'撤销处理',`${{adopted:'采纳',ignored:'忽略',open:'撤销',pending:'改为待确认',manual:'改为手动修改'}[to]} ${id}`);msg=`，PRD 已更新为 v${VERSIONS42[0].v}`}
      T('TW-42').issues=ISSUES42.filter(left).length;pageTicket('TW-42','review');
      toast({adopted:`已采纳 ${id}`,manual:`${id} 标记为已手动修改`,pending:`${id} 标记为待确认，可一并导出给需求方`+(i.sev==='blocker'?'；阻塞问题确认前不能定稿':''),ignored:`已忽略 ${id}，理由写入「不做范围」`,open:`已撤销 ${id}`}[to]+(msg||''))};
    if(to==='ignored')promptBox('忽略这个问题','理由会写入 PRD 的「不做范围」。'+(i.sev==='blocker'?'阻塞问题必须写明理由。':''),'例如：本期不做',v=>{if(!v&&i.sev==='blocker'){toast('忽略阻塞问题需要填写理由');return}i.reason=v||'本期不做';done()});else done()});
  $$('mark[data-issue]').forEach(m=>m.onclick=()=>{const el=$(`.iss[data-id="${m.dataset.issue}"]`);if(el){el.scrollIntoView({block:'center',behavior:'smooth'});el.classList.add('flash');setTimeout(()=>el.classList.remove('flash'),1200)}});
  $$('.iss').forEach(el=>el.onmouseenter=()=>{$$('mark.hot').forEach(m=>m.classList.remove('hot'));const m=$(`mark[data-issue="${el.dataset.id}"]`);if(m)m.classList.add('hot')});
  $$('[data-ac]').forEach(li=>li.onclick=()=>{const a=AC42[li.dataset.ac];a[1]=!a[1];$('.check',li).classList.toggle('on',a[1])});
  $('#fin').onclick=openFinalize;
  $('#exportq').onclick=()=>{const qs=ISSUES42.filter(i=>i.state==='pending'&&i.q);infoBox('发给需求方的待确认问题','已整理成一段可以直接发出去的文字。',`关于「优惠券叠加规则与结算页展示」，有 ${qs.length} 点需要确认：\n\n`+qs.map((i,n)=>`${n+1}. ${i.q}`).join('\n'),'复制')};
}

function tabTasks(t,rich){
  if(rich==='review'&&!t.sub){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('branch','定稿后才会拆分子任务','定稿时 AI 会建议拆成后端 / 前端子任务，由你确认后创建分支与 worktree。',`<button class="btn" id="pv">预览拆分建议</button>`)}</div></div>`;$('#pv').onclick=openFinalize;return}
  const list=rich==='dev'?TASKS39:genericTasks(t);
  if(!list.length){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('branch','还没有子任务','定稿后会按前端 / 后端拆分子任务。')}</div></div>`;return}
  $('#tbody').innerHTML=`<div class="page"><div class="tasks">${list.map((k,n)=>`
    ${n>0&&k.dep?`<div class="dep-line"><span>${I('link',12)}${k.dep}</span></div>`:''}
    <section class="task card">
      <header><span class="stamp ${k.cls} lg">${k.kind[0]==='B'?'虫':k.kind[0]}</span><div class="grow"><b>${esc(k.title)}</b><div class="dim">${k.kind}子任务 · ${k.proj}</div></div>
        <span class="tag ${k.status==='running'?'tide':k.status==='review'?'cin':k.status==='accepted'||k.status==='merged'?'moss':''}">${k.status==='running'?ripple:''}${k.statusName}</span></header>
      <div class="task-grid">
        <div><label>Agent</label>${agentTag(k.agent)}</div><div><label>Provider · 模型</label>${PROV[k.provider].name} · <span class="mono">${k.model}</span></div>
        <div><label>权限档位</label><span class="tag line">${I('shield',11)}${k.perm}</span></div><div><label>分支</label><span class="mono">${k.branch}</span></div>
        <div style="grid-column:span 2"><label>worktree</label><span class="mono">${k.wt}</span></div>
        <div><label>最近运行</label>${k.lastRun?`<a class="lnk mono" href="#/runs/${k.lastRun}">${k.lastRun}</a> · 共 ${k.runs} 次`:k.runs?`共 ${k.runs} 次`:'<span class="dim">尚未运行</span>'}</div><div><label>耗时 · 费用</label><span class="mono">${k.dur} · ${k.cost}</span></div>
        ${k.artifact?`<div style="grid-column:span 2"><label>产物</label><span class="tag moss">${I('file',11)}${k.artifact}</span> <span class="tag moss">${I('check',11)}测试 ${k.tests}</span> <span class="mono dim">${k.files} 个文件 +${k.add} −${k.del}</span></div>`:''}
      </div>
      <footer>${k.status==='running'?`<a class="btn sm" href="#/runs/${k.lastRun||'R-121'}">${I('term',13)}看实时日志</a><button class="btn danger sm" data-toast="演示：结束进程树，可稍后恢复原会话">${I('stop',13)}停止</button>`
        :k.status==='review'?`<a class="btn primary sm" href="#/ticket/${t.key}/accept">去验收</a>`
        :k.status==='blocked'?`<button class="btn sm" disabled title="${k.dep||'等待依赖'}">${I('play',13)}等待依赖</button>`:k.status==='queued'?`<button class="btn sm" data-toast="已在运行队列中，有空闲并发时自动开始">${I('clock',13)}排队中</button>`:k.status==='accepted'||k.status==='merged'?''
        :`<button class="btn sm" data-toast="演示：基于 session id 恢复原会话">${I('resume',13)}恢复</button>`}
        <button class="btn ghost sm" data-toast="演示：用 VS Code 打开该 worktree">${I('ext',13)}在 IDE 打开</button>
        <button class="btn ghost sm" data-toast="演示：更换 Agent / Provider / 权限档位">${I('gear',13)}执行配置</button></footer>
    </section>`).join('')}</div></div>`;
}

let acceptSel=null; // 当前在验收页选中的子任务
function tabAccept(t,rich){
  if(rich!=='dev'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('check','还没有可验收的改动','子任务跑完后，这里会出现 Diff、测试报告和 Agent 总结。请打开 TW-39 查看验收界面。',`<a class="btn" href="#/ticket/TW-39/accept">打开 TW-39</a>`)}</div></div>`;return}
  const viewable=k=>['review','accepted'].includes(k.status);
  if(!acceptSel||!viewable(TASKS39.find(k=>k.id===acceptSel)))acceptSel=(TASKS39.find(k=>k.status==='review')||TASKS39.find(viewable)||{}).id||null;
  if(!acceptSel){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('runs','子任务还在返工或运行中','跑完后会回到这里等你验收。',`<a class="btn" href="#/ticket/TW-39/tasks">查看子任务</a>`)}</div></div>`;return}
  const k=TASKS39.find(x=>x.id===acceptSel),d=ACCEPT39[k.id],comments=d.comments,sel=d.files.find(f=>f.sel),done=k.status==='accepted';
  const add=d.files.reduce((n,f)=>n+f.add,0),del=d.files.reduce((n,f)=>n+f.del,0);
  const rows=d.rows.map(([ol,ot,nl,nt,kind])=>`<tr class="${kind}"><td class="ln">${ol??''}</td><td class="code l">${kind==='add'?'':esc(ot)}</td><td class="ln">${nl??''}</td><td class="code r">${kind==='del'?'':esc(nt)}${nl&&!done?`<button class="addc" data-line="${nl}" title="添加行内评论">${I('plus',11)}</button>`:''}</td></tr>${comments.filter(x=>x.line===nl&&nl).map(x=>`<tr class="cmt"><td colspan="2"></td><td colspan="2"><div class="cbox">${I('msg',13)}<div><b>你</b> · 第 ${x.line} 行<p>${esc(x.text)}</p></div></div></td></tr>`).join('')}`).join('');
  $('#tbody').innerHTML=`<div class="acc">
    <div class="acc-bar"><div class="seg" id="tsel">${TASKS39.map(x=>`<button class="${x.id===k.id?'on':''}" data-t="${x.id}"><span class="stamp ${x.cls}" style="margin-right:6px;display:inline-grid">${x.kind[0]}</span>${x.kind} · ${x.statusName}</button>`).join('')}</div>
      <div class="seg"><button class="on">全部改动</button>${d.runs.map(r=>`<button data-toast="演示：只看 ${r} 这一轮的增量">${r}</button>`).join('')}</div>
      <span class="mono dim">${d.files.length} 个文件 <span style="color:var(--moss)">+${add}</span> <span style="color:var(--cinnabar)">−${del}</span></span><span class="grow"></span>
      <button class="btn sm" data-toast="演示：用 VS Code 打开 worktree，新提交会自动纳入 Diff">${I('ext',13)}在 IDE 打开</button>
      ${done?`<span class="tag moss">${I('check',11)}已通过</span>`:`<button class="btn danger sm" id="rej">${I('undo',13)}打回${comments.length?` · ${comments.length} 条意见`:''}</button><button class="btn primary sm" id="pass">${I('check',13)}通过</button>`}</div>
    <div class="acc-cols">
      <nav class="ftree"><div class="ftree-h">改动文件</div>${d.files.map(f=>{const parts=f.p.split('/');return `<a class="${f.sel?'on':''}" title="${f.p}" ${f.sel?'':'data-toast="演示数据只准备了选中文件的 Diff"'}><span class="fs ${f.s}">${f.s}</span><span class="grow ell"><b>${parts.pop()}</b><small>${parts.slice(-2).join('/')}</small></span>${f.sel&&comments.length?`<span class="cc">${I('msg',11)}${comments.length}</span>`:''}<span class="mono dim">${f.add?'+'+f.add:''}${f.del?' −'+f.del:''}</span></a>`}).join('')}</nav>
      <section class="diff"><div class="diff-h"><span class="mono">${d.dir}<b>${d.name}</b></span><span class="grow"></span><span class="dim">并排对比${done?'':' · 鼠标移到右侧行尾可添加行内评论'}</span></div>
        <div class="diff-b"><table>${rows}</table></div></section>
      <aside class="acc-side">
        <div class="sec-title">验收标准<small>${d.ac.filter(a=>a[1]).length} / ${d.ac.length} 已确认</small></div>
        <ul class="ac">${d.ac.map(([txt,on],i)=>`<li data-ac="${i}"><span class="check ${on?'on':''}">${I('check',11)}</span>${esc(txt)}</li>`).join('')}</ul>
        <div class="sec-title" style="margin-top:18px">自动测试</div>
        <div class="tests">${d.tests.map(([c,n])=>`<div><span class="tag moss">${I('check',11)}通过</span><span class="mono">${c}</span><span class="dim">${n}</span></div>`).join('')}</div>
        <div class="sec-title" style="margin-top:18px">AI 交叉审查<small>${d.xr.by} · 只读</small></div>
        <div class="xr">${d.xr.items.map(([c,n,txt])=>`<p><span class="tag ${c}">${n}</span> ${inline(txt)}</p>`).join('')}</div>
        <div class="sec-title" style="margin-top:18px">Agent 总结</div>
        <p class="sum">${inline(d.sum)}</p>
        <div class="cost mono">${I('clock',12)}${d.cost}</div>
      </aside></div></div>`;
  const redraw=()=>tabAccept(t,rich);
  const allAccepted=()=>TASKS39.every(x=>x.status==='accepted');
  $$('#tsel [data-t]').forEach(b=>b.onclick=()=>{const x=TASKS39.find(y=>y.id===b.dataset.t);if(!viewable(x)){toast(`${x.kind}子任务${x.statusName}，跑完后可验收`);return}acceptSel=x.id;redraw()});
  if(!done)$$('[data-ac]').forEach(li=>li.onclick=()=>{const a=d.ac[li.dataset.ac];a[1]=!a[1];redraw()});
  $$('.addc').forEach(b=>b.onclick=()=>promptBox(`第 ${b.dataset.line} 行 · 行内评论`,'行内评论会汇总进打回意见，作为下一轮运行的输入。','写下希望 Agent 修改的地方',v=>{if(v){comments.push({line:+b.dataset.line,text:v});redraw();toast('已添加行内评论')}}));
  const rej=$('#rej');if(rej)rej.onclick=()=>promptBox(`打回${k.kind}子任务`,`将自动创建新的运行：原任务 + 打回意见 + ${comments.length} 条行内评论，优先恢复原 Agent 会话。`,'补充总体意见（可选）',()=>{
    const prev=RUN(k.lastRun),id='R-'+(Math.max(...RUNS.map(r=>+r.id.slice(2)))+1),free=counts().running<state.limit;
    RUNS.unshift({id,ticket:'TW-39',task:k.id,title:`${k.title}（返工）`,agent:k.agent,provider:k.provider,model:k.model,status:free?'running':'queued',el:0,tin:0,tout:0,cost:0,now:free?`恢复会话，阅读打回意见与 ${comments.length} 条行内评论`:`排队中 · 并发已满 ${state.limit}/${state.limit}`,session:prev?prev.session:'—',perm:k.perm,wt:k.wt});
    k.lastRun=id;k.runs++;T('TW-39').status='developing';syncRun(RUNS[0]);
    toast(`已打回，返工运行 ${id} ${free?'已开始':'已入队'}${prev?`（恢复会话 ${prev.session.slice(0,8)}）`:''}`);location.hash='#/ticket/TW-39/tasks'},'打回并开始返工');
  const pass=$('#pass');if(pass)pass.onclick=()=>{const left=d.ac.filter(a=>!a[1]).length;if(left){toast(`还有 ${left} 条验收标准未确认`);return}
    confirmBox(`通过${k.kind}子任务`,'子任务标记为已通过。工单的全部子任务通过后，才会合并到 main。','通过',()=>{k.status='accepted';k.statusName='已通过';T('TW-39').sub.find(x=>x[0]===k.id)[1]='accepted';syncTW39();
      const next=TASKS39.find(x=>x.status==='review');
      toast(allAccepted()?'全部子任务已通过，可以「验收通过并合并」':`${k.kind}子任务已通过`+(next?`，还有${next.kind}子任务待验收`:'，其余子任务还没跑完'));
      if(next){acceptSel=next.id;pageTicket('TW-39','accept')}else location.hash='#/ticket/TW-39/tasks'})};
}

function tabRuns(t){
  const rs=RUNS.filter(r=>r.ticket===t.key);
  if(!rs.length){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('runs','还没有运行记录','开始开发或提交预审后，每次 Agent 运行都会记录在这里。')}</div></div>`;return}
  const tot=rs.reduce((a,r)=>({c:a.c+r.cost,t:a.t+r.tin+r.tout,s:a.s+r.el}),{c:0,t:0,s:0});
  $('#tbody').innerHTML=`<div class="page"><div class="kpis"><div><span>运行次数</span><b>${rs.length}</b></div><div><span>总耗时</span><b>${fmtDur(tot.s)}</b></div><div><span>Token</span><b>${fmtTok(tot.t)}</b></div><div><span>费用</span><b>$ ${tot.c.toFixed(2)}</b></div></div>
    <div class="card" style="overflow:hidden;margin-top:14px"><table class="tbl"><thead><tr><th>运行</th><th>子任务</th><th>Agent</th><th>Provider</th><th>状态</th><th class="num">耗时</th><th class="num">Token</th><th class="num">费用</th></tr></thead>
    <tbody>${rs.map(r=>`<tr data-go="#/runs/${r.id}"><td class="mono">${r.id}</td><td>${r.task} · ${esc(r.title)}</td><td>${agentTag(r.agent)}</td><td>${PROV[r.provider].name}</td><td>${runState(r)}</td><td class="num">${fmtDur(r.el)}</td><td class="num">${fmtTok(r.tin+r.tout)}</td><td class="num">$ ${r.cost.toFixed(2)}</td></tr>`).join('')}</tbody></table></div></div>`;
}
