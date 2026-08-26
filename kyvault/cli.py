"""vault CLI — 命令行入口，纯本地，零网络依赖。"""

import argparse
import json
import os
import subprocess
import sys

from . import __version__
from .store import (
    init_master_key,
    looks_like_secret,
    get_secret,
    set_secret,
    delete_secret,
    list_secrets,
    list_platforms,
    parse_ref,
    set_account,
    get_account,
    list_accounts,
    delete_account,
    set_key,
    get_key,
    list_keys,
    delete_key,
    list_all_platforms,
    list_platform_detail,
    set_server,
    get_server,
    list_servers,
    delete_server,
    set_cli_token,
    get_cli_token,
    list_clis,
    delete_cli_token,
    annotate_secret,
)
from .validator import validate_key, list_providers, get_provider
from .alias import (
    set_alias,
    get_alias,
    resolve_ref,
    list_aliases,
    delete_alias,
)
from .wizard import wizard
from .import_env import import_env_file
from .update_check import check_for_update
from .doctor import run_doctor
from .updater import run_update


def cmd_get(args: argparse.Namespace) -> None:
    """读取密钥并打印明文（供人使用）。"""
    ref = resolve_ref(args.ref)
    try:
        plaintext = get_secret(ref)
        if plaintext is None:
            print(f"未找到：{ref}", file=sys.stderr)
            sys.exit(1)
        print(plaintext, end="")
    except ValueError as e:
        print(f"错误：{e}", file=sys.stderr)
        sys.exit(1)


def cmd_list(args: argparse.Namespace) -> None:
    """列出密钥，支持按平台过滤；--json 输出结构化数据供程序解析（如 CortexOS `cs kyvault`）。"""
    secrets = list_secrets()

    if args.platform:
        secrets = [s for s in secrets if s["platform"] == args.platform]

    if getattr(args, "json", False):
        print(json.dumps(secrets, ensure_ascii=False))
        return

    if not secrets:
        print("（空）")
        return
    if args.platform and not secrets:
        print(f"平台 {args.platform} 下没有密钥")
        return

    # 历史数据里可能存在把密钥本身当 name 的条目（新版 set 已拦，旧数据还在）。
    # 人读输出对这类 name 打码，避免终端截图/录屏/共享时把明文带出去。
    # --json 分支不打码 —— 那是给程序用的，cs kyvault 需要完整 ref 才能寻址。
    masked = 0
    for s in secrets:
        account = f" ({s['account']})" if s.get("account") else ""
        name = s["name"]
        if looks_like_secret(name):
            masked += 1
            name = f"{name[:7]}…{name[-4:]}" if len(name) > 16 else f"{name[:4]}…"
            name += "  ⚠️ name 疑似密钥明文"
        print(f"  secret://{s['platform']}/{name}{account}  [{s.get('kind', '?')}]")

    if masked:
        print(
            f"\n⚠️  {masked} 条的 name 疑似密钥本身（已打码显示）。name 字段不加密，"
            f"建议用 delete + set 换成可读别名；\n"
            f"   若这些密钥曾被打印过，请到对应平台吊销重签。完整 ref 用 --json 查看。",
            file=sys.stderr,
        )


def cmd_platforms(args: argparse.Namespace) -> None:
    """列出所有平台及其密钥数量。"""
    platforms = list_platforms()
    if not platforms:
        print("（空）")
        return
    print("平台  密钥数")
    print("-" * 30)
    for platform, secrets in sorted(platforms.items()):
        kinds = ", ".join(set(s["kind"] for s in secrets))
        print(f"  {platform:<15} {len(secrets)}  [{kinds}]")


