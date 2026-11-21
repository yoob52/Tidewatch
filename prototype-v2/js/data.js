// 全部为演示数据，不连接任何后端或 Agent
const STAGES=[
 {id:'draft',name:'草稿',num:'一',hint:'随意编辑'},
 {id:'ai_review',name:'AI 预审',num:'二',hint:'Agent 读 PRD'},
 {id:'human_review',name:'人工审核',num:'三',hint:'等你处理问题',mine:true},
 {id:'finalized',name:'已定稿',num:'四',hint:'PRD 已锁定'},
 {id:'developing',name:'开发中',num:'五',hint:'Agent 执行'},
 {id:'acceptance',name:'验收中',num:'六',hint:'等你看 Diff',mine:true},
 {id:'done',name:'已完成',num:'七',hint:'已合并'},
];
const STAGE=Object.fromEntries(STAGES.map(s=>[s.id,s]));
// 合法流转（来自 docs/02-workflow/workflow.md）
const FLOW={
 draft:{ai_review:'提交预审',developing:'Bug 快速通道'},
 ai_review:{human_review:'预审完成'},
 human_review:{ai_review:'重新预审',finalized:'定稿'},
 finalized:{developing:'开始开发',human_review:'发起变更'},
 developing:{acceptance:'全部子任务完成',human_review:'发起变更'},
 acceptance:{developing:'打回',done:'验收通过并合并'},
 done:{},
};
const TYPES={
 feature_backend:{ch:'后',cls:'be',name:'后端需求'},
 feature_frontend:{ch:'前',cls:'fe',name:'前端需求'},
 feature_mixed:{ch:'混',cls:'mix',name:'前后端混合'},
 bug:{ch:'虫',cls:'bug',name:'Bug'},
 chore:{ch:'杂',cls:'chore',name:'杂项'},
};
const PRIO={urgent:'紧急',high:'高',medium:'中',low:'低'};
const AGENTS={
 claude:{name:'Claude Code',g:'cc',short:'CC'},
 codex:{name:'Codex',g:'cx',short:'CX'},
 deepseek:{name:'Claude Code · DeepSeek',g:'ds',short:'DS'},
};

const PROJECTS=[
 {id:'shop-api',name:'shop-api',kind:'backend',kindName:'后端',path:'D:\\code\\lanzhou\\shop-api',branch:'main',stack:'Java 21 · Spring Boot 3.3 · MyBatis-Plus · MySQL 8',agent:'claude',provider:'official-claude',
  cmds:{prepare:'mvn -q -DskipTests install',test:'mvn -q test',lint:'mvn -q spotless:check',build:'mvn -q -DskipTests package'},
  health:[['目录存在',1],['是 git 仓库',1],['工作区干净',1],['主分支 main',1]],open:5,
  context:`## 技术栈
Java 21、Spring Boot 3.3、MyBatis-Plus、MySQL 8、Redis 7

## 目录
- \`shop-api-web\` 控制器与 DTO
- \`shop-api-service\` 业务逻辑
- \`shop-api-dal\` Mapper 与实体
- \`docs/api\` 接口文档（每个工单一份）

## 规范
- 金额一律用 \`long\` 表示「分」，禁止 double
- 接口统一返回 \`Result<T>\`，错误码见 \`ErrorCode\`
- 新表必须带 \`created_at\` / \`updated_at\`

## 注意
- 不要改 \`legacy-order\` 模块`},
 {id:'shop-web',name:'shop-web',kind:'frontend',kindName:'前端',path:'D:\\code\\lanzhou\\shop-web',branch:'main',stack:'Vue 3.5 · Vite 6 · TypeScript · Pinia · Vant 4',agent:'codex',provider:'official-codex',
  cmds:{prepare:'pnpm install',test:'pnpm test',lint:'pnpm lint',build:'pnpm build'},
  health:[['目录存在',1],['是 git 仓库',1],['工作区干净',0],['主分支 main',1]],open:4,
  context:`## 技术栈
Vue 3.5、Vite 6、TypeScript、Pinia、Vant 4

## 规范
- 组件用 \`<script setup lang="ts">\`
- 接口封装放 \`src/api\`，类型放 \`src/types\`
- 金额展示统一用 \`formatFen()\``},
 {id:'ops-admin',name:'ops-admin',kind:'fullstack',kindName:'全栈',path:'D:\\code\\ops-admin',branch:'master',stack:'Next.js 15 · Prisma · PostgreSQL',agent:'deepseek',provider:'deepseek',
  cmds:{prepare:'pnpm install',test:'pnpm vitest run',lint:'pnpm lint',build:'pnpm build'},
  health:[['目录存在',1],['是 git 仓库',1],['工作区干净',1],['主分支 master',1]],open:2,context:'## 技术栈\nNext.js 15、Prisma、PostgreSQL'},
];
const PROJ=Object.fromEntries(PROJECTS.map(p=>[p.id,p]));

