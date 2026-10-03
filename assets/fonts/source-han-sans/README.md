# PCB 画布 MSDF 字体

2026-10-03 从用户更新后的 Web 项目 `C:/Users/Zen/Desktop/gitrepo/pomelo/public/fonts/source-han-sans/` 同步。

- 来源：Source Han Sans SC VF 2.005，Web 生成 MSDF 时使用字重 600。
- 授权：OFL-1.1，原始许可见 `LICENSE.txt`。
- 原样保留 core、Unicode 分块的 JSON/PNG 和 manifest，共 92 页；不重新生成距离场、不进行 sRGB 转换。
- `provenance.json` 记录每个文件及对应 Web 字体、布局、shader 源码的 SHA-256。
- `pages.rs` 是由同步脚本生成的嵌入清单。运行时按板文字及网络名解码需要的页，GPU 使用 RGBA8 UNORM 和线性采样。
- 只用于 PCB 板文字和自动标签。桌面 UI 字体保持原有方案；不使用 Web CSS 或 WOFF2 替换界面字体。

更新资源：

```powershell
python scripts/sync-web-msdf.py C:/Users/Zen/Desktop/gitrepo/pomelo
```

同步后必须重新执行 `scripts/check-msdf-layout-parity.mts`、`scripts/check-canvas-picking-parity.mts` 和 Windows D3D11 MSDF 硬件测试。资源增长会增加可执行文件体积；当前 PNG 共约 55 MiB。缺失字符遵循 Web 的问号回退，诊断使用共享五语消息契约。
