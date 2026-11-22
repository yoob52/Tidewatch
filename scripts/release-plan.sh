#!/usr/bin/env bash
# 发布工作流的第一步：核对版本号，决定这次构建签不签名。
# 读环境变量 GITHUB_REF_TYPE、GITHUB_REF_NAME、TAURI_SIGNING_PRIVATE_KEY，
# 把 VERSION 和 SIGNED（true / false）写进 $GITHUB_ENV。
set -euo pipefail

fail() {
  echo "::error::$1"
  exit 1
}

version=$(node -p "require('./src-tauri/tauri.conf.json').version")
[ "$(node -p "require('./package.json').version")" = "$version" ] || fail "package.json 的版本不是 $version"
grep -q "^version = \"$version\"" Cargo.toml || fail "Cargo.toml 的版本不是 $version"
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
  [ "v$version" = "${GITHUB_REF_NAME:-}" ] || fail "标签 ${GITHUB_REF_NAME:-} 和 tauri.conf.json 里的版本 $version 不一致"
fi

signed=false
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  signed=true
  pubkey=$(node -p "require('./src-tauri/tauri.conf.json').plugins.updater.pubkey")
  [ -n "$pubkey" ] || echo "::warning::tauri.conf.json 的 plugins.updater.pubkey 是空的：安装包会签名，但这个版本装上后不能在应用内更新"
else
  echo "::notice::没有配置 Secret TAURI_SIGNING_PRIVATE_KEY：只发布未签名的安装包，不生成 latest.json，应用内更新不可用"
fi

{
  echo "VERSION=$version"
  echo "SIGNED=$signed"
} >> "$GITHUB_ENV"
