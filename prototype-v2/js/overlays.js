// 浮层：命令面板、通知、确认框、定稿与拆分
const ov=()=>$('#overlay');
function closeOv(){ov().innerHTML='';ov().className=''}
function confirmBox(title,body,ok,fn){
  ov().innerHTML=`<div class="pop" style="width:440px"><div class="pop-h"><h2>${title}</h2><p>${body}</p></div><div class="pop-b"></div><div class="pop-f"><button class="btn" data-x>取消</button><button class="btn primary" id="ok">${ok}</button></div></div>`;
  $('#ok').onclick=()=>{closeOv();fn()};$('#ok').focus();
}
function promptBox(title,body,ph,fn,ok='确定'){
  ov().innerHTML=`<div class="pop" style="width:480px"><div class="pop-h"><h2>${title}</h2><p>${body}</p></div><div class="pop-b"><textarea class="textarea" id="pv" rows="3" placeholder="${ph}"></textarea></div><div class="pop-f"><button class="btn" data-x>取消</button><button class="btn primary" id="ok">${ok}</button></div></div>`;
  $('#ok').onclick=()=>{const v=$('#pv').value.trim();closeOv();fn(v)};$('#pv').focus();
}
function infoBox(title,body,text,ok){
  ov().innerHTML=`<div class="pop" style="width:520px"><div class="pop-h"><h2>${title}</h2><p>${body}</p></div><div class="pop-b"><pre class="plain boxed">${esc(text)}</pre></div><div class="pop-f"><button class="btn" data-x>关闭</button><button class="btn primary" id="ok">${ok}</button></div></div>`;
  $('#ok').onclick=()=>{closeOv();toast('已复制到剪贴板')};
}
function openFinalize(){
  const blockers=ISSUES42.filter(i=>i.sev==='blocker'&&i.state==='open').length,ac=AC42.filter(a=>a[1]).length;
  ov().innerHTML=`<div class="pop fin" style="width:720px;margin-top:-6vh"><div class="pop-h"><h2>定稿并拆分子任务</h2><p>定稿会锁定 PRD v3。之后再改需求，需要「发起变更」并重新审核。</p></div>
    <div class="pop-b">
      <div class="gate"><div class="${blockers?'bad':'ok'}">${I(blockers?'x':'check',13)}${blockers?`还有 ${blockers} 个阻塞问题未处理`:'阻塞问题已全部处理'}</div><div class="${ac?'ok':'bad'}">${I('check',13)}验收标准 ${ac} 条</div><div class="ok">${I('check',13)}已绑定后端与前端项目</div></div>
      <div class="sec-title" style="margin-top:16px">${I('spark',15)}AI 拆分建议<small>可改标题、项目、Agent，或合并 / 拆开</small></div>
      <div class="splits">${SPLIT42.map((s,i)=>`${i?`<div class="dep-line"><span>${I('link',12)}前端等后端的 API 文档 <code>docs/api/TW-42.md</code> 出现后解锁，不必等后端全部实现</span></div>`:''}
        <div class="sp-item"><span class="stamp ${s.cls} lg">${s.kind==='BE'?'后':'前'}</span><div class="grow"><input class="input" value="${s.title}"><div class="row" style="margin-top:8px;flex-wrap:wrap"><span class="tag line">${I('folder',11)}${s.proj}</span>${agentTag(s.agent)}<span class="mono dim">${I('branch',12)}</span><span class="mono dim">${s.branch}</span><span class="tag">${s.ac} 条验收标准</span>${s.first?`<span class="tag tide">${s.first}</span>`:''}</div></div><button class="iconbtn" data-toast="演示：更换项目 / Agent / Provider">${I('gear')}</button></div>`).join('')}</div>
      <label class="row" style="margin-top:14px;cursor:pointer"><span class="check on" id="auto">${I('check',11)}</span>定稿后立即开始开发（当前并发 3/3，后端子任务会先排队）</label>
    </div>
    <div class="pop-f"><button class="btn" data-x>再看看</button><button class="btn primary" id="ok" ${blockers?'disabled':''}>${I('lock',14)}确认定稿，创建 2 个分支与 worktree</button></div></div>`;
  $('#auto').parentNode.onclick=()=>$('#auto').classList.toggle('on');
  $('#ok').onclick=()=>{if(blockers)return;const t=T('TW-42'),go=$('#auto').classList.contains('on');closeOv();t.status=go?'developing':'finalized';t.issues=0;t.sub=[['BE','queued'],['FE','blocked']];t.updated='刚刚';
    toast('TW-42 已定稿（v3），已创建 tw/TW-42-be 与 tw/TW-42-fe');location.hash='#/board'};
}
function openCmd(){
  const items=[['跳转',[['board','看板','#/board','G B'],['runs','运行面板','#/runs','G R'],['plug','Provider','#/providers'],['gear','设置','#/settings']]],
    ['操作',[['plus','新建工单','#/new','N'],['paste','从剪贴板新建草稿','#/new','Ctrl Shift T'],['moon','切换亮色 / 暗色','@theme'],['stop','停止全部运行','@stopall']]],
    ['工单',TICKETS.map(t=>['file',`${t.key}　${t.title}`,`#/ticket/${t.key}`,STAGE[t.status].name])],
    ['项目',PROJECTS.map(x=>['folder',x.name,`#/projects/${x.id}`,x.kindName])]];
  ov().innerHTML=`<div class="pop cmd"><div class="cmd-in">${I('search',18)}<input id="cq" placeholder="搜索工单、项目，或输入命令…" autocomplete="off"><kbd>Esc</kbd></div><div class="cmd-list" id="cl"></div></div>`;
  const draw=q=>{let first=true;$('#cl').innerHTML=items.map(([g,l])=>{const f=l.filter(x=>!q||x[1].toLowerCase().includes(q.toLowerCase())).slice(0,q?20:(g==='工单'?6:4));return f.length?`<div class="cmd-g">${g}</div>`+f.map(([ic,n,h,k])=>{const on=first;first=false;return `<a class="cmd-i ${on?'on':''}" data-h="${h}">${I(ic)}<span>${esc(n)}</span>${k?`<span class="k">${k.length>5&&!k.includes(' ')?`<span class="dim">${k}</span>`:k.split(' ').map(x=>`<kbd>${x}</kbd>`).join('')}</span>`:''}</a>`}).join(''):''}).join('')||'<div class="cmd-g">没有匹配的结果</div>';
    $$('.cmd-i').forEach(a=>a.onclick=()=>run(a.dataset.h))};
  const run=h=>{closeOv();if(h==='@theme')setTheme(state.theme==='dark'?'light':'dark'),route();else if(h==='@stopall')confirmBox('停止全部运行','将结束 3 个正在运行的 Agent 进程树，可稍后逐个恢复。','全部停止',()=>toast('演示：已停止 3 个运行'));else location.hash=h};
  draw('');const q=$('#cq');q.focus();q.oninput=()=>draw(q.value);
  q.onkeydown=e=>{const l=$$('.cmd-i'),i=l.findIndex(x=>x.classList.contains('on'));
    if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();l[i]?.classList.remove('on');const n=l[(i+(e.key==='ArrowDown'?1:-1)+l.length)%l.length];n.classList.add('on');n.scrollIntoView({block:'nearest'})}
    if(e.key==='Enter'&&l[i])run(l[i].dataset.h)};
}
function openNotes(){
  state.note=false;const pip=$('.iconbtn .pip');if(pip)pip.remove();
  ov().className='clear';
  ov().innerHTML=`<div class="drawer"><div class="drawer-h"><h2>通知</h2><button class="btn ghost sm" data-x>全部已读</button><button class="iconbtn" data-x>${I('x')}</button></div>
    ${NOTES.map(n=>`<a class="note" href="${n.go}" data-x><span class="tag ${n.cls}" style="width:22px;height:22px;padding:0;justify-content:center">${I(n.icon,13)}</span><div><b>${n.title}</b><p>${n.body}</p><time>${n.time}</time></div></a>`).join('')}
    <p class="dim" style="padding:14px 16px">运行完成、失败或需要你处理时，也会弹出 Windows 通知。</p></div>`;
}