def cmd_set(args: argparse.Namespace) -> None:
    """写入/更新密钥。value 传 "-" 时从 stdin 读取，避免明文出现在进程参数列表
    （`ps`/`ps aux` 能看到 argv，看不到 stdin），供其他程序调用本 CLI 时使用。"""
    try:
        platform, name = parse_ref(args.ref)
        # name 不加密，list 会原样打印。把密钥本身填成 name 等于加密存一份、
        # 明文再漏一份，加密就白做了 —— 默认拦住，除非显式 --force。
        if looks_like_secret(name) and not getattr(args, "force", False):
            print(
                f"✗ 拒绝保存：ref 里的 name「{name[:12]}…」看起来是密钥本身，不是别名。\n"
                f"  name 字段不加密，kyvault list 会把它原样打印出来。\n"
                f"  改成可读别名，例如： secret://{platform}/api-key、secret://{platform}/main-pat\n"
                f"  确认无误要强行保存： 加 --force",
                file=sys.stderr,
            )
            sys.exit(1)
        value = sys.stdin.readline().rstrip("\n") if args.value == "-" else args.value
        set_secret(args.ref, value, kind=args.kind, account=args.account)
        print(f"✓ secret://{platform}/{name} 已保存")
    except ValueError as e:
        print(f"错误：{e}", file=sys.stderr)
        sys.exit(1)


def cmd_annotate(args: argparse.Namespace) -> None:
    """只改备注/类型，不需要提供密钥明文。"""
    try:
        if annotate_secret(args.ref, account=args.account, kind=args.kind):
            print(f"✓ {args.ref} 元信息已更新（密文未改动）")
        else:
            print(f"未找到：{args.ref}", file=sys.stderr)
            sys.exit(1)
    except ValueError as e:
        print(f"错误：{e}", file=sys.stderr)
        sys.exit(1)


def cmd_delete(args: argparse.Namespace) -> None:
    """删除密钥。"""
    try:
        if delete_secret(args.ref):
            platform, name = parse_ref(args.ref)
            print(f"✓ secret://{platform}/{name} 已删除")
        else:
            print(f"未找到：{args.ref}", file=sys.stderr)
            sys.exit(1)
    except ValueError as e:
        print(f"错误：{e}", file=sys.stderr)
        sys.exit(1)


def cmd_run(args: argparse.Namespace) -> None:
    """非交互式：注入环境变量到子进程，AI 无法读取明文。"""
    env = os.environ.copy()

    for env_spec in args.env:
        if "=" not in env_spec:
            print(f"格式错误：{env_spec}，应为 NAME=secret://... 或 NAME=别名", file=sys.stderr)
            sys.exit(1)
        var_name, ref_or_alias = env_spec.split("=", 1)
        ref = resolve_ref(ref_or_alias)
        try:
            value = get_secret(ref)
            if value is None:
                print(f"未找到：{ref}", file=sys.stderr)
                sys.exit(1)
            env[var_name] = value
        except ValueError as e:
            print(f"错误：{e}", file=sys.stderr)
            sys.exit(1)

    command = [c for c in args.command if c != "--"]
    if not command:
        print("错误：未指定命令", file=sys.stderr)
        sys.exit(1)

    result = subprocess.run(command, env=env)
    sys.exit(result.returncode)


def cmd_alias(args: argparse.Namespace) -> None:
    """管理别名。"""
    if args.alias_action == "set":
        set_alias(args.name, args.ref)
        print(f"✓ {args.name} → {args.ref}")
    elif args.alias_action == "get":
        ref = get_alias(args.name)
        if ref:
            print(ref)
        else:
            print(f"未找到别名：{args.name}", file=sys.stderr)
            sys.exit(1)
    elif args.alias_action == "list":
        aliases = list_aliases()
        if not aliases:
            print("（空）")
            return
        for name, ref in aliases.items():
            print(f"  {name} → {ref}")
    elif args.alias_action == "delete":
        if delete_alias(args.name):
            print(f"✓ 已删除别名：{args.name}")
        else:
            print(f"未找到别名：{args.name}", file=sys.stderr)
            sys.exit(1)


def cmd_init(args: argparse.Namespace) -> None:
    """初始化 master key。"""
    init_master_key()
    print(f"✓ Master key 已生成：~/.keyring/master.key")
    print(f"\n开始使用：")
    print(f"  kyvault wizard              # 交互式向导")
    print(f"  kyvault import --file .env  # 从 .env 导入")
    print(f"  kyvault set secret://...    # 手动添加")


