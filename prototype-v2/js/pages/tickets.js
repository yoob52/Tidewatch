function pageTickets(){
  const f=state.listFilter;
  const tabs=[['all','全部'],['mine','等我处理'],['running','运行中'],['done','已完成']];
  const rows=TICKETS.filter(t=>f==='all'||(f==='mine'&&STAGE[t.status].mine)||(f==='running'&&t.running)||(f==='done'&&t.status==='done'));
  $('#view').innerHTML=`<div class="page">
    <div class="row" style="margin-bottom:14px">
      <div class="seg" id="lf">${tabs.map(([k,n])=>`<button class="${f===k?'on':''}" data-f="${k}">${n}</button>`).join('')}</div>
      <span class="grow"></span><span class="dim">${rows.length} 张工单</span>
      <button class="btn ghost" data-toast="演示：导出为 Markdown">${I('upload',14)}导出</button>
    </div>
    <div class="card" style="overflow:hidden"><table class="tbl">
      <thead><tr><th style="width:74px">编号</th><th>标题</th><th>状态</th><th>优先级</th><th>项目</th><th>子任务</th><th>来源</th><th style="text-align:right">更新</th></tr></thead>
      <tbody>${rows.map(t=>`<tr data-go="#/ticket/${t.key}">
        <td><span class="key">${t.key}</span></td>
        <td><span class="row">${stamp(t.type)}<b style="font-weight:500">${esc(t.title)}</b>${t.running?ripple:''}</span></td>
        <td><span class="tag ${STAGE[t.status].mine?'cin':t.status==='done'?'moss':t.status==='developing'||t.status==='ai_review'?'tide':''}">${STAGE[t.status].name}</span></td>
        <td>${prio(t.prio)}</td>
        <td><span class="row" style="gap:4px">${t.projects.map(p=>`<span class="tag line">${p}</span>`).join('')}</span></td>
        <td><span class="row" style="gap:4px">${subChips(t)||'<span class="dim">—</span>'}</span></td>
        <td class="dim">${esc(t.source)}</td><td class="dim" style="text-align:right;white-space:nowrap">${t.updated}</td></tr>`).join('')}</tbody>
    </table></div></div>`;
  $('#lf').onclick=e=>{const b=e.target.closest('button');if(b){state.listFilter=b.dataset.f;pageTickets()}};
}
