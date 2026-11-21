// 路由与全局事件
function setTheme(t){state.theme=t;localStorage.setItem('tw-theme',t);document.documentElement.dataset.theme=t}
const TITLES={board:'看板',tickets:'工单',new:'新建工单',runs:'运行',projects:'项目',providers:'Provider',stats:'统计',settings:'设置'};
function renderChrome(){ // 只刷新侧栏与顶栏里的数字
  const c=counts(),g=$('.gauge-head b');if(g)g.innerHTML=`${c.running}<i> / ${state.limit}</i>`;
  const rc=$('.runchip b');if(rc)rc.textContent=`${c.running}/${state.limit}`;
}
function route(){
  const[path,query]=(location.hash.slice(2)||'board').split('?'),[p,a,b]=path.split('/');
  const q=new URLSearchParams(query||'');
  if(q.get('theme'))setTheme(q.get('theme'));
  closeOv();
  if(p==='welcome'){pageWelcome(a);return}
  const active=p==='ticket'||p==='new'?(p==='new'?'board':'tickets'):p;
  const crumb=p==='ticket'?`<a href="#/tickets">工单</a>${I('right',11)}`:p==='new'?`<a href="#/board">看板</a>${I('right',11)}`:'';
  $('#app').innerHTML=shell(TITLES[active]?active:'board',p==='ticket'?a:(TITLES[p]||'看板'),crumb);
  ({board:pageBoard,tickets:pageTickets,new:pageNew,ticket:()=>pageTicket(a,b),runs:()=>pageRuns(a),projects:()=>pageProjects(a),providers:()=>pageProviders(a),stats:pageStats,settings:pageSettings}[p]||pageBoard)();
  // 截图 / 演示用：?show=cmd|notes|finalize
  const show=q.get('show');if(show==='cmd')openCmd();if(show==='notes')openNotes();if(show==='finalize')openFinalize();
  if(q.get('fill')&&$('#fill')){$('#fill').click();$('#toasts').innerHTML=''}
}
document.addEventListener('click',e=>{
  const x=e.target.closest('[data-x]');if(x){closeOv();if(!x.getAttribute('href'))return}
  if(e.target.id==='overlay'){closeOv();return}
  const t=e.target.closest('[data-toast]');if(t){toast(t.dataset.toast);return}
  const act=e.target.closest('[data-act]');
  if(act){({cmd:openCmd,notes:openNotes,theme:()=>{setTheme(state.theme==='dark'?'light':'dark');route()}})[act.dataset.act]();return}
  const sw=e.target.closest('[data-sw]');if(sw){sw.classList.toggle('on');return}
  const go=e.target.closest('[data-go]');if(go&&!e.target.closest('a,button'))location.hash=go.dataset.go;
});
document.addEventListener('keydown',e=>{
  if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();openCmd()}
  if((e.ctrlKey||e.metaKey)&&e.shiftKey&&e.key.toLowerCase()==='t'){e.preventDefault();location.hash='#/new?fill=1'}
  if(e.key==='Escape')closeOv();
});
window.addEventListener('hashchange',route);
setTheme(state.theme);route();
