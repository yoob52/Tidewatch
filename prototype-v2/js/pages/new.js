// 新建工单：左边放原文，右边是「智能识别」的预填结果
const SAMPLE_PASTE=`【bug】订单导出的 Excel，用 WPS 打开中文表头全是乱码，Office 打开正常。

复现：
1. 后台 订单管理 → 勾选任意订单 → 导出
2. 用 WPS 11.1 打开下载的 xlsx

期望：表头显示「订单号 / 下单时间 / 门店」
实际：显示成 è®¢å•å· 这样的乱码

环境：生产，shop-api 2.14.0。上周三发版后出现的，运营催得比较急。`;
function pageNew(){
  let mode='paste',recognized=false;
  const render=()=>{
    const left={
      paste:`<textarea class="textarea paste" id="raw" placeholder="把聊天记录、PRD 或 Bug 描述整段粘贴到这里…\n\n也可以在任意位置按 Ctrl+Shift+T，直接用剪贴板内容新建草稿。">${recognized?esc(SAMPLE_PASTE):''}</textarea>
        <div class="row" style="margin-top:10px"><button class="btn sm" id="fill">${I('paste',13)}粘贴示例内容</button><span class="dim">支持 Markdown；图片会保存为附件</span></div>`,
      file:`<div class="drop">${I('upload',26)}<b>把文件拖到这里，或点击选择</b><span>.md · .docx · .pdf，转为 Markdown 并保留图片附件</span><button class="btn sm" data-toast="演示：此处会打开系统文件选择框">选择文件</button></div>
        <div class="filerow">${I('file',15)}<span class="grow"><b>发票需求v2.docx</b><span class="dim"> · 1.2 MB · 已转换，3 张图片</span></span><span class="tag moss">${I('check',11)}就绪</span></div>`,
      form:`<div class="form-grid" style="grid-template-columns:1fr"><div class="field"><label>标题</label><input class="input" placeholder="一句话说明要做什么"></div>
        <div class="field"><label>需求正文 <small>Markdown</small></label><textarea class="textarea" rows="13" placeholder="## 背景与目标\n\n## 功能需求\n\n## 验收标准"></textarea></div></div>`,
    }[mode];
    const rec=recognized?`
      <div class="field"><label>标题</label><input class="input" value="订单导出 Excel 在 WPS 中表头乱码"></div>
      <div class="form-grid" style="grid-template-columns:1fr">
        <div class="field"><label>类型</label><div class="pick">${Object.entries(TYPES).map(([k,v])=>`<button class="${k==='bug'?'on':''}">${stamp(k)}${v.name}</button>`).join('')}</div></div>
        <div class="field"><label>优先级</label><div class="pick">${Object.entries(PRIO).map(([k,v])=>`<button class="${k==='high'?'on':''}">${prio(k)}</button>`).join('')}</div></div>
      </div>
      <div class="field"><label>绑定项目 <small>依据「shop-api 2.14.0」「导出」</small></label><div class="pick">${PROJECTS.map(p=>`<button class="${p.id==='shop-api'?'on':''}">${I('folder',13)}${p.name}<span class="dim">${p.kindName}</span></button>`).join('')}</div></div>
      <div class="bugbox"><div class="bugbox-h">${stamp('bug')}已拆出 Bug 信息</div>
        <dl><dt>复现步骤</dt><dd>1. 后台 订单管理 → 勾选任意订单 → 导出<br>2. 用 WPS 11.1 打开下载的 xlsx</dd>
        <dt>期望</dt><dd>表头显示「订单号 / 下单时间 / 门店」</dd><dt>实际</dt><dd>显示为乱码 <code>è®¢å•å·</code></dd><dt>环境</dt><dd>生产 · shop-api 2.14.0</dd></dl></div>
      <div class="form-grid"><div class="field"><label>Agent</label><div class="input fake">${agentTag('claude')}<span class="dim" style="margin-left:auto">项目默认</span></div></div>
        <div class="field"><label>Provider</label><div class="input fake">官方默认 · Claude<span class="dim" style="margin-left:auto">项目默认</span></div></div></div>`
      :`<div class="rec-empty">${I('spark',22)}<b>粘贴或导入后，这里会自动预填</b><span>标题、类型、绑定项目、优先级；如果是 Bug，会拆出复现步骤、期望与实际。所有预填都可以改。</span></div>`;
    $('#view').innerHTML=`<div class="page flush newpage">
      <div class="new-l">
        <div class="seg lgseg">${[['paste','paste','粘贴'],['file','upload','导入文件'],['form','form','表单']].map(([k,ic,n])=>`<button class="${mode===k?'on':''}" data-m="${k}">${I(ic,14)}${n}</button>`).join('')}</div>
        <div class="new-body">${left}</div>
      </div>
      <aside class="new-r">
        <div class="sec-title">${I('spark',15)}智能识别${recognized?'<small>已根据原文预填，可修改</small>':''}</div>
        <div class="rec">${rec}</div>
      </aside>
      <footer class="new-f">
        <span class="dim">${recognized?'Bug 走快速通道：跳过 AI 预审与人工审核，直接进入开发':'需求工单会先经过 AI 预审与人工审核'}</span><span class="grow"></span>
        <a class="btn ghost" href="#/board">取消</a>
        <button class="btn" data-save="draft">保存为草稿</button>
        <button class="btn" data-save="ai_review">保存并提交预审</button>
        ${recognized?`<button class="btn primary" data-save="developing">${I('zap',14)}保存并开始修复</button>`:''}
      </footer></div>`;
    $$('[data-m]').forEach(b=>b.onclick=()=>{mode=b.dataset.m;render()});
    const fill=$('#fill');if(fill)fill.onclick=()=>{recognized=true;render();toast('已识别为 Bug，并拆出复现步骤')};
    $$('.pick button').forEach(b=>b.onclick=()=>{$$('button',b.parentNode).forEach(x=>x.classList.remove('on'));b.classList.add('on')});
    $$('[data-save]').forEach(b=>b.onclick=()=>{
      const st=b.dataset.save,key='TW-48';
      if(!T(key))TICKETS.unshift({key,title:recognized?'订单导出 Excel 在 WPS 中表头乱码':'未命名工单',type:recognized?'bug':'feature_mixed',status:st,prio:'high',projects:['shop-api'],source:'粘贴',updated:'刚刚',labels:['导出'],running:st==='ai_review',sub:st==='developing'?[['BUG','queued']]:undefined});
      toast(`已创建 ${key} · ${STAGE[st].name}`);location.hash='#/board'});
  };
  render();
}
