function pageSettings(){
  const sw=(on,k)=>`<span class="switch ${(k in state.sw?state.sw[k]:on)?'on':''}" data-sw data-k="${k}"></span>`;
  const seg=(k,opts,def)=>`<div class="seg" data-seg="${k}">${opts.map(o=>`<button class="${(state.seg[k]||def)===o?'on':''}">${o}</button>`).join('')}</div>`;
  const row=(t,d,c)=>`<div class="srow"><div><b>${t}</b>${d?`<p>${d}</p>`:''}</div><div class="sctl">${c}</div></div>`;
  const tpl=['prd_review','prd_normalize','task_split','backend_dev','frontend_dev','bug_fix','rework','test_fix','change_impact','code_review'];
  $('#view').innerHTML=`<div class="page flush split">
    <nav class="snav">${[['s-work','工作区与并发'],['s-def','默认执行配置'],['s-auto','自动化'],['s-guard','预算与护栏'],['s-tpl','Prompt 模板'],['s-app','通知与外观'],['s-data','数据与备份']].map(([i,n],k)=>`<a href="javascript:void 0" data-sc="${i}" class="${k?'':'on'}">${n}</a>`).join('')}</nav>
    <section class="sbody" id="sbody">
      <h3 id="s-work">工作区与并发</h3><div class="card">
        ${row('工作区路径','worktree、克隆的仓库、运行日志都放在这里，位于项目目录之外。','<div class="input mono fake w300">D:\\tidewatch</div><button class="btn sm" data-toast="演示：选择目录">更改</button>')}
        ${row('全局并发上限','同时运行的 Agent 数量。同一个 worktree 同一时间只允许一个运行。',`<div class="stepper"><button data-st="-1">−</button><b id="lim">${state.limit}</b><button data-st="1">＋</button></div>`)}
        ${row('按 Provider 限流','应对 API 限流，可为单个 Provider 单独设上限。','<span class="tag line">DeepSeek ≤ 2</span><button class="btn ghost sm" data-toast="演示：添加限流规则">添加</button>')}
        ${row('IDE','「在 IDE 打开」使用的程序。',seg('ide',['VS Code','Cursor','自定义…'],'VS Code'))}</div>
      <h3 id="s-def">默认执行配置</h3><div class="card">
        ${row('默认 Agent','优先级：子任务指定 > 工单指定 > 项目默认 > 全局默认。',seg('agent',['Claude Code','Codex'],'Claude Code'))}
        ${row('开发用 Provider','','<div class="input fake w220">官方默认 · Claude</div>')}
        ${row('预审用 Provider','预审、摘要等轻任务可以用便宜的模型。','<div class="input fake w220">DeepSeek</div>')}
        ${row('开发权限档位','full_auto 每次运行前都需要二次确认。',seg('perm',['read_only','workspace_write','full_auto'],'workspace_write'))}</div>
      <h3 id="s-auto">自动化</h3><div class="card">
        ${row('定稿后自动开始开发','关闭时需要手动点「开始开发」。',sw(true,'s1'))}
        ${row('后端产出 API 文档后解锁前端','检测到 docs/api/&lt;工单号&gt;.md 即入队，不必等后端全部实现。',sw(true,'s2'))}
        ${row('API 文档需人工确认后才解锁前端','',sw(false,'s3'))}
        ${row('运行结束后自动测试','依次执行项目卡片里的 lint、测试、构建命令。',sw(true,'s4'))}
        ${row('测试失败自动修复','把失败输出交回 Agent 继续修，超过次数转人工。',`<div class="stepper"><button data-fx="-1">−</button><b id="fx">${state.num.fix??2}</b><button data-fx="1">＋</button></div><span class="dim">次</span>`)}
        ${row('合并后清理 worktree','运行日志归档保留。',sw(true,'s5'))}</div>
      <h3 id="s-guard">预算与护栏</h3><div class="card">
        ${row('单次运行上限','超出自动停止并通知。','<div class="input fake w100 mono">30 分钟</div><div class="input fake w100 mono">$ 3.00</div>')}
        ${row('每月预算','达到 80% 时提醒。','<div class="input fake w100 mono">$ 50.00</div>')}
        ${row('卡住检测','连续无输出时提醒。','<div class="input fake w100 mono">5 分钟</div>')}
        ${row('高亮危险命令','<code>rm -rf</code>、<code>git push --force</code>、删库语句。',sw(true,'s6'))}</div>
      <h3 id="s-tpl">Prompt 模板</h3><div class="card tplc"><div class="tpll">${tpl.map((t,i)=>`<a class="${i===3?'on':''} mono">${t}</a>`).join('')}</div>
        <pre class="tple">你是本项目的后端开发者。请在当前目录（一个独立的 git worktree）中完成以下任务。

## 项目背景
<i>{{project.context}}</i>

## 需求（已定稿，版本 v<i>{{ticket.finalized_version}}</i>）
<i>{{task.spec}}</i>

## 验收标准
<i>{{task.acceptance_criteria}}</i>

## 工作要求
1. 第一步：在 \`docs/api/<i>{{ticket.key}}</i>.md\` 写出本需求涉及的全部接口文档，完成后单独提交一次。
2. 然后实现功能，并补充必要的单元测试。
3. 运行 \`<i>{{project.commands.test}}</i>\` 确保通过。
4. 不要修改与本任务无关的文件；不要推送到远程。</pre></div>
      <h3 id="s-app">通知与外观</h3><div class="card">
        ${row('Windows 通知','预审完成、需人工验收、运行失败、预算超限。',sw(true,'s7'))}
        ${row('主题','',`<div class="seg" id="thseg"><button class="${state.theme==='light'?'on':''}" data-th="light">${I('sun',13)}亮色</button><button class="${state.theme==='dark'?'on':''}" data-th="dark">${I('moon',13)}暗色</button></div>`)}
        ${row('全局快捷键','','<span class="row"><kbd>Ctrl</kbd><kbd>Shift</kbd><kbd>T</kbd><span class="dim">从剪贴板新建草稿</span></span><span class="row"><kbd>Ctrl</kbd><kbd>K</kbd><span class="dim">命令面板</span></span>')}</div>
      <h3 id="s-data">数据与备份</h3><div class="card">
        ${row('数据库','全部数据保存在本机 SQLite。','<span class="mono dim">D:\\tidewatch\\tidewatch.db · 18.4 MB</span>')}
        ${row('导出','导出数据库，或把工单导出为 Markdown。','<button class="btn sm" data-toast="演示：已导出到 D:\\tidewatch\\backup">立即备份</button><button class="btn sm" data-toast="演示：导出工单为 Markdown">导出 Markdown</button>')}
        ${row('首次启动向导','重新走一遍环境检测与初始配置。','<a class="btn sm" href="#/welcome">打开向导</a>')}</div>
    </section></div>`;
  $$('[data-st]').forEach(b=>b.onclick=()=>{state.limit=Math.max(1,Math.min(6,state.limit+ +b.dataset.st));$('#lim').textContent=state.limit;promoteQueue();route()});
  $$('[data-seg] button').forEach(b=>b.onclick=()=>{const k=b.parentNode.dataset.seg,pick=()=>{state.seg[k]=b.textContent;$$('button',b.parentNode).forEach(x=>x.classList.toggle('on',x===b))};
    if(b.textContent==='full_auto'&&state.seg[k]!=='full_auto')confirmBox('开启 full_auto','Agent 将不经确认执行任意命令。建议只在可丢弃的 worktree 中使用。','我了解风险，开启',pick);else pick()});
  $$('[data-fx]').forEach(b=>b.onclick=()=>{state.num.fix=Math.max(0,Math.min(5,(state.num.fix??2)+ +b.dataset.fx));$('#fx').textContent=state.num.fix});
  $$('[data-th]').forEach(b=>b.onclick=()=>{setTheme(b.dataset.th);pageSettings()});
  $$('[data-sc]').forEach(a=>a.onclick=()=>{$$('.snav a').forEach(x=>x.classList.remove('on'));a.classList.add('on');$('#'+a.dataset.sc).scrollIntoView({behavior:'smooth',block:'start'})});
  $$('.tpll a').forEach(a=>a.onclick=()=>{$$('.tpll a').forEach(x=>x.classList.remove('on'));a.classList.add('on');toast(`演示：切换到模板 ${a.textContent}`)});
}
