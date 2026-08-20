"""annotate（只改元信息，不碰密文）的行为测试。

盯的是一件事：**这条命令绝不能碰 ciphertext/nonce/sha256/last4/length**。
它存在的理由就是「改备注不用交出明文」，一旦它顺手重写了密文列，
不但失去意义，还可能用空值把一把好 key 覆盖掉——而这种坏法要到调用上游 401 才发现。

所以这里不 mock 到「调用成功就算过」，而是把 _query 收到的 SQL 抓下来逐项断言。
"""
import pytest

import kyvault.d1_backend as d1_backend


@pytest.fixture
def captured(monkeypatch):
    """替换 _query，记录所有 (sql, params)，并让存在性检查默认返回「有这条」。"""
    calls = []

    def fake_query(sql, params=None):
        calls.append((" ".join(sql.split()), params or []))
        if sql.strip().upper().startswith("SELECT"):
            return {"result": [{"results": [{"id": "secret://p/n"}]}]}
        return {"result": [{"results": []}]}

    monkeypatch.setattr(d1_backend, "_query", fake_query)
    return calls


def _update_call(calls):
    return next((c for c in calls if c[0].upper().startswith("UPDATE")), None)


def test_只更新传了的列_密文列一个都不出现(captured):
    assert d1_backend.annotate_secret("secret://p/n", account="新备注") is True
    sql, params = _update_call(captured)
    # 这是本文件的核心断言：密文相关列绝不能出现在 UPDATE 里
    for forbidden in ("ciphertext", "nonce", "sha256", "last4", "length"):
        assert forbidden not in sql.lower(), f"annotate 不该改 {forbidden}"
    assert "account=" in sql
    assert "kind=" not in sql, "没传 kind 就不该写 kind 列（否则会把原值冲成默认值）"
    assert "updated_at=" in sql, "改了元信息要留时间戳，否则看不出什么时候订正的"
    assert "新备注" in params


def test_两个字段都传时都更新(captured):
    d1_backend.annotate_secret("secret://p/n", account="备注", kind="Webhook")
    sql, params = _update_call(captured)
    assert "account=" in sql and "kind=" in sql
    assert "备注" in params and "Webhook" in params


def test_只传kind时不动account(captured):
    d1_backend.annotate_secret("secret://p/n", kind="DB Password")
    sql, _ = _update_call(captured)
    assert "kind=" in sql
    assert "account=" not in sql, "只改类型不该顺手把备注清空"


def test_占位符编号与参数顺序对齐(captured):
    """?1/?2/?3 的编号必须和 params 的下标一一对应，错位会把值写进错误的列。"""
    d1_backend.annotate_secret("secret://p/n", account="A", kind="K")
    sql, params = _update_call(captured)
    # 末位参数必须是 WHERE 用的 ref
    assert params[-1] == "secret://p/n"
    assert f"WHERE id = ?{len(params)}" in sql
    # 顺序：account, kind, updated_at, ref
    assert params[0] == "A" and params[1] == "K"


def test_ref不存在时返回False且不发UPDATE(monkeypatch):
    """打错一个字不能静默当成功——否则以为改好了，实际那条记录纹丝不动。"""
    calls = []

    def fake_query(sql, params=None):
        calls.append(" ".join(sql.split()))
        return {"result": [{"results": []}]}  # 查不到

    monkeypatch.setattr(d1_backend, "_query", fake_query)
    assert d1_backend.annotate_secret("secret://nope/nothing", account="x") is False
    assert not any(c.upper().startswith("UPDATE") for c in calls)


def test_什么都不传要报错而不是空跑(captured):
    with pytest.raises(ValueError, match="至少要给"):
        d1_backend.annotate_secret("secret://p/n")
    assert not captured, "参数校验该在任何查询之前拦住"


def test_ref格式非法要报错(captured):
    with pytest.raises(ValueError, match="格式错误"):
        d1_backend.annotate_secret("not-a-ref", account="x")


def test_空字符串备注是合法输入(captured):
    """account='' 与 account=None 语义不同：前者是「清空备注」，后者是「不改」。
    用 is None 判断而不是真值判断，否则永远清不掉一条写错的备注。"""
    assert d1_backend.annotate_secret("secret://p/n", account="") is True
    sql, params = _update_call(captured)
    assert "account=" in sql
    assert "" in params
