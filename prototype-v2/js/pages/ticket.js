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
  const openIssues=ISSUES42.filter(i=>i.state==='open').length;
  tab=tab||(t.status==='human_review'?'review':t.status==='acceptance'?'accept':'overview');
  const tabs=[['overview','概览'],['prd','PRD',rich==='review'?'v3':''],['review','审核',rich==='review'&&openIssues?openIssues:'',1],['tasks','子任务',t.sub?t.sub.length:''],['accept','验收',rich==='dev'?1:'',1],['runs','运行记录']];
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
const empty=(icon,title,body,btn='')=>`<div class="empty">${I(icon,28)}<b>${title}</b><p>${body}</p>${btn}</div>`;

function tabOverview(t,rich){
  const tl=rich==='review'?TIMELINE42:[['刚刚','Codex','开始运行 R-121（前端子任务）','runs'],['今天 13:12','系统','检测到 docs/api/TW-39.md，已解锁前端子任务','link'],['今天 13:10','Claude Code','后端子任务运行完成，测试 42 通过，等待验收','check'],['今天 12:58','你','确认拆分为 BE + FE 两个子任务并定稿（v4）','lock'],['昨天 17:40','AI 预审','完成，发现 4 个问题','spark']];
  const tasks=rich==='dev'?TASKS39:rich==='review'?null:(t.sub||[]).map(([k,s])=>({id:k,cls:k==='FE'?'fe':k==='BUG'?'bug':'be',title:t.title,statusName:SUB[s][0],status:s,agent:PROJ[t.projects[0]].agent,branch:`tw/${t.key}-${k.toLowerCase()}`}));
  $('#tbody').innerHTML=`<div class="page ov">
    <div class="ov-main">
      ${rich==='review'?`<div class="callout cin">${I('alert',15)}<div><b>还差 2 个阻塞问题没处理，处理完才能定稿。</b> AI 预审共发现 7 个问题，已处理 3 个。<a class="lnk" href="#/ticket/TW-42/review">去审核 →</a></div></div>`:''}
      ${rich==='dev'?`<div class="callout cin">${I('check',15)}<div><b>后端子任务已跑完，等你验收。</b> 7 个文件，+312 −18，测试 42 通过；前端子任务正在运行。<a class="lnk" href="#/ticket/TW-39/accept">去验收 →</a></div></div>`:''}
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
        <dt>当前 PRD</dt><dd>${rich==='review'?'v3 · 未定稿':rich==='dev'?'v4 · 已定稿 '+I('lock',11):'v1'}</dd>
        <dt>默认 Agent</dt><dd>${agentTag(PROJ[t.projects[0]].agent)}</dd>
        <dt>Provider</dt><dd>${PROV[PROJ[t.projects[0]].provider].name}</dd>
        <dt>标签</dt><dd>${(t.labels||[]).map(l=>`<span class="tag">${l}</span>`).join(' ')||'<span class="dim">无</span>'}</dd>
        <dt>累计费用</dt><dd class="mono">${rich==='dev'?'$ 2.21 · 15 分 32 秒':rich==='review'?'$ 0.06 · 预审 1 次':'—'}</dd>
      </dl>
    </aside></div>`;
}