def cmd_wizard(args: argparse.Namespace) -> None:
    """交互式设置向导。"""
    wizard()


def cmd_import(args: argparse.Namespace) -> None:
    """从 .env 文件导入。"""
    if args.dry_run:
        print("预览模式（不会实际导入）：")
        print()
    imported = import_env_file(
        filepath=args.file,
        prefix=args.prefix,
        dry_run=args.dry_run,
        auto_alias=not args.no_alias,
    )
    if args.dry_run:
        print()
        print(f"共 {len(imported)} 个密钥将被导入。")
        print("去掉 --dry-run 执行实际导入。")
    else:
        print(f"\n✓ 已导入 {len(imported)} 个密钥")


# ── 账户命令 ──────────────────────────────────────────────

def cmd_set_account(args: argparse.Namespace) -> None:
    """保存账户密码。"""
    set_account(args.platform, args.username, args.password)
    print(f"✓ 账户 {args.username}@{args.platform} 已保存")


def cmd_get_account(args: argparse.Namespace) -> None:
    """读取账户密码。"""
    password = get_account(args.platform, args.username)
    if password is None:
        print(f"未找到：{args.username}@{args.platform}", file=sys.stderr)
        sys.exit(1)
    print(password, end="")


def cmd_list_account(args: argparse.Namespace) -> None:
    """列出平台下所有账户。"""
    accounts = list_accounts(args.platform)
    if not accounts:
        print(f"平台 {args.platform} 下没有账户")
        return
    print(f"  {args.platform} 账户：")
    for username in accounts:
        print(f"    {username}")


def cmd_delete_account(args: argparse.Namespace) -> None:
    """删除账户。"""
    if delete_account(args.platform, args.username):
        print(f"✓ 账户 {args.username}@{args.platform} 已删除")
    else:
        print(f"未找到：{args.username}@{args.platform}", file=sys.stderr)
        sys.exit(1)


# ── 密钥命令 ──────────────────────────────────────────────

def cmd_set_key(args: argparse.Namespace) -> None:
    """保存平台密钥。"""
    set_key(args.platform, args.key_name, args.value)
    print(f"✓ 密钥 {args.key_name}@{args.platform} 已保存")


def cmd_get_key(args: argparse.Namespace) -> None:
    """读取平台密钥。"""
    value = get_key(args.platform, args.key_name)
    if value is None:
        print(f"未找到：{args.key_name}@{args.platform}", file=sys.stderr)
        sys.exit(1)
    print(value, end="")


def cmd_list_key(args: argparse.Namespace) -> None:
    """列出平台下所有密钥。"""
    keys = list_keys(args.platform)
    if not keys:
        print(f"平台 {args.platform} 下没有密钥")
        return
    print(f"  {args.platform} 密钥：")
    for key_name in keys:
        print(f"    {key_name}")


def cmd_delete_key(args: argparse.Namespace) -> None:
    """删除密钥。"""
    if delete_key(args.platform, args.key_name):
        print(f"✓ 密钥 {args.key_name}@{args.platform} 已删除")
    else:
        print(f"未找到：{args.key_name}@{args.platform}", file=sys.stderr)
        sys.exit(1)


# ── 平台查询命令 ──────────────────────────────────────────

def cmd_platform_list(args: argparse.Namespace) -> None:
    """列出所有平台及其账户/密钥摘要。"""
    platforms = list_all_platforms()
    if not platforms:
        print("（空）")
        return
    print("平台          账户数  密钥数")
    print("-" * 40)
    for platform, data in sorted(platforms.items()):
        print(f"  {platform:<15} {len(data['accounts']):<7} {len(data['keys'])}")


