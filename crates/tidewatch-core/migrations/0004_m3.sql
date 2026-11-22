-- M3：图片附件、导入

-- 图片附件：文件放工作区 attachments/ 下，这里只有元数据。path 相对工作区，搬家后不失效
ALTER TABLE attachment ADD COLUMN origin TEXT;        -- prd | user | other
ALTER TABLE attachment ADD COLUMN sha256 TEXT;
ALTER TABLE attachment ADD COLUMN width INTEGER;
ALTER TABLE attachment ADD COLUMN height INTEGER;
ALTER TABLE attachment ADD COLUMN agent_path TEXT;    -- 压缩副本路径，可空
CREATE INDEX idx_attachment_ticket ON attachment(ticket_id);
CREATE INDEX idx_attachment_sha ON attachment(sha256);

-- 每个 PRD 版本里的图片引用：保存时从 Markdown 解析出来的索引，不是第二份事实
CREATE TABLE prd_image (
  prd_version_id TEXT NOT NULL,
  attachment_id TEXT NOT NULL,
  authority TEXT NOT NULL,         -- authoritative | reference
  superseded_by TEXT,              -- attachment id，可空
  position INTEGER NOT NULL,       -- 在正文中的顺序
  PRIMARY KEY (prd_version_id, attachment_id)
);

-- 只能处理文字的 Provider（如 DeepSeek）：图片改发文字描述
ALTER TABLE provider ADD COLUMN text_only INTEGER NOT NULL DEFAULT 0;