function tabPrd(t,rich){
  if(rich!=='review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('file','演示数据未包含这张工单的 PRD','请打开 TW-42 查看 PRD 版本与对比。',`<a class="btn" href="#/ticket/TW-42/prd">打开 TW-42</a>`)}</div></div>`;return}
  $('#tbody').innerHTML=`<div class="page prd2">
    <article class="doc card"><div class="doc-bar"><span class="tag tide">v3 · 当前</span><span class="dim">今天 14:02 · 你 · 采纳 AI 建议</span><span class="grow"></span><button class="btn sm" data-toast="演示：进入编辑，保存后生成 v4">${I('edit',13)}编辑</button><button class="btn sm" id="cmp">${I('code',13)}与 v2 对比</button></div>
      <div class="prose" id="prose">${md(PRD42)}<h4>F2 补充（v3 新增）</h4><p class="ins">新增 <code>POST /api/checkout/price-preview</code>，入参为商品行与券 ID 列表，返回逐券抵扣明细与实付金额；前端勾选变化时防抖 300ms 调用。</p></div></article>
    <aside><div class="sec-title">版本</div><ol class="vers">${VERSIONS42.map(v=>`<li class="${v.cur?'on':''}"><b>v${v.v}</b><div><span>${v.src}</span><small>${v.who} · ${v.time}</small><small>${v.note}</small></div></li>`).join('')}</ol>
      <div class="callout tide" style="margin-top:14px">${I('info',15)}<div>每次保存都会生成一个不可变的新版本。定稿后 PRD 只读，改动需要「发起变更」。</div></div></aside></div>`;
  $('#cmp').onclick=()=>{$('#prose').classList.toggle('diffing');toast('高亮显示 v3 相对 v2 新增的内容')};
}

function tabReview(t,rich){
  if(rich!=='review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${t.type==='bug'?empty('zap','Bug 走快速通道，跳过了审核','描述太模糊时，也可以手动把它送去 AI 预审。',`<button class="btn" data-toast="演示：已提交预审">送去 AI 预审</button>`):empty('check','这张工单的问题已全部处理','PRD 已定稿。请打开 TW-42 查看进行中的审核。',`<a class="btn" href="#/ticket/TW-42/review">打开 TW-42</a>`)}</div></div>`;return}
  const open=ISSUES42.filter(i=>i.state==='open'),blockers=open.filter(i=>i.sev==='blocker').length,pend=ISSUES42.filter(i=>i.state==='pending').length;
  const issue=i=>{const[sn,sc]=SEV[i.sev];const closed=i.state!=='open';
    return `<article class="iss ${closed?'closed':''} s-${i.sev}" data-id="${i.id}">
      <header><span class="tag ${sc}">${sn}</span><span class="key">${i.id}</span><span class="dim">${i.cat}</span><span class="grow"></span>${closed?`<span class="tag ${i.state==='adopted'||i.state==='manual'?'moss':i.state==='pending'?'amber':''}">${ISTATE[i.state]}</span><button class="btn ghost sm" data-ia="open">撤销</button>`:''}</header>
      <blockquote>${esc(i.loc)}</blockquote>
      <p>${inline(i.problem)}</p>
      ${!closed?`${i.q?`<p class="q">${I('msg',13)}<span>需确认：${esc(i.q)}</span></p>`:''}
      <div class="fix"><span>建议改为</span><div>${inline(i.fix)}</div></div>
      <footer><button class="btn primary sm" data-ia="adopted">${I('check',13)}采纳建议</button><button class="btn sm" data-ia="manual">手动修改</button><button class="btn sm" data-ia="pending">待确认</button><button class="btn ghost sm" data-ia="ignored">忽略…</button></footer>`
      :i.reason?`<p class="dim">忽略理由：${esc(i.reason)}</p>`:''}
    </article>`};
  const groups=['blocker','major','minor','suggestion'].map(s=>{const l=ISSUES42.filter(i=>i.sev===s);return l.length?`<div class="iss-g"><span>${SEV[s][0]}</span><i>${l.filter(i=>i.state==='open').length} / ${l.length} 待处理</i></div>${l.map(issue).join('')}`:''}).join('');
  $('#tbody').innerHTML=`<div class="rv">
    <div class="rv-bar">
      <div class="rv-sum"><b>AI 预审</b><span class="dim">DeepSeek · 用时 52 秒 · $ 0.06</span><span class="q1">「需求方向清楚，但计算顺序和数据结构没写明白，直接开发大概率返工。」</span></div>
      <span class="grow"></span>
      <button class="btn sm" data-toast="演示：只审查改动部分和未关闭的问题">${I('resume',13)}重新预审</button>
      <button class="btn sm" id="exportq">${I('msg',13)}导出待确认问题${pend?` (${pend})`:''}</button>
      <button class="btn primary sm" id="fin" ${blockers?'disabled':''} title="${blockers?`还有 ${blockers} 个阻塞问题未处理`:'锁定 PRD 并拆分子任务'}">${I('lock',13)}定稿${blockers?` · 还差 ${blockers} 个阻塞`:''}</button>
    </div>
    <div class="rv-cols">
      <section class="rv-doc"><div class="doc-bar"><span class="tag tide">PRD v3</span><span class="dim">点击高亮处可定位到对应问题</span><span class="grow"></span><button class="btn ghost sm" data-toast="演示：进入编辑，保存后生成新版本">${I('edit',13)}编辑</button></div>
        <div class="prose">${md(PRD42,ISSUES42)}
          <h3>验收标准 <span class="tag tide" style="vertical-align:2px">AI 起草 · 勾选后纳入定稿</span></h3>
          <ul class="ac">${AC42.map(([s,on],i)=>`<li data-ac="${i}"><span class="check ${on?'on':''}">${I('check',11)}</span>${esc(s)}</li>`).join('')}</ul></div></section>
      <section class="rv-iss" id="isslist">${groups}</section>
    </div></div>`;
  $$('[data-ia]').forEach(b=>b.onclick=e=>{e.stopPropagation();const id=b.closest('.iss').dataset.id,i=ISSUES42.find(x=>x.id===id),to=b.dataset.ia;
    const done=()=>{i.state=to;T('TW-42').issues=ISSUES42.filter(x=>x.state==='open').length;tabReview(t,rich);pageTicketBadge();
      toast({adopted:`已采纳 ${id}，生成 PRD 新版本`,manual:`${id} 标记为已手动修改`,pending:`${id} 标记为待确认，可一并导出给需求方`,ignored:`已忽略 ${id}，理由写入「不做范围」`,open:`已撤销 ${id}`}[to])};
    if(to==='ignored')promptBox('忽略这个问题','理由会写入 PRD 的「备注 / 不做范围」。','例如：本期不做',v=>{i.reason=v||'本期不做';done()});else done()});
  $$('mark[data-issue]').forEach(m=>m.onclick=()=>{const el=$(`.iss[data-id="${m.dataset.issue}"]`);if(el){el.scrollIntoView({block:'center',behavior:'smooth'});el.classList.add('flash');setTimeout(()=>el.classList.remove('flash'),1200)}});
  $$('.iss').forEach(el=>el.onmouseenter=()=>{$$('mark.hot').forEach(m=>m.classList.remove('hot'));const m=$(`mark[data-issue="${el.dataset.id}"]`);if(m)m.classList.add('hot')});
  $$('[data-ac]').forEach(li=>li.onclick=()=>{const a=AC42[li.dataset.ac];a[1]=!a[1];$('.check',li).classList.toggle('on',a[1])});
  $('#fin').onclick=openFinalize;
  $('#exportq').onclick=()=>{const qs=ISSUES42.filter(i=>(i.state==='pending'||i.state==='open')&&i.q);infoBox('发给需求方的待确认问题','已整理成一段可以直接发出去的文字。',`关于「优惠券叠加规则与结算页展示」，有 ${qs.length} 点需要确认：\n\n`+qs.map((i,n)=>`${n+1}. ${i.q}`).join('\n'),'复制')};
}
function pageTicketBadge(){const n=ISSUES42.filter(i=>i.state==='open').length,em=$('.tabs a:nth-child(3) em');if(em){if(n)em.textContent=n;else em.remove()}}

function tabTasks(t,rich){
  if(rich==='review'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('branch','定稿后才会拆分子任务','定稿时 AI 会建议拆成后端 / 前端子任务，由你确认后创建分支与 worktree。',`<button class="btn" id="pv">预览拆分建议</button>`)}</div></div>`;$('#pv').onclick=openFinalize;return}
  const list=rich==='dev'?TASKS39:(t.sub||[]).map(([k,s])=>({id:k,kind:k==='FE'?'前端':k==='BUG'?'Bug':'后端',cls:k==='FE'?'fe':k==='BUG'?'bug':'be',proj:t.projects[k==='FE'?t.projects.length-1:0],title:t.title,status:s,statusName:SUB[s][0],agent:PROJ[t.projects[0]].agent,provider:PROJ[t.projects[0]].provider,model:PROV[PROJ[t.projects[0]].provider].model,perm:'workspace_write',branch:`tw/${t.key}-${k.toLowerCase()}`,wt:`D:\\tidewatch\\worktrees\\${t.projects[0]}\\${t.key}-${k.toLowerCase()}`,runs:1,cost:'—',dur:'—',files:0,add:0,del:0}));
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
        <div><label>最近运行</label>${k.lastRun?`<a class="lnk mono" href="#/runs/${k.lastRun}">${k.lastRun}</a> · `:''}共 ${k.runs} 次</div><div><label>耗时 · 费用</label><span class="mono">${k.dur} · ${k.cost}</span></div>
        ${k.artifact?`<div style="grid-column:span 2"><label>产物</label><span class="tag moss">${I('file',11)}${k.artifact}</span> <span class="tag moss">${I('check',11)}测试 ${k.tests}</span> <span class="mono dim">${k.files} 个文件 +${k.add} −${k.del}</span></div>`:''}
      </div>
      <footer>${k.status==='running'?`<a class="btn sm" href="#/runs/${k.lastRun||'R-121'}">${I('term',13)}看实时日志</a><button class="btn danger sm" data-toast="演示：结束进程树，可稍后恢复原会话">${I('stop',13)}停止</button>`
        :k.status==='review'?`<a class="btn primary sm" href="#/ticket/${t.key}/accept">去验收</a>`
        :k.status==='queued'||k.status==='blocked'?`<button class="btn sm" data-toast="演示：已加入运行队列">${I('play',13)}开始</button>`
        :`<button class="btn sm" data-toast="演示：基于 session id 恢复原会话">${I('resume',13)}恢复</button>`}
        <button class="btn ghost sm" data-toast="演示：用 VS Code 打开该 worktree">${I('ext',13)}在 IDE 打开</button>
        <button class="btn ghost sm" data-toast="演示：更换 Agent / Provider / 权限档位">${I('gear',13)}执行配置</button></footer>
    </section>`).join('')}</div></div>`;
}

function tabAccept(t,rich){
  if(rich!=='dev'){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('check','还没有可验收的改动','子任务跑完后，这里会出现 Diff、测试报告和 Agent 总结。请打开 TW-39 查看验收界面。',`<a class="btn" href="#/ticket/TW-39/accept">打开 TW-39</a>`)}</div></div>`;return}
  const comments=[{id:'c1',line:28,text:'「30 天内」要含今天还是从明天算？和产品确认过是含今天，这里对的；但请加一条边界单测。'}];
  const draw=()=>{
    const rows=DIFF_ROWS.map(([ol,ot,nl,nt,k,c])=>`<tr class="${k}"><td class="ln">${ol??''}</td><td class="code l">${k==='add'?'':esc(ot)}</td><td class="ln">${nl??''}</td><td class="code r">${k==='del'?'':esc(nt)}<button class="addc" data-line="${nl}" title="添加行内评论">${I('plus',11)}</button></td></tr>${comments.filter(x=>x.line===nl&&nl).map(x=>`<tr class="cmt"><td colspan="2"></td><td colspan="2"><div class="cbox">${I('msg',13)}<div><b>你</b> · 第 ${x.line} 行<p>${esc(x.text)}</p></div></div></td></tr>`).join('')}`).join('');
    $('#tbody').innerHTML=`<div class="acc">
      <div class="acc-bar"><div class="seg">${TASKS39.map(k=>`<button class="${k.id==='BE'?'on':''}" ${k.id==='FE'?'data-toast="前端子任务还在运行，跑完后可验收"':''}><span class="stamp ${k.cls}" style="margin-right:6px;display:inline-grid">${k.kind[0]}</span>${k.kind} · ${k.statusName}</button>`).join('')}</div>
        <div class="seg"><button class="on">全部改动</button><button data-toast="演示：只看 R-118 这一轮的增量">R-118</button><button data-toast="演示：只看 R-112 这一轮的增量">R-112</button></div>
        <span class="mono dim">7 个文件 <span style="color:var(--moss)">+312</span> <span style="color:var(--cinnabar)">−18</span></span><span class="grow"></span>
        <button class="btn sm" data-toast="演示：用 VS Code 打开 worktree，新提交会自动纳入 Diff">${I('ext',13)}在 IDE 打开</button>
        <button class="btn danger sm" id="rej">${I('undo',13)}打回${comments.length?` · ${comments.length} 条意见`:''}</button>
        <button class="btn primary sm" id="pass">${I('check',13)}通过</button></div>
      <div class="acc-cols">
        <nav class="ftree"><div class="ftree-h">改动文件</div>${DIFF_FILES.map(f=>{const parts=f.p.split('/');return `<a class="${f.sel?'on':''}" title="${f.p}"><span class="fs ${f.s}">${f.s}</span><span class="grow ell"><b>${parts.pop()}</b><small>${parts.slice(-2).join('/')}</small></span>${f.comments?`<span class="cc">${I('msg',11)}${comments.length}</span>`:''}<span class="mono dim">${f.add?'+'+f.add:''}${f.del?' −'+f.del:''}</span></a>`}).join('')}</nav>
        <section class="diff"><div class="diff-h"><span class="mono">shop-api-service/…/points/<b>PointsQueryService.java</b></span><span class="grow"></span><span class="dim">并排对比 · 鼠标移到右侧行尾可添加行内评论</span></div>
          <div class="diff-b"><table>${rows}</table></div></section>
        <aside class="acc-side">
          <div class="sec-title">验收标准<small>${AC39.filter(a=>a[1]).length} / ${AC39.length} 已确认</small></div>
          <ul class="ac">${AC39.map(([s,on],i)=>`<li data-ac="${i}"><span class="check ${on?'on':''}">${I('check',11)}</span>${esc(s)}</li>`).join('')}</ul>
          <div class="sec-title" style="margin-top:18px">自动测试</div>
          <div class="tests"><div><span class="tag moss">${I('check',11)}通过</span><span class="mono">mvn -q spotless:check</span><span class="dim">4s</span></div><div><span class="tag moss">${I('check',11)}通过</span><span class="mono">mvn -q test</span><span class="dim">42 项 · 38s</span></div><div><span class="tag moss">${I('check',11)}通过</span><span class="mono">mvn -q package</span><span class="dim">21s</span></div></div>
          <div class="sec-title" style="margin-top:18px">AI 交叉审查<small>Codex · 只读</small></div>
          <div class="xr"><p><span class="tag amber">提醒</span> <code>summary()</code> 连续发起 3 次查询，可合并为一条 SQL。</p><p><span class="tag">建议</span> <code>size</code> 上限 50 建议抽成配置项。</p></div>
          <div class="sec-title" style="margin-top:18px">Agent 总结</div>
          <p class="sum">新增积分流水游标分页与汇总接口，先提交了 API 文档再实现。改动 7 个文件，补充 6 条单测。<b>需要人工确认</b>：旧的 <code>LegacyPointsController</code> 已删除，未发现其他调用方。</p>
          <div class="cost mono">${I('clock',12)}11 分 20 秒　${I('coin',12)}$ 1.84　206.4k tokens</div>
        </aside></div></div>`;
    $$('[data-ac]').forEach(li=>li.onclick=()=>{const a=AC39[li.dataset.ac];a[1]=!a[1];draw()});
    $$('.addc').forEach(b=>b.onclick=()=>promptBox(`第 ${b.dataset.line} 行 · 行内评论`,'行内评论会汇总进打回意见，作为下一轮运行的输入。','写下希望 Agent 修改的地方',v=>{if(v){comments.push({id:'c'+Date.now(),line:+b.dataset.line,text:v});draw();toast('已添加行内评论')}}));
    $('#rej').onclick=()=>promptBox('打回后端子任务',`将自动创建新的运行：原任务 + 打回意见 + ${comments.length} 条行内评论，优先恢复原 Agent 会话。`,'补充总体意见（可选）',()=>{TASKS39[0].status='running';TASKS39[0].statusName='返工中';T('TW-39').sub[0][1]='running';toast('已打回，新运行 R-125 已入队（恢复会话 5b7a9e13）');location.hash='#/ticket/TW-39/tasks'},'打回并开始返工');
    $('#pass').onclick=()=>{const left=AC39.filter(a=>!a[1]).length;if(left){toast(`还有 ${left} 条验收标准未确认`);return}
      confirmBox('通过后端子任务','子任务标记为已通过。工单的全部子任务通过后，才会合并到 main。','通过',()=>{TASKS39[0].status='accepted';TASKS39[0].statusName='已通过';T('TW-39').sub[0][1]='accepted';toast('后端子任务已通过，等待前端子任务');location.hash='#/ticket/TW-39/tasks'})};
  };draw();
}