def cmd_platform_detail(args: argparse.Namespace) -> None:
    """查看指定平台详情。"""
    detail = list_platform_detail(args.platform_name)
    if detail is None:
        print(f"未找到平台：{args.platform_name}", file=sys.stderr)
        sys.exit(1)
    print(f"  {args.platform_name}：")
    if detail["accounts"]:
        print(f"    账户：")
        for username in detail["accounts"]:
            print(f"      {username}")
    if detail["keys"]:
        print(f"    密钥：")
        for key_name in detail["keys"]:
            print(f"      {key_name}")
    if not detail["accounts"] and not detail["keys"]:
        print(f"    （空）")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="kyvault",
        description="轻量级加密密钥管理 — AES-256-GCM，纯本地存储",
    )
    parser.add_argument("-V", "--version", action="version", version=f"kyvault {__version__}")
    sub = parser.add_subparsers(dest="command", help="子命令")

    # ── 账户子命令 ──
    p_account = sub.add_parser("account", help="管理账户（用户名+密码）")
    p_account.add_argument("action", choices=["set", "get", "list", "delete"], help="操作")
    p_account.add_argument("platform", help="平台名称（如 github、aws）")
    p_account.add_argument("username", nargs="?", help="用户名")
    p_account.add_argument("password", nargs="?", help="密码（set 时必填）")
    p_account.set_defaults(func=cmd_account)

    # ── 密钥子命令 ──
    p_key = sub.add_parser("key", help="管理平台密钥（API Key、Token 等）")
    p_key.add_argument("action", choices=["set", "get", "list", "delete"], help="操作")
    p_key.add_argument("platform", help="平台名称")
    p_key.add_argument("key_name", nargs="?", help="密钥名称")
    p_key.add_argument("value", nargs="?", help="密钥值（set 时必填）")
    p_key.set_defaults(func=cmd_key)

    # ── 平台查询 ──
    p_platform = sub.add_parser("platform", help="查看平台列表和详情")
    p_platform.add_argument("platform_name", nargs="?", help="平台名称（不填则列出所有）")
    p_platform.set_defaults(func=cmd_platform)

    # ── API Key 验证 ──
    p_check = sub.add_parser("check", help="验证 API Key 是否有效")
    p_check.add_argument("provider", help="平台名称（如 openai、deepseek、zhipu）")
    p_check.add_argument("key_name", nargs="?", help="kyvault 中的密钥名（可选）")
    p_check.add_argument("--key", dest="api_key", help="直接提供 API Key（可选）")
    p_check.set_defaults(func=cmd_check)

    # ── 支持的平台列表 ──
    p_providers = sub.add_parser("providers", help="列出支持的 LLM 平台")
    p_providers.set_defaults(func=cmd_providers)

    # ── 旧接口兼容 ──
    # get — 支持别名
    p_get = sub.add_parser("get", help="读取密钥（打印明文）")
    p_get.add_argument("ref", help="secret://platform/name 或别名")
    p_get.set_defaults(func=cmd_get)

    # list
    p_list = sub.add_parser("list", help="列出密钥（支持按平台过滤）")
    p_list.add_argument("--platform", "-p", help="只显示指定平台的密钥")
    p_list.add_argument("--json", action="store_true", help="输出 JSON（供程序解析）")
    p_list.set_defaults(func=cmd_list)

    # platforms
    p_platforms = sub.add_parser("platforms", help="列出所有平台及密钥数量")
    p_platforms.set_defaults(func=cmd_platforms)

    # set
    p_set = sub.add_parser("set", help="写入/更新密钥")
    p_set.add_argument("ref", help="secret://platform/name")
    p_set.add_argument("value", help="密钥值；传 - 表示从 stdin 读取（避免出现在进程参数里）")
    p_set.add_argument("--kind", default="API Key", help="密钥类型")
    p_set.add_argument("--account", default="", help="关联账号")
    p_set.add_argument("--force", action="store_true",
                       help="name 看起来像密钥本身时仍强行保存（默认拒绝）")
    p_set.set_defaults(func=cmd_set)

    # annotate —— 只改元信息，不用交出明文
    p_ann = sub.add_parser("annotate", help="只改备注/类型，不需要提供密钥明文")
    p_ann.add_argument("ref", help="secret://platform/name")
    p_ann.add_argument("--account", default=None, help="新的关联账号/备注")
    p_ann.add_argument("--kind", default=None, help="新的密钥类型")
    p_ann.set_defaults(func=cmd_annotate)

    # delete
    p_del = sub.add_parser("delete", help="删除密钥")
    p_del.add_argument("ref", help="secret://platform/name")
    p_del.set_defaults(func=cmd_delete)

    # run — 支持别名
    p_run = sub.add_parser("run", help="注入环境变量到子进程（不打印明文）")
    p_run.add_argument("--env", action="append", default=[], metavar="NAME=别名或secret://...",
                        help="注入环境变量，可多次使用")
    p_run.add_argument("command", nargs=argparse.REMAINDER, help="要执行的命令")
    p_run.set_defaults(func=cmd_run)

    # alias — 管理别名
    p_alias = sub.add_parser("alias", help="管理别名")
    p_alias_sub = p_alias.add_subparsers(dest="alias_action", help="别名操作")

    p_alias_set = p_alias_sub.add_parser("set", help="设置别名")
    p_alias_set.add_argument("name", help="别名名称（如 github_token）")
    p_alias_set.add_argument("ref", help="secret:// 引用")

    p_alias_get = p_alias_sub.add_parser("get", help="查看别名")
    p_alias_get.add_argument("name", help="别名名称")

    p_alias_list = p_alias_sub.add_parser("list", help="列出所有别名")

    p_alias_del = p_alias_sub.add_parser("delete", help="删除别名")
    p_alias_del.add_argument("name", help="别名名称")

    p_alias.set_defaults(func=cmd_alias)

    # init
    p_init = sub.add_parser("init", help="初始化 master key")
    p_init.set_defaults(func=cmd_init)

    # connect
    p_connect = sub.add_parser("connect", help="连接本地 AI 编码助手并写入规则/技能包")
    p_connect.set_defaults(func=cmd_connect)

    # server
    p_server = sub.add_parser("server", help="管理服务器资产与密码台账")
    p_server.add_argument("action", choices=["set", "get", "list", "delete"], help="操作")
    p_server.add_argument("hostname", nargs="?", help="服务器主机名（如 my-web-server）")
    p_server.add_argument("ip", nargs="?", help="服务器公网 IP（set 时必填）")
    p_server.add_argument("root_password", nargs="?", help="服务器 root 密码（set 时必填）")
    p_server.add_argument("--cost", help="月度租用成本（可选）")
    p_server.add_argument("--provider", help="云服务商（可选）")
    p_server.add_argument("--field", help="get 时指定读取的特定字段（可选）")
    p_server.set_defaults(func=cmd_server)

    # cli
    p_cli = sub.add_parser("cli", help="管理 CLI 多 Profile Token 凭证")
    p_cli.add_argument("action", choices=["set", "get", "list", "delete"], help="操作")
    p_cli.add_argument("cli_name", nargs="?", help="CLI 名称（如 studio-cli）")
    p_cli.add_argument("profile", nargs="?", help="Profile 账号别名（如 webkubor）")
    p_cli.add_argument("token", nargs="?", help="登录凭证 Token（set 时必填）")
    p_cli.set_defaults(func=cmd_cli)

    # wizard
    p_wizard = sub.add_parser("wizard", help="交互式设置向导")
    p_wizard.set_defaults(func=cmd_wizard)

    # import
    p_import = sub.add_parser("import", help="从 .env 文件导入密钥")
    p_import.add_argument("--file", "-f", default=".env", help=".env 文件路径（默认 .env）")
    p_import.add_argument("--prefix", "-p", default="", help="只导入以此开头的环境变量")
    p_import.add_argument("--dry-run", "-n", action="store_true", help="预览模式，不实际导入")
    p_import.add_argument("--no-alias", action="store_true", help="不自动创建别名")
    p_import.set_defaults(func=cmd_import)

    # doctor
    p_doctor = sub.add_parser("doctor", help="运行工具自检与健康检查")
    p_doctor.set_defaults(func=cmd_doctor)

    # update
    p_update = sub.add_parser("update", help="在线检查新版本并升级工具")
    p_update.set_defaults(func=cmd_update)

    return parser


