function pageProjects(id){
  const p=PROJ[id]||PROJECTS[0];
  $('#view').innerHTML=`<div class="page flush split">
    <aside class="plist"><div class="row" style="padding:14px 14px 8px"><b class="grow">${PROJECTS.length} 个项目</b><button class="btn sm" data-toast="演示：选择本地目录，或填 git URL 自动 clone 到工作区">${I('plus',13)}注册项目</button></div>
      ${PROJECTS.map(x=>`<a class="pi ${x.id===p.id?'on':''}" href="#/projects/${x.id}"><div class="row"><b class="mono">${x.name}</b><span class="tag ${x.kind==='backend'?'':'line'}">${x.kindName}</span><span class="grow"></span>${x.health.every(h=>h[1])?`<span class="dot" style="color:var(--moss)"></span>`:`<span class="dot" style="color:var(--amber)"></span>`}</div><small class="mono ell">${x.path}</small><small>${x.open} 张进行中的工单</small></a>`).join('')}</aside>
    <section class="pdetail">
      <div class="row" style="margin-bottom:4px"><h2 class="mono">${p.name}</h2><span class="tag">${p.kindName}</span><span class="grow"></span><button class="btn sm" data-toast="演示：用 VS Code 打开项目目录">${I('ext',13)}在 IDE 打开</button><button class="btn sm" data-toast="演示：重新检查目录、git 状态与主分支">${I('resume',13)}重新检查</button></div>
      <p class="dim">${p.stack}</p>
      <div class="pgrid">
        <div class="card"><div class="card-h"><h3>基本信息</h3></div><div class="card-b"><dl class="props wide"><dt>路径</dt><dd class="mono">${p.path}</dd><dt>默认分支</dt><dd class="mono">${I('branch',12)} ${p.branch}</dd><dt>默认 Agent</dt><dd>${agentTag(p.agent)}</dd><dt>默认 Provider</dt><dd>${PROV[p.provider].name}</dd></dl></div></div>
        <div class="card"><div class="card-h"><h3>健康检查</h3><span class="grow"></span><span class="dim">刚刚</span></div><div class="card-b hc">${p.health.map(([n,ok])=>`<div><span class="tag ${ok?'moss':'amber'}">${I(ok?'check':'alert',11)}${ok?'正常':'注意'}</span>${n}${ok?'':'<span class="dim">有 2 个未提交的文件，不影响 worktree 运行</span>'}</div>`).join('')}</div></div>
        <div class="card span2"><div class="card-h"><h3>上下文卡片</h3><span class="dim">每次运行自动拼入 Prompt，不改写仓库里的 CLAUDE.md / AGENTS.md</span><span class="grow"></span><button class="btn sm" data-toast="演示：读取仓库中的 AGENTS.md / CLAUDE.md 并填入">${I('upload',13)}从 AGENTS.md / CLAUDE.md 导入</button><button class="btn sm" data-toast="演示：进入 Markdown 编辑">${I('edit',13)}编辑</button></div>
          <div class="card-b prose sm">${md(p.context)}</div></div>
        <div class="card span2"><div class="card-h"><h3>命令配置</h3><span class="dim">运行结束后在 worktree 中依次执行；同时作为 workspace_write 档位的命令白名单</span></div>
          <div class="card-b cmds">${[['准备','prepare','worktree 首次创建后'],['Lint','lint','自动测试'],['测试','test','自动测试'],['构建','build','自动测试']].map(([n,k,h])=>`<div><label>${n}<small>${h}</small></label><div class="input mono fake">${p.cmds[k]}</div><span class="switch ${state.sw[p.id+k]===false?'':'on'}" data-sw data-k="${p.id+k}"></span></div>`).join('')}</div></div>
      </div></section></div>`;
}
