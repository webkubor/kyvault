#!/usr/bin/env bash
#
# 从 kyvault 把飞书/Lark 机器人恢复安装到 lark-cli。
# 让密钥库成为机器人真源 —— 换机、重装、新同事上手，一条命令重新装齐。
#
#   restore-lark-bots.sh            # 装库里所有成对的机器人
#   restore-lark-bots.sh hym nanzhu # 只装指定的几个
#
# 原理：飞书机器人凭据是**一对**（app-id + app-secret），lark-cli 要两个
# 一起给才装得上。这脚本从 kyvault 取出这一对，app-secret 走 stdin 注入
# lark-cli，不落盘、不打印。已装过的跳过。
set -euo pipefail

# 库里所有 -app-id 结尾的机器人（去掉后缀就是机器人名）
mapfile -t ALL < <(kyvault list 2>/dev/null \
  | grep -oE 'secret://(feishu|lark)/[a-z0-9-]+-app-id' \
  | sed -E 's|secret://(feishu\|lark)/||; s|-app-id$||' | sort -u)

if [ ${#ALL[@]} -eq 0 ]; then
  echo "✗ 从 kyvault 读不到机器人。"
  echo "  请确认本地密钥库 kyvault list 有输出。"
  exit 1
fi

WANT=("$@"); [ ${#WANT[@]} -eq 0 ] && WANT=("${ALL[@]}")
installed=$(lark-cli profile list 2>/dev/null | grep -oE '"name":\s*"[^"]+"' | sed -E 's/.*"([^"]+)"/\1/')

ok=0 skip=0 miss=0
for bot in "${WANT[@]}"; do
  # 平台可能是 feishu 或 lark
  plat=feishu
  kyvault get "secret://feishu/${bot}-app-id" >/dev/null 2>&1 || plat=lark
  id=$(kyvault get "secret://${plat}/${bot}-app-id" 2>/dev/null | tail -1)
  sec=$(kyvault get "secret://${plat}/${bot}-app-secret" 2>/dev/null | tail -1)
  if [ ${#id} -lt 8 ] || [ ${#sec} -lt 8 ]; then
    echo "⊘ $bot：库里不成对（缺 app-id 或 app-secret），跳过"
    miss=$((miss+1)); continue
  fi
  if echo "$installed" | grep -qx "$bot"; then
    echo "· $bot：lark-cli 已装，跳过"
    skip=$((skip+1)); continue
  fi
  if printf %s "$sec" | lark-cli profile add --name "$bot" --app-id "$id" \
       --app-secret-stdin --brand "$plat" >/dev/null 2>&1; then
    echo "✅ $bot：已装入 lark-cli（$plat）"
    ok=$((ok+1))
  else
    echo "✗ $bot：安装失败"
  fi
done
echo
echo "装好 $ok · 已存在 $skip · 不成对 $miss"