def cmd_account(args: argparse.Namespace) -> None:
    """路由账户子命令。"""
    if args.action == "set":
        if not args.username or not args.password:
            print("错误：set 需要 username 和 password", file=sys.stderr)
            sys.exit(1)
        cmd_set_account(args)
    elif args.action == "get":
        if not args.username:
            print("错误：get 需要 username", file=sys.stderr)
            sys.exit(1)
        cmd_get_account(args)
    elif args.action == "list":
        cmd_list_account(args)
    elif args.action == "delete":
        if not args.username:
            print("错误：delete 需要 username", file=sys.stderr)
            sys.exit(1)
        cmd_delete_account(args)


def cmd_key(args: argparse.Namespace) -> None:
    """路由密钥子命令。"""
    if args.action == "set":
        if not args.key_name or not args.value:
            print("错误：set 需要 key_name 和 value", file=sys.stderr)
            sys.exit(1)
        cmd_set_key(args)
    elif args.action == "get":
        if not args.key_name:
            print("错误：get 需要 key_name", file=sys.stderr)
            sys.exit(1)
        cmd_get_key(args)
    elif args.action == "list":
        cmd_list_key(args)
    elif args.action == "delete":
        if not args.key_name:
            print("错误：delete 需要 key_name", file=sys.stderr)
            sys.exit(1)
        cmd_delete_key(args)