const PROVIDERS=[
 {id:'official-claude',name:'官方默认 · Claude',tpl:'官方默认（本机登录态）',protocol:'—',url:'沿用本机 claude 登录态',model:'claude-sonnet-4-5',fast:'claude-haiku-4-5',agents:['claude'],tokens:'18.4M',cost:'订阅内',ok:true,latency:null,def:true},
 {id:'official-codex',name:'官方默认 · Codex',tpl:'官方默认（本机登录态）',protocol:'—',url:'沿用本机 codex 登录态',model:'gpt-5-codex',fast:'—',agents:['codex'],tokens:'9.7M',cost:'订阅内',ok:true,latency:null},
 {id:'deepseek',name:'DeepSeek',tpl:'DeepSeek（Anthropic 兼容）',protocol:'anthropic',url:'https://api.deepseek.com/anthropic',model:'deepseek-chat',fast:'deepseek-chat',agents:['claude'],tokens:'31.2M',cost:'¥ 46.80',ok:true,latency:412,key:'sk-••••••••••••3f9a',price:'¥2 / ¥8 每百万'},
 {id:'gateway',name:'公司网关',tpl:'自定义 OpenAI 兼容',protocol:'openai_responses',url:'https://llm.corp.example/v1',model:'gpt-5-codex',fast:'gpt-5-mini',agents:['codex'],tokens:'4.1M',cost:'¥ 0（内部）',ok:true,latency:188,key:'gw-••••••••a71c'},
 {id:'openrouter',name:'OpenRouter',tpl:'自定义 OpenAI 兼容',protocol:'openai_chat',url:'https://openrouter.ai/api/v1',model:'qwen/qwen3-coder',fast:'—',agents:[],tokens:'0',cost:'¥ 0',ok:false,latency:null,key:'sk-or-••••••2b10',warn:'协议不匹配：Codex 当前版本不支持 openai_chat，需经 LiteLLM 桥接'},
];
const PROV=Object.fromEntries(PROVIDERS.map(p=>[p.id,p]));

const TICKETS=[
 {key:'TW-46',title:'首页改版：活动楼层支持后台配置',type:'feature_frontend',status:'draft',prio:'medium',projects:['shop-web'],source:'粘贴',updated:'10 分钟前',labels:['首页']},
 {key:'TW-45',title:'发票抬头管理',type:'feature_mixed',status:'draft',prio:'low',projects:['shop-api','shop-web'],source:'发票需求v2.docx',updated:'昨天',labels:['财务']},
 {key:'TW-47',title:'导出 Excel 中文表头乱码',type:'bug',status:'draft',prio:'high',projects:['shop-api'],source:'粘贴',updated:'3 分钟前',labels:['导出']},
 {key:'TW-44',title:'退款原因必填并按分类统计',type:'feature_mixed',status:'ai_review',prio:'medium',projects:['shop-api','shop-web'],source:'粘贴',updated:'刚刚',running:true,labels:['售后']},
 {key:'TW-42',title:'优惠券叠加规则与结算页展示',type:'feature_mixed',status:'human_review',prio:'urgent',projects:['shop-api','shop-web'],source:'优惠券叠加PRD.md',updated:'25 分钟前',issues:6,labels:['营销','结算'],due:'11-28'},
 {key:'TW-43',title:'商品评价支持追评',type:'feature_backend',status:'human_review',prio:'medium',projects:['shop-api'],source:'粘贴',updated:'2 小时前',issues:2,labels:['评价']},
 {key:'TW-40',title:'站内信已读状态与未读角标',type:'feature_mixed',status:'finalized',prio:'medium',projects:['shop-api','shop-web'],source:'粘贴',updated:'今天 09:12',sub:[['BE','queued'],['FE','blocked']],labels:['消息']},
 {key:'TW-39',title:'会员积分明细页',type:'feature_mixed',status:'developing',prio:'high',projects:['shop-api','shop-web'],source:'积分明细PRD.md',updated:'刚刚',sub:[['BE','review'],['FE','running']],running:true,labels:['会员'],due:'11-25'},
 {key:'TW-41',title:'订单导出支持按门店筛选',type:'feature_backend',status:'developing',prio:'medium',projects:['shop-api'],source:'粘贴',updated:'4 分钟前',sub:[['BE','running']],running:true,labels:['导出']},
 {key:'TW-38',title:'结算页地址选择在 iOS 键盘弹出时错位',type:'bug',status:'developing',prio:'high',projects:['shop-web'],source:'粘贴',updated:'12 分钟前',sub:[['BUG','queued']],labels:['结算']},
 {key:'TW-37',title:'商品搜索支持拼音与错别字',type:'feature_backend',status:'acceptance',prio:'high',projects:['shop-api'],source:'搜索优化.pdf',updated:'40 分钟前',sub:[['BE','review']],labels:['搜索']},
 {key:'TW-36',title:'购物车跨端同步',type:'feature_mixed',status:'acceptance',prio:'medium',projects:['shop-api','shop-web'],source:'粘贴',updated:'今天 10:40',sub:[['BE','accepted'],['FE','changes']],labels:['购物车']},
 {key:'TW-35',title:'升级 Vite 5 → 6',type:'chore',status:'done',prio:'low',projects:['shop-web'],source:'表单',updated:'周四',sub:[['FE','merged']],labels:[]},
 {key:'TW-34',title:'登录页验证码偶发刷新失败',type:'bug',status:'done',prio:'high',projects:['shop-web'],source:'粘贴',updated:'周三',sub:[['BUG','merged']],labels:['登录']},
 {key:'TW-33',title:'订单列表游标分页',type:'feature_backend',status:'done',prio:'medium',projects:['shop-api'],source:'粘贴',updated:'周二',sub:[['BE','merged']],labels:['订单']},
];
const T=k=>TICKETS.find(t=>t.key===k);

