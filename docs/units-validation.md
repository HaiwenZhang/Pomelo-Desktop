# 显示单位与坐标验证

2026-10-02，当前仅 Windows。

配置文件补验：扩展连续文档更新测试，第二个文档保存 mil 后，重新读取保留 mil，第一个文档仍为毫米。新增真实磁盘旧配置/未知单位测试：删除 length_unit 后以新 ViewStore 实例读取默认毫米；写入未知单位后 load_matching 返回可在五语渲染的错误且原文件字节不变。16 项配置测试及应用 Clippy 通过。该证据是配置文件及身份匹配层验证，不包含真实应用退出/启动路径。

毫米与 mil 为每文档显示设置，1 mil = 0.0254 mm，源场景、拾取、几何和 GPU 实例始终采用毫米。旧配置缺少 length_unit 时默认毫米。比例尺按所选单位选取 1/2/5 刻度；选中走线总长和检查器坐标、起终点、线宽、钻孔尺寸及背钻显示直径按当前单位显示。检查器保留源数值的可往返字符串，显示时才换算和舍入，后台结果不捕获显示单位。指针读数按最新画布 bounds 和相机反投影，离开画布隐藏。

工作区 `test --workspace --all-targets --all-features --locked --offline` exit 0，32 个测试目标合计 355 passed、0 failed、7 ignored，见 [原始日志](gpu-validation/units-workspace-tests.log)。忽略项不计作通过。工作区 all-targets/all-features Clippy（`-D warnings`）通过。

随后扩展并显式执行 D3D11 硬件合成测试：切换 mil 再恢复毫米，整幅 PCB 像素与初始一致，走线/铜皮 uploaded_bytes 不变，cache_builds 均保持 1。专项 1 passed、0 failed、0 ignored，exit 0，见 [硬件日志](gpu-validation/units-hardware-tests.log)。该项使用离屏 D3D11 目标，不包含比例尺和文本 UI，也不是实际工具栏切换验收。

仍待：真实 BRD 窗口单位切换、选择与指针坐标核对、比例尺实测、五语/主题/DPI、最小窗口布局、跨启动恢复以及大板连续移动时的刷新性能。上述证据不表示 BRD MVP 或单位功能的全部验收完成。
