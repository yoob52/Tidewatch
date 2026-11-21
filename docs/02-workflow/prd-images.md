# PRD 中的图片

PRD 里的图片（原型图、流程图、截图）和文字一样是需求的一部分，需要随 PRD 一起保存、进版本、交给 Agent。但导入的 PRD 自带的示意图**经常是错的**【已确认】，所以需求方会贴自己的截图去替换或补充它们，并且要能明确指出哪张图说了算。

本文约定图片怎么存、怎么在 Markdown 里引用、怎么区分"权威图"和"仅参考图"，以及怎么交给 Claude Code / Codex。只是设计，尚未实现。

## 1. 两种来源，两种效力

| | 含义 | 默认效力 |
| --- | --- | --- |
| **原始图**（`origin = prd`） | 导入 PRD 时随文档带进来的图（docx / pdf 内嵌图、Markdown 相对路径图、飞书文档图） | **仅参考**（`reference`）【已确认】 |
| **用户图**（`origin = user`） | 需求方之后粘贴或上传的截图、手绘稿 | 【建议】默认**权威**（`authoritative`），可改为仅参考 |

哪张图是权威图由需求方标记【已确认】。效力的含义【建议】：

- **权威图**：界面长什么样、有哪些字段、流程怎么走，以它为准。与正文文字或参考图冲突时，按权威图实现，并把冲突当作问题报出来。
- **仅参考图**：只帮助理解背景和意图，**不能**作为界面布局、字段、文案的依据。

原始图导入后一律是参考图，导入、AI 规整、采纳预审建议都不会改变这一点。如果某张原始图恰好是对的，【建议】允许需求方手动把它标为权威：效力来自需求方的确认动作，而不是来自 PRD 本身。

## 2. 存储

沿用现有的附件机制（见 [安全与本地存储](../04-architecture/security-storage.md)）：图片文件放在工作区，数据库只存元数据和路径。

```
<工作区>\attachments\<ticket-key>\
  a_7f3k2m.png          # 文件名 = 附件 id + 原扩展名
  a_2c9d1x.png
```

- 导入时由解析器抽出图片（`parse(file) -> { markdown, attachments[] }`，见 [工单来源](ticket-sources.md)），原始文件名、来源文档内的顺序保留在元数据里。
- 文件**只增不改**：替换一张图是新增一个附件，旧文件保留。这样任何历史 PRD 版本都能原样渲染。
- 按 `sha256` 去重：同一张图重复粘贴不产生第二个文件。
- 粘贴的截图统一存为 PNG；过大的图（【建议】长边超过 2000px）另存一份压缩副本专供 Agent 使用，原图保留。
- 删除工单时附件目录一并清理；备份 / 导出包含附件目录。

## 3. 在 Markdown 中引用

PRD 正文仍是唯一的需求载体。图片用标准 Markdown 图片语法引用附件 id，不写磁盘路径（工作区搬家后引用不失效）：

```markdown
### F2 结算页优惠明细

![结算页优惠明细（需求方截图）](attachment:a_7f3k2m "authoritative")

![原 PRD 结算页示意](attachment:a_2c9d1x "reference; superseded-by=a_7f3k2m")
```

- `attachment:<id>`：附件引用，渲染和发给 Agent 时由系统解析为实际路径。
- 标题位（引号内）写效力：`authoritative` 或 `reference`；可附带 `superseded-by=<id>`，表示这张参考图已被哪张权威图取代。缺省视为 `reference`。
- alt 文字是图片说明，给人看，也给不支持图片的 Agent 看（见第 5 节）。

效力写在 Markdown 里而不是只存在数据库里，是有意的：

1. **进版本**：改"哪张图说了算"等于改需求，会产生新的 PRD 版本，能在版本对比里看到，定稿时一起锁定。
2. **可移植**：导出的 PRD 在任何 Markdown 工具里都能读，效力标记不丢。
3. 数据库里的 `prd_image` 只是保存时从 Markdown 解析出来的索引，用于查询和校验，不是第二份事实。

保存 PRD 时校验：引用的附件必须存在且属于本工单；`superseded-by` 必须指向同一版本里的一张权威图；作者为 `ai` 的版本不得改动任何图片的效力标记。

## 4. 需求方的操作

在工单的 PRD 页和审核页，每张图带一个效力徽标（权威 / 仅参考）。

| 操作 | 效果 |
| --- | --- |
| **粘贴 / 上传截图** | 新增一张用户图，插入光标处。若光标紧邻一张参考图，询问"替换它"还是"作为补充" |
| **替换**某张图 | 新图标为权威并放在原图位置；原图保留在其后，标 `reference; superseded-by=…`，界面上默认折叠显示为"原 PRD 图（已被取代）" |
| **补充** | 新图作为独立的图加入，不影响其他图 |
| **切换效力** | 任何一张图都可由需求方在权威 / 仅参考之间切换；把原始图标为权威时提示"此图来自原始 PRD，确认它是准确的？" |
| **移除** | 把引用从当前版本删掉；附件文件保留，旧版本不受影响 |