// ───── TW-42：人工审核中的工单 ─────
const PRD42_V1=`优惠券叠加规则与结算页展示

目前一笔订单只能用一张券。运营希望双十二前支持「店铺券 + 平台券」叠加，提升客单价。

- 同一订单最多可使用 1 张店铺券和 1 张平台券。
- 满减券与折扣券同时使用时，按合适的顺序计算优惠。
- 叠加后订单实付金额不得低于 0.01 元。
- 结算页新增「优惠明细」区域，分别列出每张券抵扣的金额。
- 用户可以手动取消勾选某张券，价格实时刷新。
- 系统默认帮用户选择最优组合。
- 部分退款时，优惠金额按比例分摊到每件商品。

订单表新增字段记录使用的优惠券。
可以同时使用两张券下单成功。`;
const PRD42_V2=`# 优惠券叠加规则与结算页展示

## 背景与目标
目前一笔订单只能用一张券。运营希望双十二前支持「店铺券 + 平台券」叠加，提升客单价。

## 功能需求
### F1 叠加规则
- 同一订单最多可使用 1 张店铺券和 1 张平台券。
- 满减券与折扣券同时使用时，按合适的顺序计算优惠。
- 叠加后订单实付金额不得低于 0.01 元。

### F2 结算页展示
- 结算页新增「优惠明细」区域，分别列出每张券抵扣的金额。
- 用户可以手动取消勾选某张券，价格实时刷新。
- 系统默认帮用户选择最优组合。

### F3 退款
- 部分退款时，优惠金额按比例分摊到每件商品。

## 数据定义
订单表新增字段记录使用的优惠券。

## 验收标准
- 可以同时使用两张券下单成功。

## 不做范围
- 不处理 I-7（非功能：结算页新增「优惠明细」区域）：本期不做，券数量目前不超过 5 张`;
// find / to：采纳时把 PRD 中的 find 行替换为 to；after：采纳后用于定位的短语
const ISSUES42=[
 {id:'I-1',sev:'blocker',cat:'歧义',loc:'按合适的顺序计算优惠',problem:'「合适的顺序」没有定义。先满减后折扣与先折扣后满减，结果不同（例：300 元订单，满 200 减 30 + 9 折：243.00 对 240.00）。',q:'满减与折扣的计算顺序是什么？是否固定「先折扣、后满减」？',fix:'满减券与折扣券同时使用时，固定**先计算折扣券、再计算满减券**；满减门槛按折扣后的金额判断。',state:'open',
  find:'- 满减券与折扣券同时使用时，按合适的顺序计算优惠。',to:'- 满减券与折扣券同时使用时，固定先计算折扣券、再计算满减券；满减门槛按折扣后的金额判断。',after:'固定先计算折扣券、再计算满减券'},
 {id:'I-2',sev:'blocker',cat:'数据',loc:'订单表新增字段记录使用的优惠券',problem:'未给出字段名、类型与结构。现有 `t_order.coupon_id BIGINT` 只能存一张券，直接复用会与叠加冲突。',q:'',fix:'新增关联表 `t_order_coupon(order_id, coupon_id, coupon_type, discount_fen, created_at)`；`t_order.coupon_id` 保留只读，仅兼容历史订单。',state:'open',
  find:'订单表新增字段记录使用的优惠券。',to:'新增关联表 `t_order_coupon(order_id, coupon_id, coupon_type, discount_fen, created_at)`，记录订单使用的每张券；`t_order.coupon_id` 保留只读，仅兼容历史订单。',after:'记录订单使用的每张券'},
 {id:'I-3',sev:'major',cat:'流程',loc:'系统默认帮用户选择最优组合',problem:'未定义「最优」：是实付最低，还是优先消耗快过期的券？两者冲突时如何取舍没有说明。',q:'最优组合以实付最低为准吗？金额相同时是否优先用先过期的券？',fix:'默认组合以**实付金额最低**为准；金额相同时优先使用**先过期**的券。',state:'open',
  find:'- 系统默认帮用户选择最优组合。',to:'- 系统默认选中实付金额最低的组合；金额相同时优先使用先过期的券。',after:'实付金额最低的组合'},
 {id:'I-4',sev:'major',cat:'接口',loc:'价格实时刷新',problem:'需要一个试算接口，PRD 未提及。现有 `POST /orders/preview` 不接受多张券。',q:'',fix:'新增 `POST /api/checkout/price-preview`，入参为商品行与券 ID 列表，返回逐券抵扣明细与实付金额；前端勾选变化时防抖 300ms 调用。',state:'adopted',
  find:'- 用户可以手动取消勾选某张券，价格实时刷新。',to:'- 用户可以手动取消勾选某张券，价格实时刷新：前端防抖 300ms 调用试算接口 `POST /api/checkout/price-preview`（入参为商品行与券 ID 列表），返回逐券抵扣明细与实付金额。',after:'价格实时刷新'},
 {id:'I-5',sev:'minor',cat:'可测试性',loc:'可以同时使用两张券下单成功',problem:'验收标准只有一条，覆盖不到计算顺序、下限金额与退款分摊。',q:'',fix:'用下方起草的 6 条可验证的验收标准替换这一条。',state:'open',
  find:'- 可以同时使用两张券下单成功。',to:null,after:'同时使用 1 张店铺券 + 1 张平台券'},
 {id:'I-6',sev:'minor',cat:'流程',loc:'优惠金额按比例分摊到每件商品',problem:'分摊出现除不尽时的尾差归属未定义。',q:'尾差计入金额最大的一件商品可以吗？',fix:'按商品金额占比分摊到「分」，尾差计入金额最大的一件商品。',state:'pending',
  find:'- 部分退款时，优惠金额按比例分摊到每件商品。',to:'- 部分退款时，优惠金额按商品金额占比分摊到「分」，尾差计入金额最大的一件商品。',after:'尾差计入金额最大的一件商品'},
 {id:'I-7',sev:'suggestion',cat:'非功能',loc:'结算页新增「优惠明细」区域',problem:'未说明券数量很多时的展示方式。',q:'',fix:'可用券超过 5 张时折叠，显示「查看全部 N 张」。',state:'ignored',reason:'本期不做，券数量目前不超过 5 张',
  find:'- 结算页新增「优惠明细」区域，分别列出每张券抵扣的金额。',to:'- 结算页新增「优惠明细」区域，分别列出每张券抵扣的金额；可用券超过 5 张时折叠，显示「查看全部 N 张」。',after:'可用券超过 5 张时折叠'},
];
const AC42=[
 ['同一订单可同时使用 1 张店铺券 + 1 张平台券并下单成功',true],
 ['满减 + 折扣叠加时先折扣后满减，300 元订单（9 折 + 满 200 减 30）实付 240.00',true],
 ['叠加后实付低于 0.01 元时，实付按 0.01 元计',true],
 ['取消勾选任一张券后 500ms 内价格刷新，明细同步变化',true],
 ['默认选中实付最低的组合；金额相同时选先过期的券',false],
 ['部分退款时优惠按金额占比分摊，各商品分摊之和等于总优惠',false],
];
ISSUES42.find(i=>i.id==='I-5').to=AC42.map(a=>'- '+a[0]).join('\n');
// 版本从新到旧；每个版本带不可变的正文快照
const VERSIONS42=[
 {v:3,who:'你',src:'采纳 AI 建议',note:'采纳 I-4：补充试算接口',time:'今天 14:02',cur:true,text:PRD42_V2.replace(ISSUES42[3].find,ISSUES42[3].to)},
 {v:2,who:'AI',src:'规整为标准结构',note:'按标准章节重排，未改动语义；记录忽略 I-7 的理由',time:'今天 13:41',text:PRD42_V2},
 {v:1,who:'你',src:'导入 优惠券叠加PRD.md',note:'原始导入',time:'今天 13:38',text:PRD42_V1},
];
const prd42=()=>VERSIONS42[0].text;
function prd42Save(text,src,note){VERSIONS42.forEach(v=>v.cur=false);VERSIONS42.unshift({v:VERSIONS42[0].v+1,who:'你',src,note,time:'刚刚',cur:true,text})}
// 阻塞问题只有「采纳 / 手动修改 / 忽略并写理由」才算处理完；「待确认」仍然阻塞定稿
const blockersLeft=()=>ISSUES42.filter(i=>i.sev==='blocker'&&(i.state==='open'||i.state==='pending')).length;
const SPLIT42=[
 {kind:'BE',cls:'be',proj:'shop-api',title:'叠加计算、试算接口与订单券关联表',agent:'claude',provider:'official-claude',branch:'tw/TW-42-be',first:'第一步产出 docs/api/TW-42.md',ac:5},
 {kind:'FE',cls:'fe',proj:'shop-web',title:'结算页优惠明细与券勾选',agent:'codex',provider:'official-codex',branch:'tw/TW-42-fe',dep:'依赖 BE 的 API 文档',ac:3},
];
const TIMELINE42=[
 ['今天 14:02','你','采纳了 I-4 的建议，生成 PRD v3','edit'],
 ['今天 13:44','AI 预审','完成，发现 7 个问题（2 阻塞 · 2 重要 · 2 次要 · 1 建议）','spark'],
 ['今天 13:41','AI','将 PRD 规整为标准结构，生成 v2','spark'],
 ['今天 13:39','你','提交预审','arrow'],
 ['今天 13:38','你','从 优惠券叠加PRD.md 导入，创建工单','upload'],
];

