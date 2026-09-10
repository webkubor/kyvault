//! kyvault — 本地加密密钥管理。
//!
//! 拆成 lib 是为了让 tests/ 下的跨语言互操作测试能直接用 crypto：
//! 这次 Rust 重写唯一不能出错的地方就是「解得开 Python 版和 Go 端写的密钥」，
//! 那个断言必须是自动化测试，不能靠手工比对一次就算完。
pub mod crypto;
pub mod d1;
pub mod model;
pub mod store;