以上每个操作都产生一个新的 PRD 版本，来源标注为"图片变更"。定稿后要改图，和改文字一样走[变更流程](workflow.md#5-定稿后的变更已确认)。

被取代的原始图不从 PRD 里删掉：保留它可以让人和 AI 看出需求方到底纠正了什么。

## 5. 交给 Agent

图片在 **AI 预审**和**开发**两个阶段都发给 Claude Code 和 Codex【已确认】。

### 5.1 怎么给

- 运行前，把本次要用到的图片复制到 `<工作区>\tmp\<run-id>\images\`，文件名带上效力前缀，例如 `authoritative-a_7f3k2m.png`、`reference-a_2c9d1x.png`。放在 worktree 之外，避免被 Agent 提交进仓库。
- Prompt 中的 PRD 正文把 `attachment:<id>` 换成上述绝对路径，并在正文前加一段**图片清单**：

```markdown
## 图片清单
| 图片 | 效力 | 说明 | 位置 |
| --- | --- | --- | --- |
| D:\Tidewatch\tmp\R-121\images\authoritative-a_7f3k2m.png | 权威 | 结算页优惠明细（需求方截图） | F2 |
| D:\Tidewatch\tmp\R-121\images\reference-a_2c9d1x.png | 仅参考，已被 a_7f3k2m 取代 | 原 PRD 结算页示意 | F2 |

规则：
1. 权威图是需求方确认过的，界面、字段、文案以它为准。
2. 仅参考图未经需求方确认（多数来自原始 PRD，经常有错），只用于理解背景，不得据此决定界面或字段。
3. 权威图与正文文字冲突时，按权威图处理，并在输出中明确指出冲突点。
```

- 适配器的 `supports.images` 为真时才发图片文件（见 [Agent 适配层](../03-agent/agent-adapter.md)）。Claude Code 通过 Prompt 中的文件路径读取图片；Codex 通过命令行的图片参数附带。两者的具体参数和对图片大小、数量的限制，实现前按当时版本复核。
- 不支持图片的 Agent / Provider（如纯文本模型）：改发文字描述。描述由多模态模型在图片入库时生成并存入附件的 `description`，需求方可以改。描述同样带效力标注。

### 5.2 预审阶段

预审拿到**全部**图片（权威图、参考图、已被取代的参考图），在原有检查维度（见 [PRD 审核](prd-review.md)）之外增加：

| 检查点 | 默认级别 |
| --- | --- |
| 权威图与正文文字矛盾（字段、文案、流程不一致） | `major` |
| 权威图之间互相矛盾 | `major` |
| 某个界面类功能点只有参考图、没有权威图，且文字不足以确定界面 | `minor`，问题写为"待确认：此图是否可信，或请补一张截图" |
| 正文引用了图但图缺失 / 无法读取 | `blocker` |

预审**不会**因为参考图与正文不一致而报问题，除非这个不一致提示正文本身可能有误（此时作为 `suggestion`）。

### 5.3 开发阶段

- 拆分子任务时，每张图跟随它所在的功能点分到对应子任务；纯界面图默认只给 FE 子任务，BE 子任务只拿到与数据、接口有关的图。可在确认拆分时调整。
- 【建议】已被取代的参考图在开发阶段默认**不发**，只发权威图和未被取代的参考图，减少误导；可在设置里改为全部发送。
- 返工（resume 原会话）时只发相对上一轮新增或变更的图。
- 实际发送的图片清单随 Prompt 一起记录在 Run 中，便于复盘"Agent 当时看到的是哪张图"。

## 6. 数据模型的变化

在 [数据模型](../04-architecture/data-model.md) 的基础上：

```sql
-- attachment 增加的列
ALTER TABLE attachment ADD COLUMN origin TEXT;        -- prd | user | other（接口样例等非图片附件）
ALTER TABLE attachment ADD COLUMN sha256 TEXT;
ALTER TABLE attachment ADD COLUMN width INTEGER;
ALTER TABLE attachment ADD COLUMN height INTEGER;
ALTER TABLE attachment ADD COLUMN agent_path TEXT;    -- 压缩副本路径，可空

-- 每个 PRD 版本里的图片引用（保存时从 Markdown 解析出来的索引）
CREATE TABLE prd_image (
  prd_version_id TEXT, attachment_id TEXT,
  authority TEXT,                  -- authoritative | reference
  superseded_by TEXT,              -- attachment id，可空
  position INTEGER,                -- 在正文中的顺序
  PRIMARY KEY (prd_version_id, attachment_id)
);
```

## 7. 待确认

见 [待决问题](../06-roadmap/open-questions.md)：

- Q13：用户图默认权威是否合适，还是每次粘贴都要明确选择。
- Q14：是否允许把原始图直接标为权威。
- Q15：开发阶段是否默认不发已被取代的参考图。
