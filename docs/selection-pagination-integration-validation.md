# Windows 选择与成员分页集成回归

日期：2026-10-02。范围：当前工作树的成员分页、行定位、拾取类别过滤及状态提示集成。

## 验证结果

- 工作区全部特性测试通过，退出码 0；默认忽略的外部案例及硬件测试不计作本命令通过范围。
- 两项 D3D11 硬件像素测试显式执行并通过，退出码 0，覆盖走线圆弧/端帽、焊盘、铜皮重叠孔洞、底层走线、裁剪及缓存。
- Windows debug 应用实际构建通过，包含当前成员翻页、滚动页面身份和四类拾取开关。
- `target/debug/pomelo.exe` SHA256：`FF5F3C5B0565C278AEDB628866D82A9DB99108BA82F2C9DBF865225A48F2B0AB`。

执行命令（仓库根目录，顺序执行）：

```powershell
python -X utf8 scripts/cargo.py +stable test --workspace --all-features --locked --offline
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --all-features --lib hardware_ --locked --offline -- --ignored
```

首次硬件测试命令遗漏 `--all-features`，实际执行 0 项，因此不计为 GPU 验证。修正参数后确认运行 2 项、2 项通过。

## 已有补充证据及限制

600 成员合成分页回归验证跨页完整性、源顺序、net 0 排除、空页统计及取消。真实 AGILEX 最大网络的 15,168 成员、60 页源成员差分见 [原始记录](gpu-validation/members-agilex.json)；其中耗时属于单次 optimized-debug CPU 成员查询，不包含 UI/GPU，不作为 release 门槛。

本轮没有实际窗口点击、键盘、滚动、焦点或截图验收。硬件测试使用诊断像素回读，产品渲染仍使用 GPUI 同设备 D3D11 呈现。多文档状态、hover、板文字、设置及完整发布验收仍按研发计划继续开发，本记录不代表 BRD MVP 完成。