// ───── TW-39：开发 / 验收中的工单 ─────
const TASKS39=[
 {id:'BE',kind:'后端',cls:'be',proj:'shop-api',title:'积分流水查询接口与汇总',status:'review',statusName:'待验收',agent:'claude',provider:'official-claude',model:'claude-sonnet-4-5',perm:'workspace_write',branch:'tw/TW-39-be',wt:'D:\\tidewatch\\worktrees\\shop-api\\TW-39-be',runs:2,cost:'$1.84',dur:'11 分 20 秒',files:7,add:312,del:18,artifact:'docs/api/TW-39.md',tests:'42 通过',lastRun:'R-118'},
 {id:'FE',kind:'前端',cls:'fe',proj:'shop-web',title:'积分明细页与筛选',status:'running',statusName:'运行中',agent:'codex',provider:'official-codex',model:'gpt-5-codex',perm:'workspace_write',branch:'tw/TW-39-fe',wt:'D:\\tidewatch\\worktrees\\shop-web\\TW-39-fe',runs:1,cost:'$0.37',dur:'4 分 12 秒',files:4,add:126,del:3,dep:'BE · API 文档已就绪',lastRun:'R-121'},
];
// 全部子任务跑完（待验收或已通过）后，工单自动进入「验收中」
function syncTW39(){const t=T('TW-39');if(t.status==='developing'&&TASKS39.every(k=>['review','accepted'].includes(k.status)))t.status='acceptance'}
const AC39=[
 ['GET /api/points/records 支持按类型、时间范围筛选并游标分页',true],
 ['汇总接口返回可用、即将过期（30 天内）、累计获得三项',true],
 ['流水金额与汇总金额口径一致（单元测试覆盖）',true],
 ['未登录访问返回 401，不泄露他人数据',false],
 ['接口文档 docs/api/TW-39.md 已提交',true],
];
const DIFF_FILES=[
 {p:'docs/api/TW-39.md',s:'A',add:86,del:0},
 {p:'shop-api-dal/src/main/java/com/lanzhou/dal/PointsRecordMapper.java',s:'M',add:24,del:2},
 {p:'shop-api-dal/src/main/resources/mapper/PointsRecordMapper.xml',s:'M',add:31,del:4},
 {p:'shop-api-service/src/main/java/com/lanzhou/service/points/PointsQueryService.java',s:'A',add:74,del:0,sel:true,comments:1},
 {p:'shop-api-service/src/test/java/com/lanzhou/service/points/PointsQueryServiceTest.java',s:'A',add:61,del:0},
 {p:'shop-api-web/src/main/java/com/lanzhou/web/points/PointsController.java',s:'M',add:36,del:8},
 {p:'shop-api-web/src/main/java/com/lanzhou/web/points/LegacyPointsController.java',s:'D',add:0,del:4},
];
// 并排 Diff：[旧行号, 旧文本, 新行号, 新文本, 类型]
const DIFF_ROWS=[
 [12,'@Service',12,'@Service',''],
 [13,'public class PointsQueryService {',13,'public class PointsQueryService {',''],
 [14,'',14,'',''],
 [15,'    private final PointsRecordMapper mapper;',15,'    private final PointsRecordMapper mapper;',''],
 [null,'',16,'    private final Clock clock;','add'],
 [16,'',17,'',''],
 [17,'    public List<PointsRecord> list(long userId) {',null,'','del'],
 [18,'        return mapper.selectByUser(userId);',null,'','del'],
 [19,'    }',null,'','del'],
 [null,'',18,'    public CursorPage<PointsRecordVO> list(long userId, PointsQuery q) {','add'],
 [null,'',19,'        int size = Math.min(q.size(), 50);','add'],
 [null,'',20,'        List<PointsRecord> rows = mapper.selectPage(','add'],
 [null,'',21,'                userId, q.type(), q.from(), q.to(), q.cursor(), size + 1);','add'],
 [null,'',22,'        boolean hasMore = rows.size() > size;','add'],
 [null,'',23,'        return CursorPage.of(rows.stream().limit(size).map(PointsRecordVO::from).toList(), hasMore);','add'],
 [null,'',24,'    }','add'],
 [20,'',25,'',''],
 [null,'',26,'    public PointsSummaryVO summary(long userId) {','add'],
 [null,'',27,'        LocalDate today = LocalDate.now(clock);','add'],
 [null,'',28,'        long expiring = mapper.sumExpiringBetween(userId, today, today.plusDays(30));','add','c1'],
 [null,'',29,'        return new PointsSummaryVO(mapper.sumAvailable(userId), expiring, mapper.sumEarned(userId));','add'],
 [null,'',30,'    }','add'],
 [21,'}',31,'}',''],
];

