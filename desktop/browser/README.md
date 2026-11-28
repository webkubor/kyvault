# Ky 闪记浏览器扩展

在普通 HTTP 或 HTTPS 网页选中文字，右键点「收进 Ky 闪记」。如果经常从某个网站摘录，点工具栏的 Ky 图标，选择「启用复制自动收藏」，并允许访问该网站。此后，在该网站顶层页面复制选区就会收藏。再次打开弹窗即可关闭，已打开的页面也会生效。

## 在 macOS 安装

1. 将支持闪记本机消息协议的 Ky.app 安装到 /Applications。
2. 在本目录运行 `./install-host.sh`，为当前用户的 Chrome、Edge、Brave 安装本机连接配置，无需 sudo。
3. 打开对应的扩展管理页（chrome://extensions、edge://extensions 或 brave://extensions），启用开发者模式，点「加载已解压的扩展程序」，选择本目录中的 **extension** 文件夹。保持该文件夹在原处。
4. 核对扩展 ID 为 `fkgfhoglncnijiadgohpgnjllphjcibg`。在普通网页右键收藏一句话，打开 Ky 闪记核对文本和来源地址。工具栏出现感叹号表示保存失败，请检查应用版本和本机连接配置。

## 采集范围

初始没有启用任何网站。开关按 URL.host 保存，区分子域名和非默认端口，HTTP 与 HTTPS 共用同一 host 的设置。浏览器按主机名授予可选权限，扩展在注入时和发送前再核对端口。只监听顶层页面的真实复制事件，读取当时的选区；跳过输入框、密码框、可编辑区和隐身窗口。扩展本地仅保存网站开关，不读取系统剪贴板。右键收藏使用浏览器提供的选区，框架内选区使用框架网址。

## Ky 桌面端接口

扩展用 sendNativeMessage 调用 com.kyvault.flash，发送 `{"text":"原文","url":"https://example.org/path"}`。网址保留协议、主机、端口和路径，删除用户名、密码、查询参数和片段。桌面端返回 `{"ok":true}` 或 `{"ok":false}`。

安装脚本将三个浏览器的配置指向 /Applications/Ky.app/Contents/MacOS/ky-desktop，限定允许来源为上述扩展 ID。浏览器启动程序时，第一个参数为 chrome-extension://fkgfhoglncnijiadgohpgnjllphjcibg/。桌面端须识别此参数并进入本机消息模式。标准输入、输出均使用本机字节序的 4 字节长度前缀和 UTF-8 JSON，日志写标准错误。[Chromium 本机消息协议](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)

## 验证

运行 `node --test tests/*.test.mjs`（Node.js 18 或更新版本）。手工验收时，在已启用的网站、其未启用的子域名、密码框和关闭开关后的原标签页各复制一次，只有第一项应新增闪记。用带查询参数和片段的页面再核对一次来源网址。
