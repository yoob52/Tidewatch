// 生成应用内更新读的 latest.json（Tauri updater 的静态清单格式）。
// 用法：node scripts/latest-json.mjs <版本> <平台> <.sig 文件> <安装包下载地址>
import { readFileSync } from 'node:fs';

const [version, platform, sigFile, url] = process.argv.slice(2);
if (!version || !platform || !sigFile || !url) {
  console.error('用法：node scripts/latest-json.mjs <版本> <平台> <.sig 文件> <安装包下载地址>');
  process.exit(2);
}
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`版本号不是 x.y.z：${version}`);
  process.exit(2);
}
if (!url.startsWith('https://')) {
  console.error(`下载地址必须是 https：${url}`);
  process.exit(2);
}
const signature = readFileSync(sigFile, 'utf8').trim();
if (!signature) {
  console.error(`签名文件是空的：${sigFile}`);
  process.exit(2);
}
process.stdout.write(
  JSON.stringify(
    {
      version,
      notes: `观澜 ${version}，更新说明见发布页。`,
      pub_date: new Date().toISOString(),
      platforms: { [platform]: { signature, url } },
    },
    null,
    2,
  ) + '\n',
);