// 前端子任务的验收数据（R-121 跑完后可验收）
const AC39_FE=[
 ['顶部展示可用、即将过期、累计获得三项汇总',true],
 ['可按类型与时间范围筛选，切换筛选后从第一页重新加载',true],
 ['触底自动加载下一页，没有更多数据时给出提示',false],
 ['空状态与加载失败（可重试）都有展示',false],
];
const DIFF_FILES_FE=[
 {p:'src/api/points.ts',s:'A',add:31,del:0},
 {p:'src/router/member.ts',s:'M',add:6,del:2,sel:true},
 {p:'src/types/points.ts',s:'A',add:24,del:0},
 {p:'src/views/points/PointsList.vue',s:'A',add:88,del:0},
 {p:'src/views/points/__tests__/PointsList.spec.ts',s:'A',add:52,del:0},
];
const DIFF_ROWS_FE=[
 [7,'',7,'',''],
 [8,'const routes: RouteRecordRaw[] = [',8,'const routes: RouteRecordRaw[] = [',''],
 [9,"  { path: '/member', component: MemberHome },",9,"  { path: '/member', component: MemberHome },",''],
 [10,"  { path: '/member/coupons', component: CouponList },",10,"  { path: '/member/coupons', component: CouponList },",''],
 [11,"  { path: '/member/points', redirect: '/member' },",null,'','del'],
 [12,'  // TODO: 积分明细页',null,'','del'],
 [null,'',11,'  {','add'],
 [null,'',12,"    path: '/member/points',",'add'],
 [null,'',13,"    component: () => import('@/views/points/PointsList.vue'),",'add'],
 [null,'',14,"    meta: { title: '积分明细', requiresAuth: true },",'add'],
 [null,'',15,'  },','add'],
 [13,']',16,']',''],
];
// 每个子任务一份验收材料；行内评论按子任务保存，切换页面后仍在
const ACCEPT39={
 BE:{files:DIFF_FILES,rows:DIFF_ROWS,dir:'shop-api-service/…/points/',name:'PointsQueryService.java',ac:AC39,runs:['R-118','R-112'],
  tests:[['mvn -q spotless:check','4s'],['mvn -q test','42 项 · 38s'],['mvn -q package','21s']],
  xr:{by:'Codex',items:[['amber','提醒','`summary()` 连续发起 3 次查询，可合并为一条 SQL。'],['','建议','`size` 上限 50 建议抽成配置项。']]},
  sum:'新增积分流水游标分页与汇总接口，先提交了 API 文档再实现。改动 7 个文件，补充 6 条单测。**需要人工确认**：旧的 `LegacyPointsController` 已删除，未发现其他调用方。',
  cost:'11 分 20 秒　$ 1.84　206.4k tokens',
  comments:[{line:28,text:'「30 天内」要含今天还是从明天算？和产品确认过是含今天，这里对的；但请加一条边界单测。'}]},
 FE:{files:DIFF_FILES_FE,rows:DIFF_ROWS_FE,dir:'src/router/',name:'member.ts',ac:AC39_FE,runs:['R-121'],
  tests:[['pnpm lint','6s'],['pnpm test','5 项 · 9s'],['pnpm build','31s']],
  xr:{by:'Claude Code',items:[['amber','提醒','「即将过期」角标用了 `--van-warning-color`，与设计稿的橙色不完全一致。'],['','建议','筛选条件建议同步到 URL query，刷新后不丢失。']]},
  sum:'新增积分明细页与两个接口封装，共改动 5 个文件；类型检查、lint、单测均通过。**需要人工确认**：即将过期的角标颜色暂用了 `--van-warning-color`。',
  cost:'4 分 36 秒　$ 0.41　58.2k tokens',comments:[]},
};