def cmd_platform(args: argparse.Namespace) -> None:
    """路由平台查询子命令。"""
    if args.platform_name:
        cmd_platform_detail(args)
    else:
        cmd_platform_list(args)


# ── 验证命令 ──────────────────────────────────────────────

def cmd_check(args: argparse.Namespace) -> None:
    """验证 API Key 是否有效。"""
    # 如果提供了 key 直接验证
    if args.api_key:
        result = validate_key(args.provider, args.api_key)
        print(result["message"])
        if result["models"]:
            print(f"  可用模型：{', '.join(result['models'])}")
        if result.get("balance"):
            print(f"  {result['balance']}")
        sys.exit(0 if result["valid"] else 1)

    # 否则从 kyvault 中读取
    if args.key_name:
        value = get_key(args.provider, args.key_name)
        if value is None:
            print(f"未找到：{args.key_name}@{args.provider}", file=sys.stderr)
            sys.exit(1)
        result = validate_key(args.provider, value)
        print(result["message"])
        if result["models"]:
            print(f"  可用模型：{', '.join(result['models'])}")
        if result.get("balance"):
            print(f"  {result['balance']}")
        sys.exit(0 if result["valid"] else 1)

    print("错误：请提供 API Key 或 kyvault 中的密钥名", file=sys.stderr)
    sys.exit(1)


def cmd_providers(args: argparse.Namespace) -> None:
    """列出支持的 LLM 平台。"""
    providers = list_providers()
    print("支持的 LLM 平台：")
    print()
    for p in providers:
        print(f"  {p['logo']} {p['id']:<15} {p['name']}")
    print()
    print(f"共 {len(providers)} 个平台")


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    check_for_update(__version__)
    if not args.command:
        parser.print_help()
        sys.exit(1)
    args.func(args)


# ── 新增加密台账与连接路由实现 ────────────────────────────

def cmd_connect(args: argparse.Namespace) -> None:
    """连接本地 AI 编码助手。"""
    from .connect import connect_agents
    connect_agents()


def cmd_set_server(args: argparse.Namespace) -> None:
    """保存服务器台账。"""
    set_server(args.hostname, args.ip, args.root_password, args.cost or "", args.provider or "")
    print(f"✓ 服务器 {args.hostname} 台账已保存")


def cmd_get_server(args: argparse.Namespace) -> None:
    """读取服务器台账。"""
    data = get_server(args.hostname, args.field)
    if data is None:
        print(f"未找到：{args.hostname}", file=sys.stderr)
        sys.exit(1)
    if isinstance(data, dict):
        for k, v in data.items():
            print(f"  {k}: {v}")
    else:
        print(data, end="")


