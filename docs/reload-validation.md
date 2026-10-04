# 文档重新加载实现与验证

日期：2026-10-02，Windows 主线。

CPU 生命周期进一步验证：已导入文档重载专项扩展至走线/绘图共享批次、焊盘/钻孔共享批次、铜皮、搜索及拾取索引的 Weak 引用，全部随旧文档状态释放。新增过期导入结果释放专项：将正式准备函数生成的合成场景结果提交给旧 generation，结果被拒绝，其场景、批次和索引引用全部失效，新请求保持 Reading。当前 18 项文档测试通过，fmt 检查和应用 all-targets/all-features Clippy（`-D warnings`）通过。这仍是 CPU 所有权证据，不是 GPU 显存测量。

当前代码完整回归：`cargo +stable test --workspace --all-targets --all-features --locked --offline` exit 0，32 个测试目标合计 345 passed、0 failed、7 ignored，原始输出见 [工作区日志](gpu-validation/reload-workspace-tests.log)。忽略项未计作通过。

另行显式执行 `cargo +stable test -p pomelo-render hardware_ --all-features --locked --offline -- --ignored`，exit 0，2 passed、0 failed、0 ignored，原始输出见 [D3D11 硬件日志](gpu-validation/reload-hardware-tests.log)。这些测试覆盖图元合成、显示设置、缓存及主动 `reset()` 后重建；不经过 Workbench 重载和 GPUI 窗口关闭流程，不能据此宣称实窗重载资源释放已验收。

生命周期源码核查：GPUI 的注册表使用弱引用，绘制句柄和场景图元持有 renderer 强引用；业务 renderer 持有自身 GPU 资源。重载移除视口后，旧场景是否仍持有绘制句柄必须通过实际窗口流程确认。CPU 场景 Weak 引用失效与硬件 reset 测试是两项独立证据，不替代该项验收。

已导入终态补验：新增 imported_reload_releases_previous_scene_and_accepts_only_new_preparation，构造合成空 BoardScene，经正式 tracks/pads/copper/search/picking 准备函数生成 PreparedDocument。首次完成导入后重载，旧场景 Weak 引用失效；旧 generation 的 PreparedDocument 被拒绝，新 generation 接受并回到 Imported。`test -p pomelo imported_reload --locked --offline` 1 passed、0 failed，工作区 Clippy 通过。这是无窗口状态机/CPU 生命周期证据，不包含 BRD 解析或窗口 GPU 资源释放。

后续菜单禁用接线已补齐：文档菜单状态与语言偏好分开持有，Workbench 根据当前文档可用性更新 MenuItem::disabled 并刷新 AppMenuBar；工具栏复用同一布尔值。无文档、排队或读取时禁用，语言重建保留当前可用性。Clippy 与 Windows debug 构建通过。下文静态菜单说明为较早实现记录，当前已更新；真实菜单灰显、焦点和快速状态变化仍待验收。

重载入口为工具栏、文件菜单及 Ctrl+R，共用 ReloadDocument Action。菜单随语言重新构建，标签同步五语。工具栏和文件菜单均接入无文档或排队/读取时禁用；处理器仍在相同条件下拒绝请求。菜单视觉与焦点行为尚未实窗验收。

DocumentSession 为终态文档创建新 generation，取消旧令牌，保持文档 ID、源路径、编码及重定位来源，清空进度/诊断页/旧视口，进入正常后台导入队列。旧请求结果不能替换新一代状态。generation 溢出返回已有五语诊断，保留原状态。

重载前捕获视图并请求持久化；内存快照直接交接至后台导入，不等待磁盘保存。对实际内容 SHA-256、格式和编码匹配后恢复相机、显示设置、过滤与选择；内容或编码变化拒绝旧状态。失败/取消后重试仍保留待匹配快照。当前重载会移除旧板视口，失败时显示失败状态，不保留旧板画面。

已执行证据：16 项文档状态机测试、39 项应用测试、9 项 i18n 资源/源码检查通过，工作区 Clippy 通过。菜单接线后 Windows debug `build -p pomelo --locked --offline` exit 0。状态机重载专项使用取消态文档，不能替代已导入状态及 GPUI 任务流程的运行验收。

仍待验收：真实已导入文档重载、源内容改变、编码改变、失败重试、快速关闭与并发导入、设备缓存释放、新批次重建、内存快照优先及跨启动恢复。文件菜单禁用状态、键盘焦点与五语布局仍需完善/验证。本记录不宣称重新加载完整验收或 BRD MVP 完成。
