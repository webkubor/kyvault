#!/bin/bash
# 把 .github/scripts/issue-triage.mjs 同步进 .github/workflows/issue-triage.yml 的 script: 块。
#
# ## 为什么需要这个
#
# actions/github-script **没有 script-path 这个 input**（v6/v7 的 action.yml 里
# 都只有 script:），所以脚本必须内联进 workflow。内联的代价是同一份逻辑在仓库里
# 有两处 —— 文件一份、workflow 一份。改了文件忘了同步，行为就会静默地变回旧版：
# 没有报错、没有 CI 失败，只有跑出来的结果不对。
#
# 所以：改逻辑只改 issue-triage.mjs，然后跑这个脚本。
# CI 里没有做这个校验（见 PR 讨论），所以这里靠人记得跑；跑完 git diff 确认
# workflow 里 script: 块变了即可。
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

SRC=".github/scripts/issue-triage.mjs"
WF=".github/workflows/issue-triage.yml"

[ -f "$SRC" ] || { echo "找不到 $SRC"; exit 1; }
[ -f "$WF" ] || { echo "找不到 $WF"; exit 1; }
python3 - "$SRC" "$WF" <<'PY'
import sys, re
src_path, wf_path = sys.argv[1], sys.argv[2]
script = open(src_path).read().rstrip('\n')
wf = open(wf_path).read()

# script: | 之后到下一个同缩进非空行之间的内容
pattern = re.compile(r'(          script: \|\n)((?:            .*\n|\n)*)', re.M)
if not pattern.search(wf):
    sys.exit('找不到 script: | 块 —— workflow 结构变了？手工同步一下。')

indented = '\n'.join(('            ' + l) if l.strip() else '' for l in script.split('\n'))
wf2 = pattern.sub(lambda m: m.group(1) + indented + '\n', wf, count=1)
if wf2 == wf:
    print('✓ 已是最新，无需改动')
else:
    open(wf_path, 'w').write(wf2)
    print(f'✓ 已把 {src_path} 同步进 {wf_path}')
    print('  记得 git diff 看一眼再提交')
PY
