# WinDivert 2.2.2-A

Windows x64 桌面版内嵌官方 DLL、签名驱动、完整许可证和 v2.2.2 对应源码，动态加载时选择上游 LGPL-3.0 授权。DLL 和驱动未修改。

- 项目：https://github.com/basil00/WinDivert
- 运行包：https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip
- 运行包 SHA-256：`63cb41763bb4b20f600b6de04e991a9c2be73279e317d4d82f237b150c5f3f15`
- 源码：https://github.com/basil00/WinDivert/archive/refs/tags/v2.2.2.zip
- 源码 ZIP SHA-256：`65ec79c9e6afa99f648a3f4d1f6db794640b40d0b65bd438770ea503ee14ecb7`
- `WinDivert.dll` SHA-256：`c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2`
- `WinDivert64.sys` SHA-256：`8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2`

使用 `OpenSesame.exe --export-windivert-source <目录>` 可从单个 EXE 提取对应源码及许可证，无需联网。`--windivert-dir <目录>` 允许用户显式加载兼容的替代 DLL 及其驱动。默认缓存的完整性检查不限制此替代方式。允许为调试 WinDivert 修改而进行逆向工程。

默认 SYN 不加载 WinDivert；自定义 TCP Flags 使用 send-only 句柄，不捕获或拦截其他网络流量。发布源码时需保留本目录及上游许可证。