def cmd_list_servers(args: argparse.Namespace) -> None:
    """列出所有服务器。"""
    servers = list_servers()
    if not servers:
        print("没有记录任何服务器")
        return
    print("已记录的服务器主机名：")
    for hostname in servers:
        print(f"  {hostname}")


def cmd_delete_server(args: argparse.Namespace) -> None:
    """删除服务器台账。"""
    if delete_server(args.hostname):
        print(f"✓ 服务器 {args.hostname} 台账已删除")
    else:
        print(f"未找到：{args.hostname}", file=sys.stderr)
        sys.exit(1)


def cmd_server(args: argparse.Namespace) -> None:
    """路由服务器子命令。"""
    if args.action == "set":
        if not args.hostname or not args.ip or not args.root_password:
            print("错误：set 需要 hostname、ip 和 root_password", file=sys.stderr)
            sys.exit(1)
        cmd_set_server(args)
    elif args.action == "get":
        if not args.hostname:
            print("错误：get 需要 hostname", file=sys.stderr)
            sys.exit(1)
        cmd_get_server(args)
    elif args.action == "list":
        cmd_list_servers(args)
    elif args.action == "delete":
        if not args.hostname:
            print("错误：delete 需要 hostname", file=sys.stderr)
            sys.exit(1)
        cmd_delete_server(args)


def cmd_set_cli(args: argparse.Namespace) -> None:
    """保存 CLI Profile Token。"""
    set_cli_token(args.cli_name, args.profile, args.token)
    print(f"✓ CLI {args.cli_name} 的 Profile {args.profile} Token 已保存")


def cmd_get_cli(args: argparse.Namespace) -> None:
    """读取 CLI Profile Token。"""
    token = get_cli_token(args.cli_name, args.profile)
    if token is None:
        print(f"未找到：{args.profile}@{args.cli_name}", file=sys.stderr)
        sys.exit(1)
    print(token, end="")


def cmd_list_clis(args: argparse.Namespace) -> None:
    """列出 CLI。"""
    clis = list_clis(args.cli_name)
    if not clis:
        if args.cli_name:
            print(f"CLI {args.cli_name} 下没有配置 Profile")
        else:
            print("没有记录任何 CLI Token")
        return
    if isinstance(clis, list):
        print(f"  {args.cli_name} Profiles:")
        for profile in clis:
            print(f"    {profile}")
    else:
        print("已记录的 CLI 与 Profiles：")
        for cli_name, profiles in clis.items():
            print(f"  {cli_name} -> {', '.join(profiles)}")


def cmd_delete_cli(args: argparse.Namespace) -> None:
    """删除 CLI Token。"""
    if delete_cli_token(args.cli_name, args.profile):
        print(f"✓ CLI {args.cli_name} 的 Profile {args.profile} Token 已删除")
    else:
        print(f"未找到：{args.profile}@{args.cli_name}", file=sys.stderr)
        sys.exit(1)


def cmd_cli(args: argparse.Namespace) -> None:
    """路由 CLI 子命令。"""
    if args.action == "set":
        if not args.cli_name or not args.profile or not args.token:
            print("错误：set 需要 cli_name、profile 和 token", file=sys.stderr)
            sys.exit(1)
        cmd_set_cli(args)
    elif args.action == "get":
        if not args.cli_name or not args.profile:
            print("错误：get 需要 cli_name 和 profile", file=sys.stderr)
            sys.exit(1)
        cmd_get_cli(args)
    elif args.action == "list":
        cmd_list_clis(args)
    elif args.action == "delete":
        if not args.cli_name or not args.profile:
            print("错误：delete 需要 cli_name 和 profile", file=sys.stderr)
            sys.exit(1)
        cmd_delete_cli(args)


def cmd_doctor(args: argparse.Namespace) -> None:
    """运行自检。"""
    run_doctor()


def cmd_update(args: argparse.Namespace) -> None:
    """在线更新工具。"""
    run_update()

