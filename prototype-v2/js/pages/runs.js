// 运行面板：左列表（运行中置顶），右日志时间线，底部状态条
const RSTATE={running:['运行中','tide'],queued:['排队','' ],done:['完成','moss'],failed:['失败','cin'],stopped:['已停止','amber']};
function runState(r){const[n,c]=RSTATE[r.status];return `<span class="tag ${c}">${r.status==='running'?ripple:''}${n}</span>`}
let runTimer=null,liveIdx=0,liveClock=252;
function logItem(e,fresh){
  const t=`<time>${e.t||fmtDur(liveClock)}</time>`,cls=fresh?' fresh':'';
  if(e.k==='sys')return `<div class="logi sys${cls}">${t}<div>${I('info',12)}${esc(e.text)}</div></div>`;
  if(e.k==='msg')return `<div class="logi msg${cls}">${t}<div class="bubble">${inline(e.text)}</div></div>`;
  if(e.k==='prompt')return `<div class="logi${cls}">${t}<details class="toolc"><summary>${I('right',12)}<b>Prompt</b><span class="arg">${esc(e.title)}</span></summary><pre class="plain">${esc(e.body)}</pre></details></div>`;
  if(e.k==='edit')return `<div class="logi${cls}">${t}<div class="toolc flat">${I('edit',12)}<b>修改</b><span class="arg mono">${esc(e.file)}</span><span class="mono" style="color:var(--moss)">+${e.add}</span>${e.del?`<span class="mono" style="color:var(--cinnabar)">−${e.del}</span>`:''}</div></div>`;
  if(e.term)return `<div class="logi${cls}">${t}<details class="toolc" ${e.bad||fresh?'open':''}><summary>${I('right',12)}<b>${e.tool}</b><span class="arg mono">${esc(e.arg)}</span>${e.bad?'<span class="tag cin">退出码 2</span>':'<span class="tag moss">退出码 0</span>'}</summary><pre class="term">${esc(e.term)}</pre></details></div>`;
  return `<div class="logi${cls}">${t}<div class="toolc flat">${I(e.tool==='搜索'?'search':'file',12)}<b>${e.tool}</b><span class="arg mono">${esc(e.arg)}</span><span class="dim">${e.out}</span></div></div>`;
}
function pageRuns(id){
  clearInterval(runTimer);
  const r=RUN(id)||RUNS[0],live=r.id==='R-121'&&r.status==='running';
  const item=x=>`<a class="ri ${x.id===r.id?'on':''}" href="#/runs/${x.id}">
    <div class="row">${agentGlyph(x.agent)}<span class="key">${x.ticket}</span><span class="tag line">${x.task}</span><span class="grow"></span><span class="mono ${x.status==='running'?'t-live':'dim'}" ${x.status==='running'?`data-el="${x.id}"`:''}>${x.status==='queued'?'排队':fmtDur(x.el)}</span></div>
    <b class="ell">${esc(x.title)}</b>
    <div class="row sub">${x.status==='running'?ripple:`<span class="dot st-${x.status}"></span>`}<span class="grow ell" ${x.status==='running'?`data-now="${x.id}"`:''}>${esc(x.now)}</span><span class="mono" ${x.status==='running'?`data-tok="${x.id}"`:''}>${fmtTok(x.tin+x.tout)}</span></div></a>`;
  const act=RUNS.filter(x=>x.status==='running'||x.status==='queued'),hist=RUNS.filter(x=>!act.includes(x));
  const body=r.id==='R-121'?LOG.map(e=>logItem(e)).join('')+LOG_LIVE.slice(0,liveIdx).map(e=>logItem(e)).join('')
    :`<div class="logi sys"><time>00:00</time><div>${I('info',12)}已创建运行 · ${AGENTS[r.agent].name} · ${r.model} · ${r.perm}</div></div><div class="logi msg"><time>00:02</time><div class="bubble">${r.status==='queued'?'等待空闲的并发名额。':'演示数据只为 R-121 准备了完整日志，请在左侧选择 R-121 查看实时效果。'}</div></div>`;
  $('#view').innerHTML=`<div class="page flush runs">
    <aside class="rlist"><div class="rl-h"><span>进行中</span><i>${act.length}</i></div>${act.map(item).join('')}<div class="rl-h"><span>历史</span><i>${hist.length}</i></div>${hist.map(item).join('')}</aside>
    <section class="rmain">
      <header class="rhead"><div class="grow"><div class="row"><a class="key lnk" href="#/ticket/${r.ticket}">${r.ticket}</a><span class="tag line">${r.task}</span><span id="rstate">${runState(r)}</span><span class="mono dim">${r.id}</span><span class="tag line">${I('shield',11)}${r.perm}</span></div><h2>${esc(r.title)}</h2></div>
        ${r.status==='running'?`<button class="btn danger" id="stop">${I('stop',14)}停止</button>`:r.status==='queued'?`<button class="btn" id="dequeue">取消排队</button>`:`<button class="btn" id="resume">${I('resume',14)}${r.status==='stopped'?'恢复':'继续追问'}</button>`}
        <button class="btn" id="append">${I('msg',14)}追加指令</button>
        <button class="btn ghost" data-toast="演示：切换为 stream-json 原始事件视图">${I('code',14)}原始日志</button>
        <button class="btn ghost" data-toast="演示：在资源管理器中打开 ${r.wt?esc(r.wt).replace(/\\/g,'\\\\'):''}">${I('folder',14)}打开 worktree</button></header>
      <div class="rlog" id="rlog">${body}${live?`<div class="logi typing" id="typing"><time></time><div>${ripple}<span data-now="${r.id}">${esc(r.now)}</span></div></div>`:''}</div>
      <footer class="rfoot mono"><span>${agentTag(r.agent)}</span><span><i>模型</i>${r.model}</span><span><i>Provider</i>${PROV[r.provider].name}</span><span title="${r.session}"><i>session</i>${r.session.slice(0,8)}${r.session.length>8?'…':''}</span><span class="grow"></span>
        <span><i>耗时</i><b ${r.status==='running'?`data-el="${r.id}"`:''}>${fmtDur(r.el)}</b></span><span><i>输入</i><b data-in>${fmtTok(r.tin)}</b></span><span><i>输出</i><b data-out>${fmtTok(r.tout)}</b></span><span><i>费用</i><b data-cost>$ ${r.cost.toFixed(2)}</b></span></footer>
    </section></div>`;
  const log=$('#rlog');log.scrollTop=log.scrollHeight;
  const ap=$('#append');ap.onclick=()=>promptBox('追加指令',r.status==='running'?'会先停止当前进程，再以 resume + 新指令继续同一会话。':'以 resume + 新指令继续同一会话。','例如：把即将过期的角标改成橙色',v=>{if(v)toast(`已追加指令，恢复会话 ${r.session.slice(0,8)}`)},'发送');
  const st=$('#stop');if(st)st.onclick=()=>confirmBox('停止运行',`将结束 ${r.id} 的整个进程树。已有改动保留在 worktree，可稍后恢复原会话。`,'停止',()=>{r.status='stopped';r.now='已人工停止 · 可恢复';syncRun(r);promoteQueue();toast(`${r.id} 已停止`);route()});
  const dq=$('#dequeue');if(dq)dq.onclick=()=>{r.status='stopped';r.now='已取消排队 · 可重新开始';syncRun(r);toast(`${r.id} 已移出队列`);route()};
  const rs=$('#resume');if(rs)rs.onclick=()=>{if(r.status!=='stopped'){toast(`演示：基于 session ${r.session.slice(0,8)} 继续追问`);return}
    const free=counts().running<state.limit;r.status=free?'running':'queued';r.now=free?`已恢复会话 ${r.session.slice(0,8)}`:`排队中 · 并发已满 ${state.limit}/${state.limit}`;syncRun(r);toast(free?`${r.id} 已恢复原会话`:`${r.id} 已入队，等待空闲并发`);route()};
  // 模拟实时：计时、Token 增长、日志逐条流入
  let n=0;
  runTimer=setInterval(()=>{
    if(!$('.runs')){clearInterval(runTimer);return}
    n++;
    RUNS.filter(x=>x.status==='running').forEach(x=>{x.el++;x.tin+=Math.round(60+Math.random()*240);x.tout+=Math.round(8+Math.random()*40);x.cost+=0.0009;
      $$(`[data-el="${x.id}"]`).forEach(el=>el.textContent=fmtDur(x.el));$$(`[data-tok="${x.id}"]`).forEach(el=>el.textContent=fmtTok(x.tin+x.tout))});
    if(r.status==='running'){$('[data-in]').textContent=fmtTok(r.tin);$('[data-out]').textContent=fmtTok(r.tout);$('[data-cost]').textContent='$ '+r.cost.toFixed(2)}
    if(live&&n%4===0&&liveIdx<LOG_LIVE.length){
      liveClock=r.el;const e=LOG_LIVE[liveIdx++],ty=$('#typing');e.t=e.t||fmtDur(r.el); // 记下流入时刻，重新进入页面时照原样重放
      const stick=log.scrollHeight-log.scrollTop-log.clientHeight<80;
      ty.insertAdjacentHTML('beforebegin',logItem(e,true));
      const nx=LOG_LIVE[liveIdx];r.now=nx?(nx.k==='edit'?'编辑 '+nx.file:nx.k==='tool'?'运行 '+nx.arg:'思考中'):'自动测试中';
      $$(`[data-now="${r.id}"]`).forEach(el=>el.textContent=r.now);
      if(liveIdx===LOG_LIVE.length){r.status='done';r.now='完成 · 测试 5 通过';ty.remove();$('#rstate').innerHTML=runState(r);Object.assign(TASKS39[1],{files:5,add:201,del:2,tests:'5 通过',dur:fmtDur(r.el).replace(':',' 分 ')+' 秒',cost:'$'+r.cost.toFixed(2)});syncRun(r);promoteQueue();toast('R-121 运行完成，TW-39 前端子任务待验收');setTimeout(()=>{if($('.runs'))route()},1500)}
      if(stick)log.scrollTop=log.scrollHeight;
    }
  },1000);
}