// ───── 运行 ─────
const RUNS=[
 {id:'R-121',ticket:'TW-39',task:'FE',title:'积分明细页与筛选',agent:'codex',provider:'official-codex',model:'gpt-5-codex',status:'running',el:252,tin:48200,tout:6100,cost:0.37,now:'编辑 src/views/points/PointsList.vue',session:'0199c3e2-7b1a-7c40',perm:'workspace_write',wt:'D:\\tidewatch\\worktrees\\shop-web\\TW-39-fe'},
 {id:'R-122',ticket:'TW-41',task:'BE',title:'订单导出支持按门店筛选',agent:'claude',provider:'official-claude',model:'claude-sonnet-4-5',status:'running',el:96,tin:21400,tout:1900,cost:0.21,now:'运行 mvn -q test',session:'8f2d41c0-3a7e-4b19',perm:'workspace_write',wt:'D:\\tidewatch\\worktrees\\shop-api\\TW-41-be'},
 {id:'R-123',ticket:'TW-44',task:'预审',title:'退款原因必填并按分类统计',agent:'deepseek',provider:'deepseek',model:'deepseek-chat',status:'running',el:38,tin:9800,tout:700,cost:0.02,now:'读取 RefundService.java',session:'c41e09aa-5d02-4f6b',perm:'read_only',wt:'D:\\code\\lanzhou\\shop-api（只读）'},
 {id:'R-124',ticket:'TW-38',task:'BUG',title:'结算页地址选择在 iOS 键盘弹出时错位',agent:'codex',provider:'official-codex',model:'gpt-5-codex',status:'queued',el:0,tin:0,tout:0,cost:0,now:'排队中 · 并发已满 3/3',session:'—',perm:'workspace_write',wt:'D:\\tidewatch\\worktrees\\shop-web\\TW-38-bug'},
 {id:'R-118',ticket:'TW-39',task:'BE',title:'积分流水查询接口与汇总',agent:'claude',provider:'official-claude',model:'claude-sonnet-4-5',status:'done',el:680,tin:184000,tout:22400,cost:1.52,now:'完成 · 测试 42 通过',session:'5b7a9e13-20cf-4d88',perm:'workspace_write',when:'今天 13:10'},
 {id:'R-117',ticket:'TW-36',task:'FE',title:'购物车跨端同步',agent:'codex',provider:'gateway',model:'gpt-5-codex',status:'failed',el:412,tin:96000,tout:11800,cost:0,now:'测试失败 · 自动修复 2/2 次后转人工',session:'0199c1b7-44e0-7a02',perm:'workspace_write',when:'今天 10:31'},
 {id:'R-116',ticket:'TW-37',task:'BE',title:'商品搜索支持拼音与错别字',agent:'deepseek',provider:'deepseek',model:'deepseek-chat',status:'done',el:934,tin:402000,tout:31000,cost:0.15,now:'完成 · 测试 58 通过',session:'a03c77d1-9e1b-4c2f',perm:'workspace_write',when:'今天 09:48'},
 {id:'R-115',ticket:'TW-36',task:'BE',title:'购物车跨端同步',agent:'claude',provider:'official-claude',model:'claude-sonnet-4-5',status:'stopped',el:187,tin:40100,tout:3900,cost:0.31,now:'已人工停止 · 可恢复',session:'e7120b5c-6aa3-41de',perm:'workspace_write',when:'昨天 17:22'},
];
const RUN=id=>RUNS.find(r=>r.id===id);
// 把一次运行的状态同步到它所属工单的子任务
function syncRun(r){
  const t=T(r.ticket);if(!t)return;
  const st={running:'running',queued:'queued',stopped:'stopped',done:'review'}[r.status],sub=t.sub&&t.sub.find(x=>x[0]===r.task);
  if(sub&&st)sub[1]=st;
  if(r.ticket==='TW-39'){const k=TASKS39.find(x=>x.id===r.task);if(k&&st){k.status=st;k.statusName=SUB[st][0]}syncTW39()}
  t.running=RUNS.some(x=>x.ticket===t.key&&x.status==='running');
}
// 有空闲并发时，让最早排队的运行开始
function promoteQueue(){
  [...RUNS].reverse().forEach(r=>{if(r.status==='queued'&&counts().running<state.limit){r.status='running';r.now='启动中 · 组装 Prompt';syncRun(r)}});
}
// R-121 的日志时间线；后半段在页面打开后逐条「流入」
const LOG=[
 {t:'00:00',k:'sys',text:'已创建运行 · Codex · gpt-5-codex · workspace_write · 分支 tw/TW-39-fe'},
 {t:'00:01',k:'prompt',title:'Prompt（frontend_dev 模板，2,140 字）',body:'你是本项目的前端开发者。请在当前目录（一个独立的 git worktree）中完成以下任务。\n\n## 项目背景\nVue 3.5、Vite 6、TypeScript、Pinia、Vant 4 …\n\n## 接口文档（来自后端子任务产物 docs/api/TW-39.md）\nGET /api/points/records …\nGET /api/points/summary …'},
 {t:'00:04',k:'msg',text:'我先看一下现有的会员中心路由和接口封装方式，再按 API 文档对接。'},
 {t:'00:06',k:'tool',tool:'读取',arg:'src/router/member.ts',out:'42 行'},
 {t:'00:07',k:'tool',tool:'搜索',arg:'formatFen\\(',out:'11 处匹配，5 个文件'},
 {t:'00:15',k:'tool',tool:'读取',arg:'src/api/coupon.ts',out:'68 行'},
 {t:'00:31',k:'msg',text:'现有接口都通过 `request<T>()` 封装并在 `src/types` 下声明类型，我照这个方式新增 `points.ts`。'},
 {t:'00:48',k:'edit',file:'src/types/points.ts',add:24,del:0},
 {t:'01:12',k:'edit',file:'src/api/points.ts',add:31,del:0},
 {t:'01:40',k:'tool',tool:'命令',arg:'pnpm vue-tsc --noEmit',term:'$ pnpm vue-tsc --noEmit\n\nsrc/api/points.ts:14:9 - error TS2322: Type \'string | undefined\' is not assignable to type \'string\'.\n\nFound 1 error in src/api/points.ts:14',bad:true},
 {t:'01:52',k:'msg',text:'`cursor` 首页请求时为空，类型应为可选。修正后重新检查。'},
 {t:'02:03',k:'edit',file:'src/api/points.ts',add:1,del:1},
 {t:'02:20',k:'tool',tool:'命令',arg:'pnpm vue-tsc --noEmit',term:'$ pnpm vue-tsc --noEmit\n\n✓ 无类型错误（3.1s）'},
 {t:'02:41',k:'msg',text:'接口层就绪。开始写页面：顶部汇总卡片（可用 / 即将过期 / 累计获得），下方流水列表，类型与时间筛选，触底加载。'},
 {t:'03:30',k:'edit',file:'src/views/points/PointsList.vue',add:70,del:0},
 {t:'04:02',k:'edit',file:'src/router/member.ts',add:6,del:2},
];
const LOG_LIVE=[
 {k:'tool',tool:'命令',arg:'pnpm lint',term:'$ pnpm lint\n\n✓ 0 errors, 0 warnings'},
 {k:'msg',text:'补一个空状态和加载失败的重试，再跑单元测试。'},
 {k:'edit',file:'src/views/points/PointsList.vue',add:22,del:4},
 {k:'tool',tool:'命令',arg:'pnpm test -- points',term:'$ pnpm test -- points\n\n ✓ src/views/points/__tests__/PointsList.spec.ts (5)\n   ✓ 渲染汇总卡片\n   ✓ 切换类型筛选会重置游标\n   ✓ 触底加载下一页\n   ✓ 空状态\n   ✓ 加载失败可重试\n\n Test Files  1 passed (1)\n      Tests  5 passed (5)'},
 {k:'msg',text:'已完成。新增积分明细页与两个接口封装，共改动 4 个文件；类型检查、lint、单测均通过。需要人工确认：即将过期的角标颜色暂用了 `--van-warning-color`。'},
 {k:'sys',text:'进程结束 · 自动提交 tw(TW-39): run #1 · 开始自动测试'},
];

const NOTES=[
 {icon:'alert',cls:'cin',title:'TW-36 前端运行失败',body:'测试失败，自动修复 2 次后仍未通过，需要你处理',time:'10:31',go:'#/ticket/TW-36/tasks'},
 {icon:'check',cls:'moss',title:'TW-39 后端子任务待验收',body:'7 个文件，+312 −18，测试 42 通过',time:'13:10',go:'#/ticket/TW-39/accept'},
 {icon:'spark',cls:'tide',title:'TW-42 预审完成',body:'发现 7 个问题，其中 2 个阻塞',time:'13:44',go:'#/ticket/TW-42/review'},
 {icon:'coin',cls:'amber',title:'本月费用已达预算 80%',body:'$ 40.12 / $ 50.00',time:'昨天',go:'#/stats'},
];
