# TOP 图层列表顺序修正

日期：2026-10-03。用户指出 PCB Layer 顺序反了。只读观察原实例可见 TOP 位于列表最后；本轮修正这项已复现的显示问题，UI 字体保持原状。

## 原因与实现

`BoardDisplay::ordered_layers` 返回绘制用的从底到顶顺序，应用此前直接按该顺序生成左侧列表，所以 BOTTOM 和其他层出现在 TOP 上方。共享核心新增 `layers_front_to_back`，应用列表按从顶到底顺序排列，默认 TOP → 内层 → BOTTOM → 其他绘图层。传给 D3D11 的绘制顺序保持从底到顶，菜单的“置于顶层／底层”仍使用同一持久化身份和顺序。

本轮未反转 HLSL 绘制命令或修改 `display_rank`。原先同类别物理图元已经按 TOP 高于下层排列，D3D11 和 CPU 拾取使用同一 rank；钻孔、选择/悬停覆盖层及跨图元类别规则仍沿用 Web 策略。这里的证据证明**图层列表反序修复及手动提升同步**，不支持声称所有跨类别覆盖或所有板图的渲染顺序均已验收。

## 验证

新增核心回归检查默认 TOP 首行、同类别 TOP 的绘制 rank、手动置顶/置底与列表首尾对应。最终工作区 439 项测试通过、17 默认忽略；Clippy、格式、debug/Release 构建通过。

最终 debug 私有简中深色窗口打开 USBC_FPC，实窗和 accessibility 顺序为 TOP、L2-F、BOTTOM。将 BOTTOM 置顶后，它移到列表首行，同时重叠区域变为 BOTTOM 的黄色；再将 TOP 置顶，TOP 回到首行，重叠区域恢复 TOP 的蓝色。两次动作未增加静态几何上传和缓存构建，呈现无 GPU 错误。选择 J1 后正常退出、重开，TOP 首行和手动排序恢复；更多键盘、DPI、语言和复杂类别组合未在本轮验证。

冻结遥测：`.cache/canvas-parity/top-layer-promoted-bottom.json`、`top-layer-promoted-top.json`；启动版本为 `top-layer-launch.json`，恢复后的 accessibility 为 `top-layer-reopened-accessibility.txt`。此窗口使用隔离配置，用户原实例保持运行。

最终 debug：`f0249a62e43ab28a4d76a53645d1a4cb58ef276a83b749b0baad22d8299b5e7f`；Release：`616f3092217d7b7ec385d4c96646e5291efd29895f851cdf0b32e3de4462f9d1`。实窗只验 debug 副本，Release 构建通过。`theme.rs` 字体策略 SHA-256 仍为 `2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`。源码、日志和二进制指纹包含在 `.cache/canvas-parity/locate-provenance.json`。
