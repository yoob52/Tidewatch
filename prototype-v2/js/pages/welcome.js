// 首次启动向导：检测环境 → 工作区 → 第一个项目 → Provider → 示例工单
function pageWelcome(step){
  step=+step||1;
  const steps=['检测环境','设置工作区','添加项目','配置 Provider','创建示例工单'];
  const body=[
   `<h2>先看看这台电脑上有什么</h2><p class="lead">观澜只编排你已经装好的命令行 Agent，不会修改它们的全局配置。</p>
    <div class="envlist">${[['git','2.47.0','必需',1],['claude','Claude Code 2.0.14 · 已登录','至少一个 Agent',1],['codex','Codex CLI 0.46.0 · 已登录','至少一个 Agent',1],['node','v22.11.0','前端项目需要',1],['gh','未找到','可选：用于开 PR',0]].map(([n,v,h,ok])=>`<div><span class="tag ${ok?'moss':''}">${I(ok?'check':'min',11)}${ok?'可用':'缺失'}</span><b class="mono">${n}</b><span class="mono dim">${v}</span><span class="grow"></span><span class="dim">${h}</span></div>`).join('')}</div>`,
   `<h2>worktree 放在哪里</h2><p class="lead">每个子任务会在这里得到一个独立的工作目录，你日常开发的目录不受影响。</p>
    <div class="field"><label>工作区路径</label><div class="row"><div class="input mono fake grow">D:\\tidewatch</div><button class="btn">更改</button></div></div>
    <div class="callout tide" style="margin-top:14px">${I('info',15)}<div>建议放在盘符根目录附近，避免 Windows 长路径问题。当前磁盘剩余 212 GB。</div></div>`,
   `<h2>添加第一个项目</h2><p class="lead">可以是本地目录，也可以是 git 地址（会自动 clone 到工作区）。</p>
    <div class="field"><label>本地目录或 git URL</label><div class="input mono fake">D:\\code\\lanzhou\\shop-api</div></div>
    <div class="detected">${I('spark',15)}<div><b>已探测到</b>：pom.xml → Java 21 · Spring Boot 3.3；测试命令 <code>mvn -q test</code>；主分支 <code>main</code>；发现 <code>CLAUDE.md</code>，可导入为上下文卡片。</div></div>`,
   `<h2>用哪个模型渠道</h2><p class="lead">默认沿用本机登录态。也可以现在添加 DeepSeek、中转站或公司网关，之后随时能改。</p>
    <div class="pick col">${[['官方默认（本机登录态）','不注入任何变量，直接用已登录的 Claude / ChatGPT 账号',1],['DeepSeek（Anthropic 兼容）','通过 Claude Code 使用，只需填 Key'],['自定义 Provider','中转站、公司网关或本地模型']].map(([n,d,on])=>`<button class="${on?'on':''}"><b>${n}</b><span class="dim">${d}</span></button>`).join('')}</div>`,
   `<h2>跑一张示例工单试试</h2><p class="lead">我们准备了一张很小的 Bug 工单，会在独立 worktree 里改一行 README，让你看到从创建到合并的完整过程。</p>
    <div class="tk" style="max-width:520px"><div class="row">${stamp('bug')}<span class="key">TW-1</span><b>示例：修正 README 中的错别字</b></div><div class="row dim" style="margin-top:8px">${agentTag('claude')}<span>快速通道 · 预计 1 分钟 · 约 $ 0.02</span></div></div>`][step-1];
  $('#app').innerHTML=`<div class="welcome"><aside><div class="brand"><span class="seal">观澜</span><span><b>观澜</b><small>Tidewatch</small></span></div>
      <p class="motto">观水有术，<br>必观其澜。</p><p class="dim">把需求、Agent 和代码改动放在同一条水流里看：你守住定稿与合并两道闸，其余交给 Agent。</p>
      <ol class="wsteps">${steps.map((s,i)=>`<li class="${i+1<step?'past':i+1===step?'now':''}"><i>${i+1<step?I('check',11):i+1}</i>${s}</li>`).join('')}</ol></aside>
    <main><div class="wbody">${body}</div><footer>${step>1?`<a class="btn" href="#/welcome/${step-1}">上一步</a>`:''}<span class="grow"></span><a class="btn ghost" href="#/board">跳过向导</a>
      <a class="btn primary lg" id="wnext" href="${step<5?`#/welcome/${step+1}`:'#/board'}">${step<5?'下一步':'创建并进入看板'}${I('arrow',14)}</a></footer></main></div>`;
  if(step===5)$('#wnext').onclick=()=>{if(!T('TW-1'))TICKETS.push({key:'TW-1',title:'示例：修正 README 中的错别字',type:'bug',status:'developing',prio:'low',projects:['shop-api'],source:'首次启动向导',updated:'刚刚',sub:[['BUG','queued']],labels:['示例']});toast('已创建示例工单 TW-1，已加入运行队列')};
  $$('.pick.col button').forEach(b=>b.onclick=()=>{$$('button',b.parentNode).forEach(x=>x.classList.remove('on'));b.classList.add('on')});
}
