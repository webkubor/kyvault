//! 两个后端共用的元信息结构。
//!
//! file 后端拿不到 last4/length/updated_at（secrets.json 的 keys 里只有 {name: 密文}，
//! 压根没有这几列），所以它们是空值而不是编造 —— list 显示空比显示一个算出来的
//! 假值好：后者会让人以为本地库也有这些元信息。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SecretMeta {
    pub platform: String,
    pub name: String,
    pub kind: String,
    pub account: String,
    pub last4: String,
    pub length: u64,
    pub updated_at: String,
    /// 所属组织。空串表示还没登记 —— 存量数据都是这样，不当错误处理。
    #[serde(default)]
    pub org: String,
    /// 权限范围，逗号分隔。同上，空串是「未登记」不是「无权限」。
    #[serde(default)]
    pub scopes: String,
}

impl SecretMeta {
    pub fn r#ref(&self) -> String {
        format!("secret://{}/{}", self.platform, self.name)
    }
}

/// 解析 secret://platform/name。name 里可以再带斜杠（cli/<name>/<profile>、
/// server/<host>/<field> 都靠这个），所以只在第一个斜杠处切。
pub fn parse_ref(r: &str) -> anyhow::Result<(String, String)> {
    let rest = r
        .strip_prefix("secret://")
        .ok_or_else(|| anyhow::anyhow!("格式错误，应为 secret://platform/name：{r}"))?;
    match rest.split_once('/') {
        Some((p, n)) if !p.is_empty() && !n.is_empty() => Ok((p.to_string(), n.to_string())),
        _ => Err(anyhow::anyhow!("格式错误：{r}")),
    }
}

pub fn last4(v: &str) -> String {
    let chars: Vec<char> = v.chars().collect();
    if chars.len() <= 4 {
        v.to_string()
    } else {
        chars[chars.len() - 4..].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ref_ok() {
        assert_eq!(
            parse_ref("secret://github/token").unwrap(),
            ("github".into(), "token".into())
        );
        // name 保留后续斜杠 —— cli/gh/main 这种三段寻址依赖它
        assert_eq!(
            parse_ref("secret://cli/gh/main").unwrap(),
            ("cli".into(), "gh/main".into())
        );
    }

    #[test]
    fn parse_ref_rejects_bad() {
        for bad in [
            "github/token",
            "secret://github",
            "secret://",
            "secret:///x",
            "secret://x/",
        ] {
            assert!(parse_ref(bad).is_err(), "{bad} 应该被拒");
        }
    }

    #[test]
    fn last4_handles_short_and_utf8() {
        assert_eq!(last4("ab"), "ab");
        assert_eq!(last4("abcdef"), "cdef");
        // 按字符切而不是按字节 —— 按字节会把中文切成乱码
        assert_eq!(last4("密钥令牌值"), "钥令牌值");
    }
}
