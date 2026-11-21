function pageStats(){
  const days=['11/08','11/09','11/10','11/11','11/12','11/13','11/14','11/15','11/16','11/17','11/18','11/19','11/20','11/21'];
  const cost=[1.2,0.4,2.1,3.4,2.8,1.9,0.3,0.2,2.6,3.9,4.4,3.1,2.9,2.58],tok=[1.1,0.3,2.4,3.0,2.9,2.2,0.4,0.2,2.8,4.1,5.2,3.6,3.3,2.9];
  const W=640,H=180,mx=5,x=i=>30+i*(W-50)/(days.length-1),y=v=>H-24-v/mx*(H-44);
  const area=`M${x(0)} ${y(tok[0])} `+tok.map((v,i)=>`L${x(i)} ${y(v)}`).join(' ')+` L${x(13)} ${H-24} L${x(0)} ${H-24}Z`;
  const line=cost.map((v,i)=>`${i?'L':'M'}${x(i)} ${y(v)}`).join(' ');
  const agents=[['claude','Claude Code',24,92,12,'$ 21.40'],['codex','Codex',17,82,24,'$ 12.90'],['deepseek','Claude Code · DeepSeek',21,86,19,'$ 5.82']];
  const cyc=[['草稿',0.3],['AI 预审',0.1],['人工审核',1.4],['已定稿',0.2],['开发中',0.9],['验收中',1.2]],ct=cyc.reduce((a,c)=>a+c[1],0);
  $('#view').innerHTML=`<div class="page">
    <div class="row" style="margin-bottom:14px"><div class="seg"><button data-toast="演示：切换到本周">本周</button><button class="on">本月</button></div><span class="dim">2026 年 11 月 1 日 – 21 日</span></div>
    <div class="kpis big">
      <div><span>完成工单</span><b>19</b><small class="up">较上月 +6</small></div>
      <div><span>平均周期 <i>创建 → 完成</i></span><b>4.1<em>天</em></b><small class="up">缩短 1.3 天</small></div>
      <div><span>预审发现问题</span><b>87</b><small>其中阻塞 14 个，均在开发前解决</small></div>
      <div><span>验收打回率</span><b>18<em>%</em></b><small class="down">较上月 +3%</small></div>
      <div><span>本月费用</span><b>$ 40.12</b><small>预算 $ 50.00 的 80%</small><div class="bar" style="margin-top:6px"><i style="width:80%;background:var(--amber)"></i></div></div>
    </div>
    <div class="sgrid">
      <div class="card"><div class="card-h"><h3>Token 与费用趋势</h3><span class="grow"></span><span class="lgd"><i style="background:var(--tide-wash);border:1px solid var(--tide)"></i>Token（百万）</span><span class="lgd"><i style="background:var(--cinnabar);height:2px"></i>费用（$）</span></div>
        <div class="card-b"><svg viewBox="0 0 ${W} ${H}" class="chart">${[0,1,2,3,4,5].map(v=>`<line x1="30" x2="${W-20}" y1="${y(v)}" y2="${y(v)}" class="grid"/><text x="22" y="${y(v)+3}" text-anchor="end">${v}</text>`).join('')}
          <path d="${area}" class="area"/><path d="${line}" class="line"/>${cost.map((v,i)=>`<circle cx="${x(i)}" cy="${y(v)}" r="2.5" class="pt"/>`).join('')}
          ${days.map((d,i)=>i%2===1?`<text x="${x(i)}" y="${H-8}" text-anchor="middle">${d}</text>`:'').join('')}</svg></div></div>
      <div class="card"><div class="card-h"><h3>时间花在哪个阶段</h3><span class="grow"></span><span class="dim">平均每张工单</span></div>
        <div class="card-b"><div class="cyc">${cyc.map(([n,d],i)=>`<i style="flex:${d};opacity:${[.35,.5,1,.45,.7,1][i]};background:var(--${i===2||i===5?'cinnabar':'tide'})" title="${n} ${d} 天"></i>`).join('')}</div>
          <ul class="cycl">${cyc.map(([n,d],i)=>`<li><span>${n}${i===2||i===5?'<em>等你</em>':''}</span><b class="mono">${d} 天</b><span class="dim mono">${Math.round(d/ct*100)}%</span></li>`).join('')}</ul>
          <p class="dim" style="margin-top:10px">63% 的时间在等你审核和验收，Agent 执行只占 22%。</p></div></div>
      <div class="card span2"><div class="card-h"><h3>各 Agent 表现</h3></div>
        <table class="tbl"><thead><tr><th>Agent</th><th class="num">运行次数</th><th style="width:26%">成功率</th><th style="width:26%">返工率</th><th class="num">平均耗时</th><th class="num">费用</th></tr></thead>
        <tbody>${agents.map(([a,n,r,s,rw,c],i)=>`<tr><td>${agentTag(a)}</td><td class="num">${r}</td><td><div class="row"><div class="bar grow"><i style="width:${s}%;background:var(--moss)"></i></div><span class="mono" style="width:34px;text-align:right">${s}%</span></div></td><td><div class="row"><div class="bar grow"><i style="width:${rw*2}%;background:var(--amber)"></i></div><span class="mono" style="width:34px;text-align:right">${rw}%</span></div></td><td class="num">${['9:42','7:15','12:08'][i]}</td><td class="num">${c}</td></tr>`).join('')}</tbody></table></div>
    </div></div>`;
}
