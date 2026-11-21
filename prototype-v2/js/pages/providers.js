function pageProviders(id){
  const p=PROV[id]||PROV.deepseek,official=p.protocol==='—';
  $('#view').innerHTML=`<div class="page flush split">
    <aside class="plist wide"><div class="row" style="padding:14px 14px 8px"><b class="grow">${PROVIDERS.length} 个 Provider</b><button class="btn sm" id="addp">${I('plus',13)}添加</button></div>
      ${PROVIDERS.map(x=>`<a class="pi ${x.id===p.id?'on':''}" href="#/providers/${x.id}"><div class="row"><b>${x.name}</b>${x.def?'<span class="tag tide">全局默认</span>':''}<span class="grow"></span><span class="dot" style="color:var(--${x.ok?'moss':'amber'})"></span></div><small class="mono ell">${x.url}</small>
        <div class="row" style="margin-top:6px"><span class="tag line">${x.protocol==='—'?'本机登录态':x.protocol}</span><span class="grow"></span><small class="mono">${x.tokens} · ${x.cost}</small></div></a>`).join('')}
      <div class="callout tide" style="margin:12px 14px">${I('shield',15)}<div>Provider 只在启动 Agent 子进程时按进程注入，不会改动 <code>~/.claude</code> 与 <code>~/.codex</code>。</div></div></aside>
    <section class="pdetail narrow">
      <div class="row"><h2>${p.name}</h2><span class="grow"></span>${p.def?'':'<button class="btn sm" data-toast="演示：已设为全局默认">设为全局默认</button>'}<button class="btn danger sm" data-toast="演示：删除前会检查是否有项目在用">删除</button></div>
      ${p.warn?`<div class="callout" style="margin-top:12px">${I('alert',15)}<div><b>${p.warn}</b><br><a class="lnk" data-toast="演示：显示 LiteLLM 配置示例">查看 LiteLLM 配置示例 →</a></div></div>`:''}
      <div class="form" style="margin-top:16px">
        <div class="field"><label>模板</label><div class="pick">${['官方默认（本机登录态）','DeepSeek（Anthropic 兼容）','Anthropic API','OpenAI API','自定义 Anthropic 兼容','自定义 OpenAI 兼容','本地 LiteLLM 桥接'].map(n=>`<button class="${n===p.tpl?'on':''}">${n}</button>`).join('')}</div></div>
        ${official?`<div class="callout tide">${I('info',15)}<div>使用本机已登录的账号，不注入任何环境变量。适合 Claude 订阅 / ChatGPT 登录。</div></div>`:`
        <div class="form-grid"><div class="field"><label>显示名</label><input class="input" value="${p.name}"></div><div class="field"><label>协议</label><div class="seg">${['anthropic','openai_responses','openai_chat'].map(x=>`<button class="${x===p.protocol?'on':''}">${x}</button>`).join('')}</div></div></div>
        <div class="field"><label>Base URL</label><input class="input mono" value="${p.url}"></div>
        <div class="field"><label>API Key <small>${I('lock',11)} 保存在 Windows 凭据管理器，数据库只存引用</small></label><input class="input mono" value="${p.key||''}"></div>
        <div class="form-grid"><div class="field"><label>默认模型</label><input class="input mono" value="${p.model}"></div><div class="field"><label>小 / 快模型 <small>预审、摘要等轻任务</small></label><input class="input mono" value="${p.fast}"></div></div>
        <div class="form-grid"><div class="field"><label>价格 <small>用于估算费用，可选</small></label><input class="input" value="${p.price||''}" placeholder="输入 / 输出，每百万 Token"></div><div class="field"><label>HTTP 代理 <small>可选</small></label><input class="input mono" placeholder="http://127.0.0.1:7890"></div></div>`}
        <div class="field"><label>注入预览 <small>${p.agents.length?'仅作用于本次子进程':'当前没有可直接使用的 Agent'}</small></label><pre class="term small">${official?'# 不注入任何变量，沿用本机登录态':p.protocol==='anthropic'?`ANTHROPIC_BASE_URL=${p.url}\nANTHROPIC_AUTH_TOKEN=••••••••\nANTHROPIC_MODEL=${p.model}\nANTHROPIC_SMALL_FAST_MODEL=${p.fast}`:`codex exec --json \\\n  -c model_provider=${p.id} \\\n  -c model_providers.${p.id}.base_url="${p.url}" \\\n  -c model_providers.${p.id}.wire_api="${p.protocol==='openai_chat'?'chat':'responses'}" \\\n  -c model_providers.${p.id}.env_key=TW_PROVIDER_KEY`}</pre></div>
        <div class="testrow"><button class="btn" id="test">${I('zap',14)}测试连接</button><button class="btn" id="test2">${I('term',14)}用 Agent 测试</button><span id="tres">${p.latency?`<span class="tag moss">${I('check',11)}上次测试通过</span><span class="dim mono">${p.latency} ms</span>`:official?'<span class="dim">沿用本机登录态，无需测试 Key</span>':'<span class="dim">尚未测试</span>'}</span><span class="grow"></span><button class="btn primary" data-toast="演示：已保存">保存</button></div>
      </div>
      <div class="kpis" style="margin-top:22px"><div><span>累计 Token</span><b>${p.tokens}</b></div><div><span>累计费用</span><b>${p.cost}</b></div><div><span>可用 Agent</span><b style="font:500 13px var(--font-sans)">${p.agents.map(a=>AGENTS[a].name.split(' · ')[0]).join('、')||'需桥接'}</b></div></div>
    </section></div>`;
  $$('.pick button,.form .seg button').forEach(b=>b.onclick=()=>{$$('button',b.parentNode).forEach(x=>x.classList.remove('on'));b.classList.add('on')});
  const run=(msg,ok)=>{$('#tres').innerHTML=`${ripple}<span class="dim">${msg}</span>`;setTimeout(()=>{if($('#tres'))$('#tres').innerHTML=ok},1300)};
  $('#test').onclick=()=>run('正在发送最小请求…',p.ok?`<span class="tag moss">${I('check',11)}连接正常</span><span class="dim mono">${p.latency||236} ms · ${p.model}</span>`:`<span class="tag amber">${I('alert',11)}URL 与 Key 可用，但协议与 Codex 不匹配</span>`);
  $('#test2').onclick=()=>run('正在用该 Provider 启动 Agent 执行 echo 任务…',p.ok?`<span class="tag moss">${I('check',11)}端到端可用</span><span class="dim mono">3.4 s</span>`:`<span class="tag cin">${I('x',11)}Agent 启动失败：需经 LiteLLM 桥接</span>`);
  $('#addp').onclick=()=>toast('演示：选择模板 → 填 Key → 测试连接');
}
