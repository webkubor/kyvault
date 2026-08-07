"""LLM API Key 验证模块 — 支持主流大模型平台。"""

import json
import urllib.request
import urllib.error
from typing import Optional


PROVIDERS = {
    "openai": {
        "name": "OpenAI",
        "url": "https://api.openai.com/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "OPENAI_API_KEY",
        "logo": "🟢",
    },
    "deepseek": {
        "name": "DeepSeek",
        "url": "https://api.deepseek.com/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "DEEPSEEK_API_KEY",
        "logo": "🔵",
    },
    "zhipu": {
        "name": "智谱 AI",
        "url": "https://open.bigmodel.cn/api/paas/v4/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "ZHIPU_API_KEY",
        "logo": "🟣",
    },
    "moonshot": {
        "name": "Moonshot (Kimi)",
        "url": "https://api.moonshot.cn/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "MOONSHOT_API_KEY",
        "logo": "🌙",
    },
    "anthropic": {
        "name": "Anthropic (Claude)",
        "url": "https://api.anthropic.com/v1/messages",
        "header": "x-api-key",
        "prefix": "",
        "env_key": "ANTHROPIC_API_KEY",
        "logo": "🟠",
        "method": "POST",
        "body": json.dumps({
            "model": "claude-3-haiku-20240307",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
        }),
        "extra_headers": {"anthropic-version": "2023-06-01", "content-type": "application/json"},
    },
    "gemini": {
        "name": "Google Gemini",
        "url": "https://generativelanguage.googleapis.com/v1beta/models?key=",
        "header": "",
        "prefix": "",
        "env_key": "GEMINI_API_KEY",
        "logo": "💎",
        "key_in_url": True,
    },
    "qwen": {
        "name": "通义千问",
        "url": "https://dashscope.aliyuncs.com/compatible-mode/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "DASHSCOPE_API_KEY",
        "logo": "☁️",
    },
    "aliyun": {
        "name": "阿里云百炼",
        "url": "https://dashscope.aliyuncs.com/compatible-mode/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "DASHSCOPE_API_KEY",
        "logo": "☁️",
    },
    "minimax": {
        "name": "MiniMax",
        "url": "https://api.minimaxi.chat/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "MINIMAX_API_KEY",
        "logo": "🔷",
    },
    "doubao": {
        "name": "字节豆包",
        "url": "https://ark.cn-beijing.volces.com/api/v3/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "DOUBAO_API_KEY",
        "logo": "🫘",
    },
    "groq": {
        "name": "Groq",
        "url": "https://api.groq.com/openai/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "GROQ_API_KEY",
        "logo": "⚡",
    },
    "together": {
        "name": "Together AI",
        "url": "https://api.together.xyz/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "TOGETHER_API_KEY",
        "logo": "🤝",
    },
    "openrouter": {
        "name": "OpenRouter",
        "url": "https://openrouter.ai/api/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "OPENROUTER_API_KEY",
        "logo": "🔀",
    },
    "fireworks": {
        "name": "Fireworks AI",
        "url": "https://api.fireworks.ai/inference/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "FIREWORKS_API_KEY",
        "logo": "🔥",
    },
    "siliconflow": {
        "name": "SiliconFlow",
        "url": "https://api.siliconflow.cn/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "SILICONFLOW_API_KEY",
        "logo": "🧊",
    },
    "baichuan": {
        "name": "百川",
        "url": "https://api.baichuan-ai.com/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "BAICHUAN_API_KEY",
        "logo": "🌊",
    },
    "spark": {
        "name": "讯飞星火",
        "url": "https://spark-api-open.xf-yun.com/v1/models",
        "header": "Authorization",
        "prefix": "Bearer ",
        "env_key": "SPARK_API_KEY",
        "logo": "✨",
    },
    "github": {
        "name": "GitHub",
        "url": "https://api.github.com/user",
        "header": "Authorization",
        "prefix": "token ",
        "env_key": "GITHUB_TOKEN",
        "logo": "🐙",
    },
}


def get_provider(name: str) -> Optional[dict]:
    """获取 provider 配置。"""
    return PROVIDERS.get(name.lower())