function tabRuns(t){
  const rs=RUNS.filter(r=>r.ticket===t.key);
  if(!rs.length){$('#tbody').innerHTML=`<div class="page"><div class="card">${empty('runs','还没有运行记录','开始开发或提交预审后，每次 Agent 运行都会记录在这里。')}</div></div>`;return}
  const tot=rs.reduce((a,r)=>({c:a.c+r.cost,t:a.t+r.tin+r.tout,s:a.s+r.el}),{c:0,t:0,s:0});
  $('#tbody').innerHTML=`<div class="page"><div class="kpis"><div><span>运行次数</span><b>${rs.length}</b></div><div><span>总耗时</span><b>${fmtDur(tot.s)}</b></div><div><span>Token</span><b>${fmtTok(tot.t)}</b></div><div><span>费用</span><b>$ ${tot.c.toFixed(2)}</b></div></div>
    <div class="card" style="overflow:hidden;margin-top:14px"><table class="tbl"><thead><tr><th>运行</th><th>子任务</th><th>Agent</th><th>Provider</th><th>状态</th><th class="num">耗时</th><th class="num">Token</th><th class="num">费用</th></tr></thead>
    <tbody>${rs.map(r=>`<tr data-go="#/runs/${r.id}"><td class="mono">${r.id}</td><td>${r.task} · ${esc(r.title)}</td><td>${agentTag(r.agent)}</td><td>${PROV[r.provider].name}</td><td>${runState(r)}</td><td class="num">${fmtDur(r.el)}</td><td class="num">${fmtTok(r.tin+r.tout)}</td><td class="num">$ ${r.cost.toFixed(2)}</td></tr>`).join('')}</tbody></table></div></div>`;
}