def list_providers() -> list[dict]:
    """列出所有支持的 provider。"""
    return [{"id": k, **v} for k, v in PROVIDERS.items()]


def _check_balance(provider_name: str, api_key: str) -> Optional[str]:
    """查询余额（仅支持的平台）。"""
    balance_apis = {
        "deepseek": {
            "url": "https://api.deepseek.com/user/balance",
            "parse": lambda r: f"余额：¥{r['balance_infos'][0]['total_balance']}" if r.get("balance_infos") else "余额：未知",
        },
        "moonshot": {
            "url": "https://api.moonshot.cn/v1/users/me/balance",
            "parse": lambda r: f"余额：¥{r.get('data', {}).get('available_balance', 0):.4f}",
        },
        "zhipu": {
            "url": "https://open.bigmodel.cn/api/paas/v4/user/balance",
            "parse": lambda r: f"余额：¥{r.get('balance', 0) / 100:.4f}",
        },
        "qwen": {
            "url": "https://dashscope.aliyuncs.com/api/v1/services/billing/usage",
            "parse": lambda r: f"余额：{r.get('balance', 'N/A')}",
        },
        "aliyun": {
            "url": "https://dashscope.aliyuncs.com/api/v1/services/billing/usage",
            "parse": lambda r: f"余额：{r.get('balance', 'N/A')}",
        },
        "doubao": {
            "url": "https://ark.cn-beijing.volces.com/api/v3/account/balance",
            "parse": lambda r: f"余额：¥{r.get('data', {}).get('balance', 0) / 10000:.4f}",
        },
    }

    config = balance_apis.get(provider_name)
    if not config:
        return None

    try:
        req = urllib.request.Request(
            config["url"],
            headers={"Authorization": f"Bearer {api_key}"},
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            result = json.loads(resp.read().decode("utf-8"))
            return config["parse"](result)
    except Exception:
        return None


def validate_key(provider_name: str, api_key: str) -> dict:
    """验证 API Key 是否有效。

    Returns:
        {"valid": bool, "message": str, "provider": str, "models": list, "balance": str|None}
    """
    provider = get_provider(provider_name)
    if not provider:
        return {
            "valid": False,
            "message": f"不支持的平台：{provider_name}",
            "provider": provider_name,
            "models": [],
            "balance": None,
        }

    try:
        url = provider["url"]

        # Gemini 特殊处理：key 放在 URL 参数
        if provider.get("key_in_url"):
            url = url + api_key

        headers = {}
        if provider["header"]:
            headers[provider["header"]] = provider["prefix"] + api_key

        # 添加额外 headers
        if "extra_headers" in provider:
            headers.update(provider["extra_headers"])

        method = provider.get("method", "GET")
        body = provider.get("body")

        data = body.encode("utf-8") if body else None
        req = urllib.request.Request(url, data=data, headers=headers, method=method)

        with urllib.request.urlopen(req, timeout=10) as resp:
            result = json.loads(resp.read().decode("utf-8"))

            # 提取模型列表
            models = []
            if "data" in result:
                models = [m.get("id", "") for m in result["data"][:5]]

            # 查询余额
            balance = _check_balance(provider_name, api_key)

            return {
                "valid": True,
                "message": f"{provider['logo']} {provider['name']} 验证通过",
                "provider": provider_name,
                "models": models,
                "balance": balance,
            }

    except urllib.error.HTTPError as e:
        if e.code == 401:
            msg = "认证失败：API Key 无效或已过期"
        elif e.code == 403:
            msg = "权限不足：API Key 无访问权限"
        elif e.code == 429:
            msg = "请求过于频繁（但 Key 有效）"
        else:
            msg = f"HTTP {e.code}: {e.reason}"
        return {
            "valid": False,
            "message": f"{provider['logo']} {provider['name']} {msg}",
            "provider": provider_name,
            "models": [],
            "balance": None,
        }
    except urllib.error.URLError as e:
        return {
            "valid": False,
            "message": f"网络错误：{e.reason}",
            "provider": provider_name,
            "models": [],
            "balance": None,
        }
    except Exception as e:
        return {
            "valid": False,
            "message": f"验证失败：{str(e)}",
            "provider": provider_name,
            "models": [],
            "balance": None,
        }
