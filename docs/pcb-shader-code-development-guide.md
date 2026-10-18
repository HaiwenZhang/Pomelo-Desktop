# PCB 二维和三维 Shader 源码与开发指南

日期：2026-10-18。面向当前 Pomelo Desktop 的 Rust、GPUI 和 D3D11／Metal／wgpu 架构。本文把本机安装包中找到的相关 shader 原文、渲染 pass 配置，以及 Pomelo 的独立实现示例直接放在代码块中，阅读不需要打开安装目录里的参考文件。


## 阅读与实施顺序

1. 先读“要解决的问题”和“如何接入现有项目”，确定职责、输入和 pass 顺序。
2. “Pomelo 完整示例”给出可独立检查的 Rust 数据契约及 HLSL，适合从一个合成三层板开始实现。
3. 附录 A 直接收录 PCB 专用 GLSL，附录 B 收录 pass XML，附录 C 补齐其 include 和选定后处理依赖，附录 D 收录内嵌 GLSL。
4. 开发时按下文批次验证。单独 shader 编译通过，不等于已经具备实体生成、GPUI 接入、GPU 状态管理或可用的三维视口。

## 要解决的问题

| 功能 | 原文中值得研究的部分 | Pomelo 开发决策 |
| --- | --- | --- |
| 重复焊盘、孔和组件 | RGBlock 的实例 SSBO、padstack 局部变换、主 ID 和层子 ID | 模板网格共享，实例记录源对象归属、层跨度和变换；不要为每个过孔复制整份网格 |
| 多层半透明 | BOIT、BOITComposite、StandardMaterial 的透明变体和 Default.xml 的附件 | 不透明先写深度；透明不写深度，积累颜色与 revealage 后合成；OIT 是近似透明 |
| 剖切 | CutPlanes 的局部平面、屏幕相关切边；Layout 的 clip plane 数组 | 所有 pass 用相同物理空间平面；切边着色不等于生成截面封口 |
| 对象选择 | Pick 的多命中记录、世界坐标、子对象和标志 | 先用 CPU BVH 完成循环选择；GPU ID 缓冲负责最近命中，扩展多命中时再处理原子缓冲和溢出 |
| 二维和三维辅助线 | LineSet 的屏幕空间扩张及加宽 pick 范围 | 用于标注、选中边和飞线；PCB 走线仍保留真实毫米宽度和当前解析曲线能力 |
| 层高和 Z 拉伸 | Layout 的 elev/use2d、SIwave 的 Z scale/shift | 物理层高来自数据，显示拉伸只改视图；测量用物理坐标 |
| 材质、文字和抗锯齿 | Shading、Text、GammaCorrection、FXAA3 | 首期稳定漫反射即可；保留既有 MSDF；分别核对线性色和宿主合成，避免二次 Gamma |

参考代码采用 Y 为竖直轴、XZ 为 PCB 平面的变换约定；本文的 Pomelo 示例采用 XY 板平面、Z 为厚度方向，不能只替换变量名后混用。GLSL `std430`、HLSL structured buffer、Metal 和 WGSL 的布局需要显式对齐，不能假定相同字段就有相同 stride。

## 如何接入现有项目

本次只交付文档。下面路径中的“新增”表示开发时才创建的生产模块，当前没有实现这些模块。

| 修改位置 | 具体工作 | 输入与输出 |
| --- | --- | --- |
| `pomelo-core/src/model/`，新增物理层叠契约 | 层厚、Z、高度来源、板域、孔跨度和元件实体；维持 LayerId/ObjectId | 已知物理结构，不以二维 layer_order 猜高度 |
| `pomelo-import/src/formats/` | 将支持的源字段转成统一契约，保留缺失诊断 | 物理模型与已有二维 BoardScene 同一来源身份 |
| `pomelo-render/src/scene/`，新增 solid 准备 | 从合法带孔覆盖生成顶面、底面、外侧壁和孔壁；共享模板与实例 | 顶点、法线、索引、实例、对象映射和 BVH |
| `pomelo-render/src/backend/solid_renderer.rs`（新增） | 持有三维资源和帧版本，组合不透明、OIT、边、选择和拾取 | 不可变 Board3dFrame，不直接读源文件 |
| `pomelo-render/src/backend/common/` | 增加三维附件、pass 与 capability 契约 | 跨平台语义，避免把 Direct3D 对象传到 UI |
| `pomelo-render/src/backend/d3d11/driver.rs` | 新建深度附件、MRT、独立 blend、整数拾取纹理、异步读回、输出状态恢复 | 原生资源与命令；当前二维 Draw 不直接支持这些全部能力 |
| `pomelo-render/src/backend/d3d11/shaders.rs` | 增加 Solid/Oit/Composite/Pick 入口；源码拼装与编译错误上下文 | HLSL 与实际入口名，保持旧的二维入口可用 |
| Metal/wgpu 对应 driver 和 shader | 按相同 ABI、数学空间、深度和 alpha 语义实现 | 三端分别验收，不能以 HLSL 编译代替另外两端 |
| `pomelo/src/document/preparation.rs` | 后台按需准备三维数据、取消、预算、来源身份校验 | 准备完成快照；三维失败仍能二维查看 |
| `pomelo/src/viewport/` | 相机、显示规则、工具状态、帧提交、候选与检查器联动 | BoardViewport 继续做状态唯一所有者 |
| `pomelo-core/src/view_state.rs` 与偏好服务 | V1 到新 schema 的显式迁移，保存 2D/3D 独立相机和显示字段 | 原子写入、来源不匹配拒绝恢复选择 |

现有 `CopperMesh` 是外轮廓覆盖减孔洞覆盖的二维合成数据；不能把其全部索引直接挤出成三维顶面。必须先求实际带孔区域，再生成合法实体表面。现有模板缓冲和深度类型也不能直接证明已实现三维遮挡：当前 D3D11 对应二维管线关闭深度测试和写入。

完整总体架构及 59 项功能映射见项目内的《PCB 三维显示开发设计》。本指南聚焦如何把 shader 和 pass 真正接入渲染器。

## GPU 数据和坐标契约

采用右手坐标、毫米和 f64 物理数据。上传前先在 f64 中减去 render origin，再转 f32；世界平面 `n·p+d>=0` 也要同时转换到相同的相对坐标。只减顶点而不更新剖切平面、光源、相机或拾取位置会产生空间漂移。

| 结构 | 字节大小 | HLSL 对应 |
| --- | --- | --- |
| 顶点 | 32 | position float4 + normal float4；顶点 xyz 为模板局部坐标 |
| 实例 | 128 | 3 行模型矩阵、3 行法线矩阵、linear RGBA、uint4 身份 |
| Frame | 240 | row-major VP、Z 显示参数、uint4 参数、灯光、8 个 float4 平面 |

示例的模板 Z 在 0..1，实例将其映射到真实 lower/upper；XY 模板以自身局部原点定义。模板网格必须已经有正确孔洞、侧面和法线。首期示例仅处理正 XY 缩放，镜像、任意三维资产和弯折需要更完整仿射变换与绕序处理。

原始颜色和上传颜色的边界要固定：示例 `linear_rgba` 是直通、未预乘的线性 RGBA。源 sRGB 颜色需在 CPU 或明确的转换环节转线性；不要把 alpha 也做 Gamma。对宿主目标究竟需要线性、sRGB、直通或预乘输出，先核对 GPUI 当前后端，再决定最终合成。

## 三维 pass 和 D3D11 状态

建议在 GPUI 设备上创建自有离屏颜色/深度及 OIT 附件，完成三维多遍后合成到当前窗口目标。所有目标属于同一设备；不能另建交换链，也不能跨帧保存 GPUI 借用的颜色目标或编码器。

| pass | 目标与 clear | 深度 | blend 与说明 |
| --- | --- | --- | --- |
| 背景 | opaque color 清背景色；depth 清 1 | 无须写深度 | 背景必须不透明，与后面 composite 的 alpha=1 约定一致 |
| 不透明实体 | opaque color + depth | LessEqual，写深度 | Replace；只提交不透明对象 |
| OIT | accum 清 0；reveal 清 1；共用 opaque depth | LessEqual，不写深度 | RT0 One/One/Add；RT1 Zero/InvSrcColor/Add；启用 IndependentBlend |
| OIT 合成 | 新的 scene color，不与输入 SRV 使用同一纹理 | 关闭深度 | Replace，示例一次完成透明与 opaque 的合成 |
| 边/标注 | scene color + 需要的深度 | 按语义选择 | 有限偏置，不能画出全部三角化对角线 |
| 选择 | 独立 overlay 或受控重画 | 正常遮挡为默认 | 穿透强调是显式模式；不盲目照搬原 XML 中 depthtest=always |
| GPU 最近拾取 | R32G32B32A32_UINT 清 0；独立 depth 清 1 | LessEqual，写深度 | Replace；提交所有可拾取可见对象，透明度零排除 |
| 最终合成 | GPUI 当前目标 | 使用宿主约定 | 应用内容裁剪、DPI、宿主 opacity 并恢复状态 |

建议附件格式为 RGBA16F 或经过能力核查的颜色格式，depth 使用支持的深度格式，accum 使用 RGBA16F、reveal 使用 R16F。原资源是 R8 reveal，不代表 Pomelo 必须照搬：层数多时要检查精度。密集透明叠加还需测 FP16 溢出；必要时降权或使用更高精度。

下面是独立重写的 D3D11 blend 描述符代码块，用于说明准确状态。它需要项目已有 windows crate 环境，未计入本文的独立 Rust 编译检查。

```rust
use windows::Win32::Graphics::Direct3D11::*;

fn oit_blend_descriptor() -> D3D11_BLEND_DESC {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.IndependentBlendEnable = true.into();
    desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_ONE,
        DestBlend: D3D11_BLEND_ONE,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_ONE,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    desc.RenderTarget[1] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_ZERO,
        DestBlend: D3D11_BLEND_INV_SRC_COLOR,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ZERO,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_RED.0 as u8,
    };
    desc
}
```

对只读深度/附件能力、IndependentBlend、浮点混合和纹理采样先查询设备支持。不支持时选择已经验证的降级模式并显示原因。MSAA 与 OIT 的每 sample/每 pixel resolve 不在首期代码范围，先用单采样附件完成正确流程。

## Pomelo 完整示例

以下三段是独立实现示例，完整代码直接嵌入本文。它们没有接入生产渲染器。Rust 不依赖外部 crate，HLSL 以 Shader Model 5 编译，适合先验证 Windows D3D11 路线。

GPU 顶点索引约定：使用 `DrawInstanced` 时 SV_VertexID 就是连续顶点号；使用 `DrawIndexedInstanced` 时它是索引缓冲提供的顶点号，供 shader 从 structured buffer 取顶点。实例 ID 必须与上传实例表和调用的起始实例约定一致。

### Rust 数据契约与数值检查

```rust
//! Standalone CPU contracts for the shader development guide; not an app renderer.
pub const MAX_CLIP_PLANES: usize = 8;

#[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct GpuInstance {
    pub model_rows: [[f32; 4]; 3],
    pub normal_rows: [[f32; 4]; 3],
    pub color: [f32; 4],
    /// GPU object key, layer key, component key, reserved; object key zero is empty.
    pub ids: [u32; 4],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct GpuFrame {
    pub view_projection: [[f32; 4]; 4],
    /// Relative Z anchor, positive display scale, unused, unused.
    pub z_display: [f32; 4],
    pub view_params: [u32; 4],
    pub light: [f32; 4],
    pub clip_planes: [[f32; 4]; MAX_CLIP_PLANES],
}
const _: () = assert!(std::mem::size_of::<GpuInstance>() == 128);
const _: () = assert!(std::mem::size_of::<GpuFrame>() == 240);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    NonFinite,
    InvalidSize,
    InvalidColor,
    EmptyObjectKey,
    InvalidPlane,
}

/// Template mesh Z is in [0, 1]; positive XY scales only in this prototype.
pub struct InstanceInput {
    pub at_mm: [f64; 2],
    pub angle_radians: f64,
    pub xy_scale: [f64; 2],
    pub z_span_mm: [f64; 2],
    pub render_origin_mm: [f64; 3],
    pub linear_rgba: [f32; 4],
    pub ids: [u32; 4],
}

fn narrow(value: f64) -> Result<f32, InputError> {
    let value32 = value as f32;
    if !value.is_finite() || !value32.is_finite() {
        return Err(InputError::NonFinite);
    }
    Ok(value32)
}

pub fn build_instance(input: &InstanceInput) -> Result<GpuInstance, InputError> {
    if input.ids[0] == 0 {
        return Err(InputError::EmptyObjectKey);
    }
    if !input.angle_radians.is_finite()
        || !input.at_mm.iter().all(|v| v.is_finite())
        || !input.render_origin_mm.iter().all(|v| v.is_finite())
        || !input.z_span_mm.iter().all(|v| v.is_finite())
    {
        return Err(InputError::NonFinite);
    }
    let [sx, sy] = input.xy_scale;
    let height = input.z_span_mm[1] - input.z_span_mm[0];
    if ![sx, sy, height].iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err(InputError::InvalidSize);
    }
    if !input.linear_rgba.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
        return Err(InputError::InvalidColor);
    }
    let (s, c) = input.angle_radians.sin_cos();
    let tx = narrow(input.at_mm[0] - input.render_origin_mm[0])?;
    let ty = narrow(input.at_mm[1] - input.render_origin_mm[1])?;
    let z0 = narrow(input.z_span_mm[0] - input.render_origin_mm[2])?;
    let height32 = narrow(height)?;
    let inverse_height32 = narrow(1.0 / height)?;
    if height32 <= 0.0 || inverse_height32 <= 0.0 {
        return Err(InputError::InvalidSize);
    }
    Ok(GpuInstance {
        model_rows: [
            [narrow(c * sx)?, narrow(-s * sy)?, 0.0, tx],
            [narrow(s * sx)?, narrow(c * sy)?, 0.0, ty],
            [0.0, 0.0, height32, z0],
        ],
        normal_rows: [
            [narrow(c / sx)?, narrow(-s / sy)?, 0.0, 0.0],
            [narrow(s / sx)?, narrow(c / sy)?, 0.0, 0.0],
            [0.0, 0.0, inverse_height32, 0.0],
        ],
        color: input.linear_rgba,
        ids: input.ids,
    })
}

/// Convert absolute n·p+d >= 0 to physical render-origin-relative space.
pub fn relative_clip_plane(
    normal: [f64; 3],
    d_mm: f64,
    render_origin_mm: [f64; 3],
) -> Result<[f32; 4], InputError> {
    if !normal.iter().chain(render_origin_mm.iter()).all(|v| v.is_finite())
        || !d_mm.is_finite()
    {
        return Err(InputError::NonFinite);
    }
    let length = normal[0].hypot(normal[1]).hypot(normal[2]);
    if !length.is_finite() || length <= 0.0 {
        return Err(InputError::InvalidPlane);
    }
    let n = normal.map(|v| v / length);
    let relative_d = d_mm / length
        + n[0] * render_origin_mm[0]
        + n[1] * render_origin_mm[1]
        + n[2] * render_origin_mm[2];
    Ok([narrow(n[0])?, narrow(n[1])?, narrow(n[2])?, narrow(relative_d)?])
}

pub fn display_z(z: f64, anchor: f64, scale: f64) -> Option<f64> {
    if ![z, anchor, scale].iter().all(|v| v.is_finite()) || scale <= 0.0 {
        return None;
    }
    let displayed = anchor + (z - anchor) * scale;
    displayed.is_finite().then_some(displayed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickStamp {
    pub document_generation: u64,
    pub camera_revision: u64,
    pub display_revision: u64,
    pub request_id: u64,
}
pub fn accepts_readback(current: PickStamp, completed: PickStamp) -> bool {
    current == completed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rebased_rotated_template_has_known_physical_top_corner() {
        let instance = build_instance(&InstanceInput {
            at_mm: [1001.0, 2002.0],
            angle_radians: std::f64::consts::FRAC_PI_2,
            xy_scale: [2.0, 4.0],
            z_span_mm: [10.0, 10.035],
            render_origin_mm: [1000.0, 2000.0, 10.0],
            linear_rgba: [0.6, 0.4, 0.2, 0.5],
            ids: [1, 2, 0, 0],
        }).unwrap();
        let point = [1.0, 0.0, 1.0, 1.0];
        let result = instance.model_rows.map(|row| {
            row.iter().zip(point).map(|(a, b)| a * b).sum::<f32>()
        });
        let expected = [1.0, 4.0, 0.035];
        assert!(result.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-5));
    }
    #[test]
    fn rebased_plane_preserves_absolute_half_space_distance() {
        let plane = relative_clip_plane([2.0, 0.0, 0.0], -2000.0, [999.0, 0.0, 0.0]).unwrap();
        assert_eq!(plane, [1.0, 0.0, 0.0, -1.0]);
    }
    #[test]
    fn display_stretch_preserves_anchor_and_roundtrips_coordinates() {
        let shown = display_z(1.25, 1.0, 8.0).unwrap();
        assert_eq!((shown, 1.0 + (shown - 1.0) / 8.0), (3.0, 1.25));
    }
    #[test]
    fn old_camera_readback_is_rejected() {
        let current = PickStamp {
            document_generation: 7, camera_revision: 20,
            display_revision: 3, request_id: 42,
        };
        let previous = PickStamp { camera_revision: 19, ..current };
        assert!(!accepts_readback(current, previous));
    }
}
```

### 实体 OIT 和最近拾取 HLSL

```hlsl
// Independent Pomelo prototype; positions share the same rebased physical space.
struct Vertex { float4 position; float4 normal; };
struct Instance {
    float4 model_rows[3];
    float4 normal_rows[3];
    float4 color;
    uint4 ids;
};
StructuredBuffer<Vertex> vertices : register(t0);
StructuredBuffer<Instance> instances : register(t1);
cbuffer Frame : register(b0) {
    row_major float4x4 view_projection;
    float4 z_display;
    uint4 view_params;
    float4 light;
    float4 clip_planes[8];
};
struct Surface {
    float4 position : SV_Position;
    float3 physical_position : TEXCOORD0;
    float3 displayed_normal : TEXCOORD1;
    nointerpolation float4 color : COLOR0;
    nointerpolation uint4 ids : TEXCOORD2;
};
Surface solid_vertex(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID) {
    Vertex vertex = vertices[vertex_id];
    Instance item = instances[instance_id];
    float4 p = float4(vertex.position.xyz, 1.0);
    float3 physical = float3(dot(item.model_rows[0], p),
                             dot(item.model_rows[1], p),
                             dot(item.model_rows[2], p));
    float3 displayed = physical;
    displayed.z = z_display.x + (physical.z - z_display.x) * z_display.y;
    float4 n = float4(vertex.normal.xyz, 0.0);
    float3 normal = float3(dot(item.normal_rows[0], n),
                           dot(item.normal_rows[1], n),
                           dot(item.normal_rows[2], n));
    normal.z /= z_display.y;
    Surface output;
    output.position = mul(view_projection, float4(displayed, 1.0));
    output.physical_position = physical;
    output.displayed_normal = normalize(normal);
    output.color = item.color;
    output.ids = item.ids;
    return output;
}
float minimum_plane_distance(float3 physical_position) {
    float result = 1e20;
    [loop] for (uint i = 0; i < min(view_params.x, 8u); ++i) {
        result = min(result, dot(clip_planes[i], float4(physical_position, 1.0)));
    }
    return result;
}
float3 shaded_color(Surface input) {
    float distance_to_plane = minimum_plane_distance(input.physical_position);
    float pixel_tolerance = max(fwidth(distance_to_plane), 1e-8);
    clip(distance_to_plane);
    float3 n = normalize(input.displayed_normal);
    float diffuse = saturate(dot(n, normalize(light.xyz)));
    float3 color = input.color.rgb * (light.w + (1.0 - light.w) * diffuse);
    if (view_params.x > 0u && distance_to_plane < pixel_tolerance) {
        color = lerp(color, float3(1.0, 1.0, 1.0), 0.65);
    }
    return color;
}
float4 solid_fragment(Surface input) : SV_Target0 {
    return float4(shaded_color(input), 1.0);
}
struct OitOutput { float4 accum : SV_Target0; float4 reveal : SV_Target1; };
OitOutput oit_fragment(Surface input) {
    float alpha = saturate(input.color.a);
    clip(alpha - 1e-6);
    float3 color = shaded_color(input);
    float weight = max(pow(clamp(alpha, 0.01, 0.99), 3.0), 1e-4);
    OitOutput output;
    output.accum = float4(color * alpha, alpha) * weight;
    output.reveal = alpha.xxxx;
    return output;
}
uint4 pick_fragment(Surface input) : SV_Target0 {
    clip(input.color.a - 1e-6);
    clip(minimum_plane_distance(input.physical_position));
    return input.ids;
}
```

### 透明合成 HLSL

```hlsl
Texture2D<float4> accumulation : register(t0);
Texture2D<float> revealage : register(t1);
Texture2D<float4> opaque_color : register(t2);
float4 fullscreen_vertex(uint id : SV_VertexID) : SV_Position {
    float2 uv = float2((id << 1u) & 2u, id & 2u);
    return float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
}
float4 composite_fragment(float4 position : SV_Position) : SV_Target0 {
    int3 pixel = int3(int2(position.xy), 0);
    float4 accum = accumulation.Load(pixel);
    float remaining = saturate(revealage.Load(pixel));
    float3 average = accum.rgb / max(accum.a, 1e-6);
    float3 opaque = opaque_color.Load(pixel).rgb;
    // Opaque input includes an opaque background, so output alpha is one.
    return float4(average * (1.0 - remaining) + opaque * remaining, 1.0);
}
```


## 示例的关键接线规则

### 实例与 Z 显示

`build_instance` 只把局部模板映射到已知物理跨度，不能推断层厚或生成过孔镀铜壁。shader 先得到 physical_position，再做展示 Z 缩放；剖切在 physical_position 上求距离，法线再补偿显示缩放的逆转置作用。

Frame 的 `z_display.y` 必须为有限正数；`light.xyz` 必须为非零单位方向，`light.w` 在 0..1；plane count 不超过 8，未用字段清零。VP 矩阵对应的是展示空间、render-origin-relative 坐标，并采用 D3D11 0..1 深度。CPU 和 GPU 的 row-major/乘法方向必须由已知点测试约束。

若剖切工具支持“展示空间定义平面”，先把平面转换回物理空间。不要把 Z 拉伸后的点直接代入物理平面。测量、属性与导出仍使用未拉伸物理值。

### OIT 与透明对象分类

opaque pass 只提交 alpha=1 的实体；transparent pass 提交 0<alpha<1 的实体。示例的 opaque shader 强制输出 alpha=1，因此不能把透明对象错误提交到该 pass。层/对象/网络的隐藏规则在生成 draw ranges 时生效，两种 pass 共用同一个可见性快照。

OIT 的 accum 保存加权预乘颜色和权重和，reveal 通过目标混合累乘剩余透过率；合成时用 accum.rgb/accum.a 求平均颜色，再按 reveal 混合 opaque 图像。它不等价于按真实表面顺序的精确透射，也不是折射或体积光学。不能把这个 pass 用作普通二维铜皮填充透明的默认替代。

合成示例的像素原点是离屏目标左上角，viewport 从 0,0 开始，三个输入纹理尺寸完全相同。若画布在窗口中偏移，只在最后 GPUI 合成时换算；不要在 OIT Load 地址中再次叠加窗口原点。

### 剖切、切边与 early tests

opaque、OIT 与 GPU picking 都调用相同的平面距离函数。切边用距离的屏幕导数估计宽度；原始实现采用世界坐标导数，这里重新在同一物理空间距离上估计。两者都没有构造截面封口。

不要给含 discard/clip 的 pass 强制添加会改变深度或副作用语义的 early-test 属性。尤其是多命中 shader 写入 SSBO/UAV 时，必须理解深度、裁剪和写副作用的执行顺序。原文中的 early_fragment_tests 条件值得连同 pass 一起读。

### GPU 最近拾取和多命中

示例 `pick_fragment` 是最近表面 ID pass，不能一次得到全部重叠对象。`ids.x` 是 GPU key，0 表示无对象；CPU 维护 key→领域对象/层/元件的映射，不把 u32 截断成 RGB8。读取一个像素或小区域，不逐帧同步读取整个画布。

创建至少双缓冲的 staging/readback 资源，给提交记录 PickStamp；完成回读只在文档、相机、显示与请求 ID 都未变化时接受。先用 CPU BVH 获取前后候选并按领域身份去重，能支持透明对象和 B 循环选择。

附录 A 的 Pick 原文展示原子计数器 + SSBO 多命中方案：每项有世界点、对象/subID、透明和拒绝标志。移植到 D3D11 需要 pixel UAV 和原子计数资源，Metal/wgpu 也需分别验证。写入顺序不能当作深度顺序，读取时按射线距离排序；计数超过容量必须重试或退回 CPU，不能把前 N 项静默当作完整候选。

### 二维可以先做什么

1. 继续保留当前走线、焊盘解析 shader 的毫米宽度、圆弧和坐标补偿。
2. 参考 LineSet 为标注/选中边生成固定像素宽度的三角形，处理零长度段、近裁面和 DPI。原文的额外 pick 像素宽度应转成 Pomelo 的统一容差。
3. 把显示、拾取和边框的对象身份统一，不从混色后的像素颜色猜对象。
4. 封装/过孔的模板实例数据可复用到三维；二维的 layer_order 继续只是显示顺序，物理高度单独保存。
5. 文字延续现有 MSDF，不因 engine/Text.glsl 含 SDF 就宣称原 PCB 标注在使用 SDF。原 AnnotationText technique 启用的是 ALPHAMAP，文字 pick pass 在该资源中被注释为未支持。

## 开发批次与完成条件

| 批次 | 实现 | 可验收结果 |
| --- | --- | --- |
| 1 | 物理层叠契约、两个实体盒子、自有颜色/深度、GPUI 合成 | 遮挡正确，resize/DPI/clip/关闭窗口不污染 UI |
| 2 | 合法带孔板域、铜层、焊盘/孔跨度、实例化及三维相机 | 可看板顶板底和盲孔；缺层高明确报缺失，不制造默认板厚 |
| 3 | 不透明/透明分类、OIT 附件和合成 | 多角度/不同提交顺序结果稳定；极端 alpha 和空透明帧正确 |
| 4 | 剖切、Z 显示、最近拾取与 CPU 多候选 | 被切除/隐藏对象不能命中；拉伸前后物理测量一致 |
| 5 | 辅助线、选择强调、标注、查看状态迁移 | 快捷键、取消、保存恢复、多文档与检查器一致 |
| 6 | Metal/WGSL 等价实现与性能 | 分别在目标设备验收，静止不忙刷新、旋转不整板重上传 |

每个批次都应做纵向接线，不能先把全部按钮建好，再长期留空。未来 API 名称、目录和帧类型以实施时的源码为准，不把文档示例当作已存在的项目接口。

## 验证

本文独立示例已通过 Rust 1.98.1 的库编译和 4 项测试：已知旋转/层厚变换、平面 rebasing、Z 显示往返、过期拾取拒绝。HLSL 六个入口均通过 Windows SDK 10.0.26100.0 FXC 的 `/WX` 编译：`solid_vertex`、`solid_fragment`、`oit_fragment`、`pick_fragment`、`fullscreen_vertex`、`composite_fragment`。

安装包原文未在其原厂预处理环境编译。本文也未完成像素、实窗、Metal 或 wgpu 的 GPU 验收。D3D11 blend 描述符片段仅展示集成状态，不在独立编译范围。

未来 GPU 验证至少包括：透明顺序置换、0/1 alpha、多重孔洞、正反面法线、多个裁切面、Z 拉伸后的拾取、空附件、readback 延迟、全透明层、设备恢复和 GPUI 状态恢复。截图比较需记录 GPU、附件格式、像素尺寸和采样口径。

## 原文代码索引

共收录 **22 个 GLSL**（11 个 PCB 专用文件和 11 个依赖/选定效果）、**5 个 XML 配置**，以及 **34 段去重 GLSL**（对应原始 49 个字符串位置）。另含 3 段完整 Pomelo 示例和 1 段 blend 状态接线代码。

| 部分 | 名称 | 内容 |
| --- | --- | --- |
| A | RGBlock.glsl | PCB 专用 shader 完整原文，463 行 |
| A | StandardMaterial.glsl | PCB 专用 shader 完整原文，222 行 |
| A | BOIT.glsl | PCB 专用 shader 完整原文，60 行 |
| A | BOITComposite.glsl | PCB 专用 shader 完整原文，40 行 |
| A | CutPlanes.glsl | PCB 专用 shader 完整原文，91 行 |
| A | Pick.glsl | PCB 专用 shader 完整原文，130 行 |
| A | LineSet.glsl | PCB 专用 shader 完整原文，182 行 |
| A | Shading.glsl | PCB 专用 shader 完整原文，340 行 |
| A | Annotation.glsl | PCB 专用 shader 完整原文，117 行 |
| A | Background.glsl | PCB 专用 shader 完整原文，136 行 |
| A | Utilities.glsl | PCB 专用 shader 完整原文，115 行 |
| B | Default.xml | pass / 附件 / 变体配置，123 行 |
| B | Annotation.xml | pass / 附件 / 变体配置，22 行 |
| B | AnnotationLineSet.xml | pass / 附件 / 变体配置，22 行 |
| B | AnnotationText.xml | pass / 附件 / 变体配置，23 行 |
| B | StandardMaterial.xml | pass / 附件 / 变体配置，30 行 |
| C | BRDF.glsl | 完整依赖 / 选定效果源码，164 行 |
| C | Constants.glsl | 完整依赖 / 选定效果源码，7 行 |
| C | CopyFramebuffer.glsl | 完整依赖 / 选定效果源码，20 行 |
| C | FXAA3.glsl | 完整依赖 / 选定效果源码，747 行 |
| C | GammaCorrection.glsl | 完整依赖 / 选定效果源码，21 行 |
| C | PostProcess.glsl | 完整依赖 / 选定效果源码，89 行 |
| C | Samplers.glsl | 完整依赖 / 选定效果源码，84 行 |
| C | ScreenPos.glsl | 完整依赖 / 选定效果源码，58 行 |
| C | Text.glsl | 完整依赖 / 选定效果源码，122 行 |
| C | Transform.glsl | 完整依赖 / 选定效果源码，221 行 |
| C | Uniforms.glsl | 完整依赖 / 选定效果源码，222 行 |
| D01 | LayoutBaseUI.dll | 顶点候选，981 字节；1 个位置 |
| D02 | LayoutBaseUI.dll | 顶点候选，1579 字节；1 个位置 |
| D03 | LayoutBaseUI.dll | 片段/其他候选，350 字节；1 个位置 |
| D04 | SharedUIDLL.dll | 顶点候选，1912 字节；2 个位置 |
| D05 | SharedUIDLL.dll | 片段/其他候选，2062 字节；2 个位置 |
| D06 | SharedUIDLL.dll | 顶点候选，2887 字节；2 个位置 |
| D07 | SharedUIDLL.dll | 片段/其他候选，3271 字节；2 个位置 |
| D08 | SharedUIDLL.dll | 顶点候选，509 字节；2 个位置 |
| D09 | SharedUIDLL.dll | 片段/其他候选，393 字节；3 个位置 |
| D10 | SharedUIDLL.dll | 顶点候选，377 字节；2 个位置 |
| D11 | SharedUIDLL.dll | 片段/其他候选，191 字节；3 个位置 |
| D12 | SharedUIDLL.dll | 顶点候选，1038 字节；2 个位置 |
| D13 | SharedUIDLL.dll | 片段/其他候选，251 字节；3 个位置 |
| D14 | SharedUIDLL.dll | 顶点候选，716 字节；2 个位置 |
| D15 | SharedUIDLL.dll | 顶点候选，381 字节；2 个位置 |
| D16 | siwave.exe | 顶点候选，1917 字节；1 个位置 |
| D17 | siwave.exe | 片段/其他候选，1396 字节；1 个位置 |
| D18 | siwave.exe | 顶点候选，290 字节；1 个位置 |
| D19 | siwave.exe | 片段/其他候选，96 字节；1 个位置 |
| D20 | siwave.exe | 顶点候选，539 字节；1 个位置 |
| D21 | siwave.exe | 顶点候选，432 字节；1 个位置 |
| D22 | siwave.exe | 片段/其他候选，321 字节；1 个位置 |
| D23 | StackupCalc.dll | 片段/其他候选，2637 字节；1 个位置 |
| D24 | StackupCalc.dll | 顶点候选，1084 字节；1 个位置 |
| D25 | StackupCalc.dll | 片段/其他候选，348 字节；1 个位置 |
| D26 | StackupCalc.dll | 顶点候选，480 字节；1 个位置 |
| D27 | StackupCalc.dll | 片段/其他候选，395 字节；1 个位置 |
| D28 | StackupCalc.dll | 顶点候选，501 字节；1 个位置 |
| D29 | StackupCalc.dll | 片段/其他候选，273 字节；1 个位置 |
| D30 | StackupCalc.dll | 顶点候选，399 字节；1 个位置 |
| D31 | StackupCalc.dll | 顶点候选，414 字节；1 个位置 |
| D32 | StackupCalc.dll | 顶点候选，762 字节；1 个位置 |
| D33 | Phoenix.dll | 顶点候选，341 字节；1 个位置 |
| D34 | Phoenix.dll | 片段/其他候选，608 字节；1 个位置 |

原始文件和嵌入字符串均在生成文档前按此前记录的 SHA256 核对。嵌入字符串按完整字符串字节去重，相同代码的所有文件偏移列在对应代码块前；偏移是文件偏移，不能直接作为虚拟地址或函数地址使用。字符串不等于反编译 C++ 代码，也不等于已确认运行时绑定。

## 附录 A PCB 专用 Shader 原文

### RGBlock.glsl

原始位置：`data/Shaders/GLSL/RGBlock.glsl`。

SHA256：`04b9f438fa7e9804c54f8f72dd130d9b1395df51efb55817c1252289bbbeffa9`。

````glsl
#version 430 

// enable early fragment tests if possible
#if defined(COMPILEPS) && !defined(CUT_PLANES_ENABLED)
layout (early_fragment_tests) in;
#endif

/////////////////////////////////////////////////////
// REQUIRED ENGINE DEFINITIONS
// always enable instancing (we will toggle it based on uniform values)
#define INSTANCED
// always assume simple directional lighting
#define DIRLIGHT

/////////////////////////////////////////////////////
// PASS DEFINTIONS

// setup pick pass options
#ifdef RG_PICK_PASS
    #define PICK
    #define NO_SHADING
#endif

// setup base pass options
#ifdef RG_BASE_PASS
    #define SHADING_MODEL_DEFAULT
#endif

// setup alpha pass options
#ifdef RG_BASE_ALPHA_PASS
    #define BOIT
    #define SHADING_MODEL_DEFAULT
#endif

// setup highlight pass options
#ifdef RG_HIGHLIGHT_PASS
    #define NO_SHADING
    #define NO_ON_CUT_PLANE_COLOR
#endif

// setup selection pass options
#ifdef RG_SELECTION_PASS
    #define NO_SHADING
    #define NO_ON_CUT_PLANE_COLOR
#endif

/////////////////////////////////////////////////////

// Setup blended order-independent rendering requirements
#ifdef BOIT
    // need multi-target rendering for accum/composite
    #define MRT
    #define MRT_COUNT 2
    // tone mapping will be performed in the composite step
    #define SHADING_NO_TONEMAP
    // weight mode for blending
    #define BOIT_WEIGHT_MODE BOIT_WEIGHT_OPTIMIZED
#endif

/////////////////////////////////////////////////////
// URHO3D SHADER INCLUDES
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "PostProcess.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "Constants.glsl"

/////////////////////////////////////////////////////
// OUR SHADER INCLUDES
#include "Utilities.glsl"

// Setup shading model
#if !defined(NO_SHADING)
#include "Shading.glsl"
#endif

// Setup pick-write support
#if defined(PICK)
#include "Pick.glsl"
#endif

// Setup blended OIT
#if defined(BOIT)
#include "BOIT.glsl"
#endif

// Setup cutting planes support
#if defined(CUT_PLANES_ENABLED)
#include "CutPlanes.glsl"
#endif

//////////////////////////////////////////////////////
// FEATURE FLAG DEFINITIONS
//
// These flags are used to enabled/disable various features
// of the shader through the "cFeatureFlags" uniform. 
// 
// These flag values MUST be kept in sync with the flag values
// defined in RGRenderer::ShaderState::OptionBit
//

// The shader should source its transform data from the incoming
// vertext instance arrays
#define OPTION_FLAG_INSTANCE_DATA           (0x01 << 0)

// The shader should apply incoming transform information from the
// data bound in the "cPadstackItemData" SSBO
#define OPTION_FLAG_PADSTACK_DATA           (0x01 << 1)

// The shader should invert normal orientations, typically indicates
// that the current pass is rendering backfaces as outward facing
#define OPTION_FLAG_INVERT_NORMALS          (0x01 << 2)

// The current pass is a stencil-only operation, e.g. no color data is 
// being written so any color calculations can be omitted
#define OPTION_FLAG_STENCIL_ONLY_PASS       (0x01 << 3)

// The current pass is a pick operation pass, so any operations that
// are irrelevant for pick calculation can be omitted
#define OPTION_FLAG_PICK_ENABLED            (0x01 << 4)

// The current pass is writing color information to the output buffer
// so all color calculations should be included
#define OPTION_FLAG_COLOR_ENABLED           (0x01 << 5)

// Flag indicating  the render is for pick reject fragments, fragments that 
// will reject other pick results at the same depth level with matching
// reject mask assignments
#define OPTION_FLAG_PICK_REJECT_PASS        (0x01 << 6)

// Flag indicating the render is for fragments that may be rejected 
// by the pick rejection facility
#define OPTION_FLAG_PICK_REJECT_ALLOWED     (0x01 << 7)

// Flag indicating that color dimming should be applied for the active
// operation
#define OPTION_FLAG_COLOR_DIMMING_ACTIVE    (0x01 << 8)

//////////////////////////////////////////////////////
// Option flag test, check if the current option flag is enabled
#define IsOptionEnabled(flag)  ((cOptionFlags & flag) != 0)

//////////////////////////////////////////////////////
// VERTEX PROGRAM DEFINITIONS
#ifdef COMPILEVS

// MUST be kept in-sync with cpu-side enum values
#define RG_BUFFER_BIND_ITEM_XFORM               4
#define RG_BUFFER_BIND_PAD_STACK_ITEM_XFORM     5

////////////////////////////////
// VERTEX STAGE UNIFORMS

// Option feature flags enabled for the shader (shared with fragment stage)
uniform int cOptionFlags = 0;

// Uniform value for object ID's when the ID is not provided
// by the instance data
uniform int cPickID = 0;
uniform vec3 cPickSubID = vec3(0);

// World vertical min/max extents for the current group
uniform float cGroupMin = 0;
uniform float cGroupMax = 0;

////////////////////////////////
// VERTEX STAGE INPUTS

// Additional input vertex stream for padstack item data index
attribute int iObjectIndex1;

// GPU data layout for a transform object
// THIS MUST BE KEPT IN SYNC WITH : RGInstanceItemVBOData
struct ItemInstanceData
{
    float tx; // x translation
    float ty; // y translation
    float cr; // cos(theta) of rotation

    float sx; // x scale factor
    float sy; // y scale factor
    float sr; // sin(theta) of rotation

    // Overall object picking ID
    int pickID;

    // Subobject identifiers
    int subID1;
    int subID2;
    int subID3;
};

// GPU data layout for a transform object
// THIS MUST BE KEPT IN SYNC WITH : RGPadStackItemVBOData
struct PadStackItemData
{
    float tx; // x translation
    float ty; // y translation
    float cr; // cos(theta) of rotation

    float sx; // x scale factor
    float sy; // y scale factor
    float sr; // sin(theta) of rotation

    // Subobject tag identifier, overrides the tag of the instance
    // item for padstack objects
    int subIDTag;

    // Subobject layer identifier, overrides the relevant layer subid data
    // item for padstack objects
    int subIDLayer;
};


// Item instance data provided through SSBO
layout (binding=RG_BUFFER_BIND_ITEM_XFORM, std430) restrict readonly buffer cItemInstanceData
{
    ItemInstanceData itemInstanceData[];
};

// Padstack item data provided through SSBO
layout (binding=RG_BUFFER_BIND_PAD_STACK_ITEM_XFORM, std430) restrict readonly buffer cPadStackItemData
{
    PadStackItemData padsInstanceData[];
};

////////////////////////////////
// VERTEX STAGE OUTPUTS
out vec3 vNormal;
out vec4 vWorldPos;
out float vViewDepth;
flat out int vPickID;
flat out ivec3 vSubObjectIDs;

////////////////////////////////
// VERTEX STAGE UTILITY FUNCTIONS

ItemInstanceData GetItemInstanceData()
{
    ItemInstanceData result;
    
    ItemInstanceData item = itemInstanceData[iObjectIndex];
    if(IsOptionEnabled(OPTION_FLAG_PADSTACK_DATA)) {
        PadStackItemData padItem = padsInstanceData[iObjectIndex1];
        // combine pad data transform with item transform    
        result.tx = item.tx + (padItem.tx * item.cr - padItem.ty * item.sr);
        result.ty = item.ty + (padItem.tx * item.sr + padItem.ty * item.cr);    
        result.sx = item.sx * padItem.sx;
        result.sy = item.sy * padItem.sy;
        result.cr = (item.cr * padItem.cr) - (item.sr * padItem.sr);
        result.sr = (item.sr * padItem.cr) + (item.cr * padItem.sr);
        result.pickID = item.pickID;
        result.subID1 = padItem.subIDTag;
        result.subID2 = item.subID2;
        result.subID3 = padItem.subIDLayer;
    } else {
        result = item;
    }

    return result;
}


// Get the 4x4 matrix for a given Item transform
void GetItemData(out mat4 itemXForm, out int pickID, out ivec3 subObjectIDs)
{    
    vec3 t = vec3(0.0, cGroupMin, 0.0);
    vec3 s = vec3(1.0, cGroupMax - cGroupMin, 1.0);
    vec2 r = vec2(1.0, 0.0);

    if(IsOptionEnabled(OPTION_FLAG_INSTANCE_DATA)) {
        ItemInstanceData itemData = GetItemInstanceData();
        t.x = itemData.tx;
        t.z = itemData.ty;
        s.x = itemData.sx;
        s.z = itemData.sy;
        r.x = itemData.cr;
        r.y = itemData.sr;    
        pickID = itemData.pickID;
        subObjectIDs = ivec3(itemData.subID1, itemData.subID2, itemData.subID3);
    } else {
        pickID = cPickID;
        subObjectIDs.x = floatBitsToInt(cPickSubID.x);
        subObjectIDs.y = floatBitsToInt(cPickSubID.y);
        subObjectIDs.z = floatBitsToInt(cPickSubID.z);
    } 

    vec4 r1 = vec4(s.x * r.x,     0, s.z * -r.y,   t.x);
    vec4 r2 = vec4(        0,   s.y,          0,   t.y);
    vec4 r3 = vec4(s.x * r.y,     0,  s.z * r.x,   t.z);
    vec4 r4 = vec4(        0,     0,          0,     1);

    itemXForm = mat4(r1, r2, r3, r4);
}

#endif

//////////////////////////////////////////////////////
// VERTEX PROGRAM
void VS()
{
    // read item vertex data
    mat4 itemMatrix;
    GetItemData(itemMatrix, vPickID, vSubObjectIDs);

    mat4 modelMatrix = itemMatrix * cModel;
    vec3 worldPos = GetWorldPos(modelMatrix);

    // Vertex outputs
    gl_Position = GetClipPos(worldPos);        
    vViewDepth = GetDepth(gl_Position);
    vNormal = normalize(GetWorldNormal(modelMatrix));
    vWorldPos = vec4(worldPos, GetDepth(gl_Position)); 

    if(IsOptionEnabled(OPTION_FLAG_INVERT_NORMALS)) {
        vNormal = -vNormal;
    }
}


//////////////////////////////////////////////////////


//////////////////////////////////////////////////////
// FRAGMENT PROGRAM DEFINITIONS
#ifdef COMPILEPS

///////////////////////////////////////////////
// Inputs from vertex program
in vec3 vNormal;
in vec4 vWorldPos;
in float vViewDepth;
flat in int vPickID;
flat in ivec3 vSubObjectIDs;

///////////////////////////////////////////////
// Uniforms

// Design ID uniform
uniform int cDesignID = 0;

// Design-to-world and world-to-design instance transforms
uniform mat4 cDesignToWorld;
uniform mat4 cWorldToDesign;

// Feature option flags for the shader (shared with Vertex stage)
uniform int cOptionFlags = 0;

// the pick tag mask for the current pick pass
uniform int cPickTagMask = 0;

// the current tag value for generated pick buffer items
uniform int cPickTagValue = 0;

// Color for alpha blending onto the base color, e.g. used to visualize
// selection set when not using a separate base pass 
uniform vec4 cBlendColor = vec4(0.0);

// Color dimming parameters when color dimming is enabled
//      x - brightness
//      y - saturation
uniform vec2 cDimColorParams = vec2(1.0, 1.0);

////////////////////////////
// FRAGMENT HELPER FUNCTIONS

// Helper function to compute the color
vec4 ComputeColor()
{
    // the initial color
    vec4 color = cMatDiffColor;
    
    // do dimming
    if (IsOptionEnabled(OPTION_FLAG_COLOR_DIMMING_ACTIVE)) {
        color.rgb = DimColorRgb(color.rgb, cDimColorParams.x, cDimColorParams.y);
    }

    // do blending
    color = AlphaBlend(cBlendColor, color);

#if !defined(NO_GAMMA)
    color = ToLinearRGBA(color);
#endif

#if !defined(NO_SHADING)
    color = DoShading(color, vWorldPos.xyz, vNormal);
#endif

    // color result
    return color;
}


// Helper function to do pick collection
void DoPick(vec4 color)
{
#if defined(PICK)
    uint flags = 0;
    if(IsOptionEnabled(OPTION_FLAG_PICK_REJECT_PASS)) {
        flags |= PICK_BUFFER_ITEM_FLAG_REJECT_FRAGMENT;
    }
    if (IsOptionEnabled(OPTION_FLAG_PICK_REJECT_ALLOWED)) {
        flags |= PICK_BUFFER_ITEM_FLAG_ALLOW_REJECT;
    }
    WritePick(gl_FragCoord.xy, vWorldPos.xyz, color, cDesignID, vPickID, vSubObjectIDs, flags, cPickTagMask, cPickTagValue);
#endif
}


// Helper function to do the color target write
void DoColorWrite(vec4 color)
{
#if defined(BOIT)    
    gl_FragData[0] = BOITAccum(color);
    gl_FragData[1] = BOITReveal(color);
#else
    gl_FragData[0] = color;
#endif               
}

#endif

//////////////////////////////////////////////////////
// FRAGMENT PROGRAM
void PS()
{       
    // Handle cutting plane discard    
#if defined(CUT_PLANES_ENABLED)
    int cutPos = CUT_PLANE_POS_INSIDE;
    if (!IsOptionEnabled(OPTION_FLAG_STENCIL_ONLY_PASS)) {
        cutPos = GetCutPlanePosition(vWorldPos.xyz, cWorldToDesign);
        if (cutPos == CUT_PLANE_POS_OUTSIDE) {
            discard;
        }
    }
#endif

    // color result for the fragment
    vec4 color = vec4(0.0);

    // compute the fragment color
    if(IsOptionEnabled(OPTION_FLAG_COLOR_ENABLED)) {
        color = ComputeColor();
    }

    // Handle fragments that fall on the plane
#if !defined(PICK) && defined(CUT_PLANES_ENABLED) && !defined(NO_ON_CUT_PLANE_COLOR)
    if(cutPos == CUT_PLANE_POS_ON_PLANE) {
        color = GetOnCutPlaneColor(color);
    }
#endif

    // pick results writing
    if(IsOptionEnabled(OPTION_FLAG_PICK_ENABLED)) {
        DoPick(color);
    }

    // write render targets
    DoColorWrite(color); 
}

//////////////////////////////////////////////////////
````

### StandardMaterial.glsl

原始位置：`data/Shaders/GLSL/StandardMaterial.glsl`。

SHA256：`de00989aae1948fbbcd52eabc8b05bf36b6c10beeafb10da835e59e49cfad470`。

````glsl
#version 430 

// Enable early fragment tests if possible, must disable in case of
// cutting planes as cutting planes rely on "discard" which is 
// incompatible with early fragment tests
#if defined(COMPILEPS) && !defined(CUT_PLANES_ENABLED)
layout (early_fragment_tests) in;
#endif

/////////////////////////////////////////////////////
// REQUIRED ENGINE DEFINITIONS
// always assume simple directional lighting
#define DIRLIGHT

// Check if this is a BOIT pass
#if defined(BOIT_ACCUM) || defined(BOIT_REVEAL)
    #define BOIT
    #define SHADING_NO_TONEMAP
#endif

/////////////////////////////////////////////////////
// URHO3D SHADER INCLUDES
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "PostProcess.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "Constants.glsl"

/////////////////////////////////////////////////////
// OUR SHADER INCLUDES
#include "Utilities.glsl"

// Setup shading model for the material
#if !defined(NO_SHADING)
#define SHADING_MODEL_DEFAULT
#include "Shading.glsl"
#endif

// Setup pick utilities if this is a pick pass
#if defined(PICK)
#include "Pick.glsl"
#endif

// Setup OIT utilities if this is an alpha pass
#if defined(BOIT)
#define BOIT_WEIGHT_FACTOR 3.0
#include "BOIT.glsl"
#endif

// Setup cutting planes if cutting planes are enabled
#if defined(CUT_PLANES_ENABLED)
#include "CutPlanes.glsl"
#endif

//////////////////////////////////////////////////////
// VERTEX PROGRAM DEFINITIONS
#ifdef COMPILEVS

// Per-instance extended inputs (e.g. pick id)
attribute vec4 iTexCoord7;

// Vertex shader outputs
out vec3 vNormal;
out vec4 vWorldPos;

#ifdef USE_VERTEX_COLOR
out vec3 vColor;
#endif

// pick identifiers
#ifdef PICK
flat out int vObjectID;
flat out ivec3 vObjectSubID;
#endif

#endif

//////////////////////////////////////////////////////
// VERTEX PROGRAM
void VS()
{        
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vNormal = GetWorldNormal(modelMatrix);
    vWorldPos = vec4(worldPos, GetDepth(gl_Position));

#ifdef PICK
    vObjectID = floatBitsToInt(iTexCoord7.x);
    vObjectSubID.x = floatBitsToInt(iTexCoord7.y);
    vObjectSubID.y = floatBitsToInt(iTexCoord7.z);
    #ifdef USE_VERTEX_GEOMETRY_ID
    vObjectSubID.z = iObjectIndex;
    #else
    vObjectSubID.z = floatBitsToInt(iTexCoord7.w);
    #endif
#endif

#ifdef USE_VERTEX_COLOR
    vColor = iColor.rgb;
#endif

}
//////////////////////////////////////////////////////


//////////////////////////////////////////////////////
// FRAGMENT PROGRAM DEFINITIONS
#ifdef COMPILEPS

// Inputs from vertex program
in vec3 vNormal;
in vec4 vWorldPos;

#ifdef USE_VERTEX_COLOR
in vec3 vColor;
#endif

#ifdef PICK
flat in int vObjectID;
flat in ivec3 vObjectSubID;
#endif

// Design ID uniform
uniform int cDesignID = 0;

// Design-to-world and world-to-design instance transforms
uniform mat4 cDesignToWorld;
uniform mat4 cWorldToDesign;

// Selection color to use when selection color is enabled
uniform vec4 cSelectionColor;

// Highlight color ot use when highglight color is enabled
uniform vec4 cHighlightColor;

// Color dimming parameters when color dimming is enabled
//      x - brightness
//      y - saturation
uniform vec2 cDimColorParams = vec2(1.0, 1.0);

//////////////////////////////////////////////////////
// FRAGMENT PROGRAM HELPERS
void DoColorWrite(vec4 color)
{
    gl_FragData[0] = color;
}

vec4 GetColor()
{
    // Source color for the fragment
    #if defined(USE_SELECTION_COLOR)
        vec4 color = cSelectionColor;
    #elif defined(USE_HIGHLIGHT_COLOR)
        vec4 color = cHighlightColor;
    #elif defined(USE_VERTEX_COLOR)
        vec4 color = vec4(vColor, cMatDiffColor.a);
    #else
        vec4 color = cMatDiffColor;
    #endif

    #if defined(USE_COLOR_DIMMING)
        color.rgb = DimColorRgb(color.rgb, cDimColorParams.x, cDimColorParams.y);
    #endif

    #if defined(BLEND_SELECTION_COLOR)
        color = AlphaBlend(cSelectionColor, color);
    #endif

    #if !defined(NO_GAMMA)
        color = ToLinearRGBA(color);
    #endif

    #if !defined(NO_SHADING)
        color = DoShading(color, vWorldPos.xyz, vNormal);
    #endif

    return color;
}

#endif 

//////////////////////////////////////////////////////
// FRAGMENT PROGRAM
void PS()
{       

// Check for cutting plane discard
#if defined(CUT_PLANES_ENABLED)
    int cutPos = GetCutPlanePosition(vWorldPos.xyz, cWorldToDesign);
    if (cutPos == CUT_PLANE_POS_OUTSIDE) {
        discard;
    }
#endif

    vec4 color = GetColor();

#if !defined(PICK) && defined(CUT_PLANES_ENABLED) && !defined(NO_ON_CUT_PLANE_COLOR)
    if(cutPos == CUT_PLANE_POS_ON_PLANE) {
        color = GetOnCutPlaneColor(color);
    }
#endif

#if defined(BOIT)                     
    #if defined(BOIT_ACCUM)            
        color = BOITAccum(color);
    #endif
    #if defined(BOIT_REVEAL)    
        color = BOITReveal(color);
    #endif        
#endif

#if defined(PICK)
    WritePick(gl_FragCoord.xy, vWorldPos.xyz, color, cDesignID, vObjectID, vObjectSubID);
#endif    

    // write render targets
    DoColorWrite(color);
}

//////////////////////////////////////////////////////
````

### BOIT.glsl

原始位置：`data/Shaders/GLSL/BOIT.glsl`。

SHA256：`0d0f99855054a0ec3674db749379cd7ea77c1ef3a696967f39b565fc05028cc1`。

````glsl
#ifdef COMPILEPS
// Support for "blended order-independent (weighted) transparency"
// See the following references:
//  http://casual-effects.blogspot.com/2015/03/implemented-weighted-blended-order.html
//   Morgan McGuire and Louis Bavoil, Weighted Blended Order-Independent Transparency, Journal of Computer Graphics Techniques (JCGT), vol. 2, no. 2, 122-141, 2013
//   Available online http://jcgt.org/published/0002/02/09/
//
// Provides fast approximate transparency calculations that typically provides
// "good enough" results without requiring the renderer to resolve per-pixel
// blend orderings which results in significant performance gains over sorting-based
// approaches
//

#define BOIT_AW_MIN         0.0001
#define BOIT_AW_MAX         0.9999
#define BOIT_AW_POW         3.0

#define BOIT_WEIGHT_SCALE   100

// the overall weight factor, supports weighting 
// various geometry passes differently
#ifndef BOIT_WEIGHT_FACTOR
#define BOIT_WEIGHT_FACTOR  1.0
#endif

// Compute the BOIT weighting for the given color
float BOITWeight(vec4 color)
{
    // alpha colr value 
    float a = color.a;    

    // compute alpha weight
    float aW = pow(clamp(a, 0.01, 0.99), BOIT_AW_POW);
    aW = clamp(aW, BOIT_AW_MIN, BOIT_AW_MAX);               

    float weight = aW * BOIT_WEIGHT_SCALE * BOIT_WEIGHT_FACTOR;
    return weight;
}

vec4 BOITAccum(vec4 color)
{    
    float weight = BOITWeight(color);  
    return vec4(color.rgb * color.a, color.a) * weight;
}

vec4 BOITReveal(vec4 color)
{
    float reveal = color.a;
    return vec4(reveal);
}

vec4 BOITCompose(ivec2 pixel, sampler2D accumTex, sampler2D revealTex)
{
    float revealage = texelFetch(revealTex, pixel, 0).r;
    vec4 accum = texelFetch(accumTex, pixel, 0);
    vec3 averageColor = accum.rgb / max(accum.a, 0.00001);
    return vec4(averageColor, 1.0 - revealage);
}

#endif
````

### BOITComposite.glsl

原始位置：`data/Shaders/GLSL/BOITComposite.glsl`。

SHA256：`7a591b02cba72b8f3ba8057a40744478f50155071cdab86d0510829f2c6d7ada`。

````glsl
#version 430

// Resolves BOIT intermediate buffers to output buffer
// See : BOIT.glsl for reference
#include "Constants.glsl"
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "PostProcess.glsl"

// custom shaders
#include "BOIT.glsl"
#include "Utilities.glsl"
#include "Shading.glsl"

// BOIT source textures for composition
#ifdef COMPILEPS
layout (binding=0) uniform sampler2D sAccum;
layout (binding=1) uniform sampler2D sReveal;
#endif

varying vec2 vScreenPos;

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vScreenPos = GetScreenPosPreDiv(gl_Position);
}

void PS()
{
    ivec2 pixel = ivec2(gl_FragCoord.x, gl_FragCoord.y);
    vec4 color = BOITCompose(pixel, sAccum, sReveal);
    color = ShadingTonemapColor(color);
    gl_FragColor = color;
}
````

### CutPlanes.glsl

原始位置：`data/Shaders/GLSL/CutPlanes.glsl`。

SHA256：`6c66cefac1882e8f45f65d132aad02d3784c3b624d4b8abb1576b0e898c051c3`。

````glsl

#if defined(COMPILEPS) && defined(CUT_PLANES_ENABLED)

// Maximum number of concurrently active cutting planes. Note, this should be
// kept in sync with the limits defined in the C++ code to ensure that the 
// uniform size agrees with the supplied input
#define MAX_CUT_PLANES_COUNT        16

// The position is outside the volume defined by the cutting planes
// and should be discarded
#define CUT_PLANE_POS_INSIDE        0

// The position is "on" a cutting plane, within the tolerance
#define CUT_PLANE_POS_ON_PLANE      1

// The position is "inside" the volume defined by the cutting planes
// and should be kept
#define CUT_PLANE_POS_OUTSIDE       2

// Cutting plane definitions, each cutting plane is represented
// as the 4 parameters of the plane equation (ax + by + cz + d = 0) 
// in design-local space, where "abcd" correspond to "xyzw" of the vec4
//
uniform vec4 cCutPlanes[MAX_CUT_PLANES_COUNT];

// Current number of active cutting planes for the view, must be
// less than MAX_CUT_PLANES_COUNT
uniform int cCutPlanesCount;


// Classify the given world position according to the current cutting planes.
// The worldToLocal matrix transforms the world-space position into the
// design-local coordinate space where the cutting plane equations are defined.
int GetCutPlanePosition(vec3 worldPos, mat4 worldToLocal)
{    
    if(cCutPlanesCount <= 0) {
        return CUT_PLANE_POS_INSIDE;
    }

    // Transform world position to design-local space where planes are defined
    vec3 localPos = (vec4(worldPos, 1.0) * worldToLocal).xyz;

    // Find minimum signed distance and min signed distance plane
    int signedDistancePlane = -1;
    float signedDistanceMin = 100000;
    for(int i=0; i < cCutPlanesCount; ++i) {
        float signedDistance = dot(cCutPlanes[i].xyz, localPos) + cCutPlanes[i].w;
        if(signedDistance < signedDistanceMin) {
            signedDistanceMin = signedDistance;
            signedDistancePlane = i;
        }
    }
                    
    // Estimate world-space pixel size along the plane normal for on-plane
    // tolerance. Use worldPos for fwidth (screen-space derivatives are
    // meaningful in world space), and rotate the design-local plane normal
    // to world space via the transpose of the upper-left 3x3 of worldToLocal.
    vec3 localPlaneNormal = normalize(cCutPlanes[signedDistancePlane].xyz);
    mat3 localToWorldRot = transpose(mat3(worldToLocal));
    vec3 worldPlaneNormal = normalize(localPlaneNormal * localToWorldRot);
    vec3 worldPosDelta = fwidth(worldPos);
    float worldPixelSize = abs(dot(worldPosDelta, worldPlaneNormal));

    // Pixel outside plane
    if (signedDistanceMin < 0) {
        return CUT_PLANE_POS_OUTSIDE;
    }

    // Use estimated world pixel size as on-plane tolerance
    float tolerance = 1.1 * worldPixelSize;
    if (signedDistanceMin < tolerance) {
        return CUT_PLANE_POS_ON_PLANE;
    }

    return CUT_PLANE_POS_INSIDE;
}


// Get the on-plane pixel color for the given color
vec3 GetOnCutPlaneColor(vec3 rgb)
{
    return vec3(1.0);
}


vec4 GetOnCutPlaneColor(vec4 rgba)
{
    return vec4(GetOnCutPlaneColor(rgba.rgb), 1.0);
}

#endif
````

### Pick.glsl

原始位置：`data/Shaders/GLSL/Pick.glsl`。

SHA256：`f577151e64c494923081a7e4c585236b0a4c8ae580c1f52e34eecaab20aeb63a`。

````glsl
// Pick buffer utilities
#ifdef COMPILEPS

// binding point for the atomic counter
#ifndef PICK_BUFFER_COUNTER_BIND
#define PICK_BUFFER_COUNTER_BIND 0
#endif

// binding point for the pick item data
#ifndef PICK_BUFFER_ITEM_DATA_BIND 
#define PICK_BUFFER_ITEM_DATA_BIND 0
#endif

// flag indicating the pick item is not opaque
//
//      see : algro3d_pick.cpp for how these flags are interpreted on the C++ side
//
#define PICK_BUFFER_ITEM_FLAG_TRANSLUCENT           0x01
#define PICK_BUFFER_ITEM_FLAG_NO_DEPTH_TEST         0x02
#define PICK_BUFFER_ITEM_FLAG_REJECT_FRAGMENT       0x04
#define PICK_BUFFER_ITEM_FLAG_ALLOW_REJECT          0x08

// size for the pick radius when determining whether a given
// fragment is part the of the pick (in pixels)
#define PICK_PIXELS_RADIUS 1

// single fragment item recorded in the pick buffer
//      see : algro3d_pick.cpp
struct FragmentPickData
{
    uint flags;
    uint tagMask;
    uint tagValue;
    int designID;
    int objectID;
    int objectSubID1;
    int objectSubID2;
    int objectSubID3;
    float worldPosX;
    float worldPosY;
    float worldPosZ;
};

// max number of items allowed in the pick buffer
uniform int cPickMaxItems;

// the current pick pixel for recording pick items
uniform vec2 cPickFragCoord;

// atomic pick item counter
layout (binding=PICK_BUFFER_COUNTER_BIND) uniform atomic_uint cPickItemCounter;

// pick item data buffer, SSBO buffer used for holding pick results
layout (binding=PICK_BUFFER_ITEM_DATA_BIND, std430) restrict writeonly buffer cPickBufferData
{
    FragmentPickData pickItems[];
};


// Determine if given pixel is relevant for active pick recording
bool IsPickPixel(vec2 pixel)
{
    vec2 delta = abs(pixel - cPickFragCoord);
    return 
        delta.x <= PICK_PIXELS_RADIUS && 
        delta.y <= PICK_PIXELS_RADIUS;    
}


void AddPickResult(vec3 worldPos, vec4 color, int designID, int objectID, ivec3 objectSubIDs, uint flags, uint tagMask, uint tagValue)
{    
    uint index = atomicCounterIncrement(cPickItemCounter);
    if(index < cPickMaxItems) {
        FragmentPickData pick;

        // set initial flags
        pick.flags = flags;

        // set pick tag mask
        pick.tagMask = tagMask;

        // set pick tag value
        pick.tagValue = tagValue;

        // set design identifier
        pick.designID = designID;

        // set pick object identifiers
        pick.objectID = objectID;
        pick.objectSubID1 = objectSubIDs.x;
        pick.objectSubID2 = objectSubIDs.y;
        pick.objectSubID3 = objectSubIDs.z;

        // set world coordinate information
        pick.worldPosX = worldPos.x;
        pick.worldPosY = worldPos.y;
        pick.worldPosZ = worldPos.z;

        // set special data flag indicating item is translucent
        if(color.a < 1.0) {
            pick.flags |= PICK_BUFFER_ITEM_FLAG_TRANSLUCENT;
        }

        // set flag to indicate that the current item does not
        // obey normal depth sorting and so needs to be sorted
        // in front of other depth-tested samples
#if defined(PICK_ITEM_DEPTH_TEST_DISABLED)
        pick.flags |= PICK_BUFFER_ITEM_FLAG_NO_DEPTH_TEST;
#endif

        // write to pick results buffer
        pickItems[index] = pick;
    }
}


void WritePick(vec2 pixel, vec3 worldPos, vec4 color, int designID, int objectID, ivec3 objectSubIDs, uint flags, uint tagMask, uint tagValue)
{
    if(IsPickPixel(pixel)) {
        AddPickResult(worldPos, color, designID, objectID, objectSubIDs, flags, tagMask, tagValue);
    }
}


void WritePick(vec2 pixel, vec3 worldPos, vec4 color, int designID, int objectID, ivec3 objectSubIDs)
{
    WritePick(pixel, worldPos, color, designID, objectID, objectSubIDs, 0, 0, 0);
}

#endif
````

### LineSet.glsl

原始位置：`data/Shaders/GLSL/LineSet.glsl`。

SHA256：`d44e344dc4348bbd336245b2de5af12ced1d38e4246bc49bb260b5c31138db92`。

````glsl
#version 430 

// Implements techniques from the following article:
//      https://wwwtyro.net/2019/11/18/instanced-lines.html
//

// enable early fragment tests
#ifdef COMPILEPS
layout (early_fragment_tests) in;
#endif

/////////////////////////////////////////////////////
// URHO3D SHADER INCLUDES
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "PostProcess.glsl"

/////////////////////////////////////////////////////
// OUR SHADER INCLUDES
#include "Utilities.glsl"

#ifdef PICK
#include "Pick.glsl"
#endif 

//////////////////////////////////////////////////////
// VERTEX OUTPUTS

// Vertex world position
varying vec4 vWorldPos;

// Vertex color
#ifdef VERTEXCOLOR
varying vec4 vColor;
#endif

// Pick identifier
#ifdef PICK
flat varying int vPickID;
#endif


#ifdef COMPILEVS

/////////////////////////////////////////////
// Additional vertex input attributes
attribute vec3 iPos1;
attribute vec3 iPos2;

/////////////////////////////////////////////
// Vertex input attribute semantics

// First position attribute is used as blend weighting between
// the two bounding end points of the line segement
#define inBlendWeights iPos

// Second position attribute is used as local space coordinate 
// of the line segment's first end point
#define inPointA iPos1

// Third position attribute is used as local space coordinate
// of the line segment's second end point
#define inPointB iPos2

// X of texture coordinate is used as the pixel width of the
// line segment
#define inPixelWidth iTexCoord.x

// Add a pixel width adjustment for the pick pass so that the 
// picking is a bit wider than the actual screen size which makes
// it easier to pick the line segment.
#ifdef PICK
#define pixelWidthAdjustment 4.0
#else
#define pixelWidthAdjustment 0.0
#endif

// ADD NEW ATTRIBUTE SEMANTICS AS NECESSARY

/////////////////////////////////////////////
// Vertex Uniforms

// Bring viewport size into vertex shader as we need to be able to 
// determine the world-space size of a pixel during vertex generation
uniform vec2 cGBufferInvSize;

#endif


void VS() 
{
    // get viewport resolution
    vec2 resolution = vec2(1.0 / cGBufferInvSize.x, 1.0 / cGBufferInvSize.y);

    // project endpoints of this segment to clip space
    vec4 clip0 = vec4(inPointA, 1.0) * iModelMatrix * cViewProj;
    vec4 clip1 = vec4(inPointB, 1.0) * iModelMatrix * cViewProj;

    // project clip space endpoints to screen space
    vec2 screen0 = resolution * (0.5 * clip0.xy/clip0.w + 0.5);
    vec2 screen1 = resolution * (0.5 * clip1.xy/clip1.w + 0.5);

    // compute basis vector along line length
    vec2 xBasis = normalize(screen1 - screen0);

    // compute basis vector perpendicular to line
    vec2 yBasis = vec2(-xBasis.y, xBasis.x);

    // screen space width of the line segment
    float pixelWidth = inPixelWidth + pixelWidthAdjustment;

    // offset screen space points by blend weights along basis vectors
    vec2 pt0 = screen0 + pixelWidth * (inBlendWeights.x * xBasis + inBlendWeights.y * yBasis);
    vec2 pt1 = screen1 + pixelWidth * (inBlendWeights.x * xBasis + inBlendWeights.y * yBasis);

    // apply Z weighting
    vec2 pt = mix(pt0, pt1, inBlendWeights.z);
    vec4 clip = mix(clip0, clip1, inBlendWeights.z);
    vec4 position = vec4(clip.w * (2.0 * pt/resolution - 1.0), clip.z, clip.w);

    gl_Position = position;
    
    vWorldPos = (inverse(cViewProj) * position);

#ifdef VERTEXCOLOR
    vColor = iColor;
#endif
    
#ifdef PICK
    vPickID = iObjectIndex;
#endif

}

//////////////////////////////////////////////////////


//////////////////////////////////////////////////////
// FRAGMENT PROGRAM DEFINITIONS
#ifdef COMPILEPS

// Design ID uniform
uniform int cDesignID = 0;

// Design-to-world and world-to-design instance transforms
uniform mat4 cDesignToWorld;
uniform mat4 cWorldToDesign;

// Fragment uniforms
uniform vec4 cSelectionColor;
uniform vec4 cHighlightColor;

#endif

void PS()
{
    // source color for the fragment
    #if defined(USE_SELECTION_COLOR)
        vec4 color = cSelectionColor;
    #elif defined(USE_HIGHLIGHT_COLOR)
        vec4 color = cHighlightColor;
    #else
        #if defined(VERTEXCOLOR)
            vec4 color = vColor;
        #else
            vec4 color = cMatDiffColor;
        #endif
    #endif

    #if !defined(NO_GAMMA)
       color = ToLinearRGBA(color);
    #endif

    #if defined(PICK)
        WritePick(gl_FragCoord.xy, vWorldPos.xyz, color, cDesignID, vPickID, ivec3(0));
    #endif    

    // write color output
    gl_FragData[0] = color;
}
````

### Shading.glsl

原始位置：`data/Shaders/GLSL/Shading.glsl`。

SHA256：`d8a7f977e274678bc6e5b6d7508a15dceb1cbb909d4919b743179f8ac3acefae`。

````glsl
// Lighting/shading model utilities, for simplicity and performance we use a fixed lighting 
// model to save doing any lighting work on the CPU (e.g. assigning batches to lighting
// queues) and GPU (e.g. multiple per-light passes). This is fine (at least for now) since
// lighting positions/details isn't something the user is expected to have any control over
#ifdef COMPILEPS


// Set default shading model if not defined
#if !defined(SHADING_MODEL_PBR) && !defined(SHADING_MODEL_PHONG)
    #define SHADING_MODEL_DEFAULT
#endif

// Set default shading model if requested
#if defined(SHADING_MODEL_DEFAULT)
    #define SHADING_MODEL_PBR
#endif

// include Urho3D PBR implementation if PBR enabled
#ifdef SHADING_MODEL_PBR
#define PBR
#include "BRDF.glsl"
#endif

// Setup constants for lighting
#define SHADING_LIGHT_COLOR                             vec3(0.8)
#define SHADING_AMBIENT_INTENSITY                       0.1
#define SHADING_DIRECTIONAL_INTENSITY                   1.0

// Setup default constants for PBR materials
#define SHADING_PBR_DEFAULT_ROUGHNESS                   0.9
#define SHADING_PBR_DEFAULT_METALLIC                    0.0

// Setup default constatns for Phong materials
#define SHADING_PHONG_DEFAULT_SPECULAR_POWER            8.0
#define SHADING_PHONG_DEFAULT_SPECULAR_STRENGTH         0.02

// Setup constants for lighting modes
#ifdef SHADING_USE_DEBUG_LIGHTS
#define SHADING_NO_FORWARD_LIGHT
#define SHADING_LIGHT_COUNT 6
#else
#define SHADING_USE_FORWARD_LIGHT
#define SHADING_LIGHT_COUNT 1
#endif

// Setup constants for tonemapping
#ifndef SHADING_TONEMAP_WHITEPOINT
#define SHADING_TONEMAP_WHITEPOINT                     1.0
#endif

//////////////////////////////////////////////////////////////////
// Bring view transform into the pixel shader to automatically add 
// forward lighting for all pixels, will automatically add a 
// directional light pointing in the same direction as the
// view (camera) with a ray passing through the given pixel
#if defined(SHADING_USE_FORWARD_LIGHT) && !defined(SHADING_NO_FORWARD_LIGHT)
uniform mat4 cViewInv;
#endif

///////////////////////////////////////////////
// Data for single directional light
struct ShadingDirLight
{
    vec3 dir;
    float intensity;
};

///////////////////////////////////////////////
// Lighting data structure that packages all relevant
// lighting information for the scene for shading operations
struct ShadingLightInput
{   
    // Lighting color 
    vec3 lightColor;
    // Ambient intensity for the lighting
    float ambientIntensity;
     // Directional lights for lighting
    ShadingDirLight lights[SHADING_LIGHT_COUNT];
};

///////////////////////////////////////////////
// Parameters for shading PBR material
struct ShadingPBRMaterialParams
{
    vec4 albedo;
    float roughness;
    float metallic;
};

///////////////////////////////////////////////
// Parameters for shading Phong material
struct ShadingPhongMaterialParams
{
    vec4 diffuse;
    float specularPower;
    float specularStrength;
};


// Set the material params to use for the current shading model
#if defined(SHADING_MODEL_PBR)
#define ShadingMaterialParams ShadingPBRMaterialParams
#elif defined(SHADING_MODEL_PHONG)
#define ShadingMaterialParams ShadingPhongMaterialParams
#else
#error "Invaid or missing shader model definition"
#endif


///////////////////////////////////////////////
// Data structure for describing shading input to 
// a shading mdoel
struct ShadingInput
{
    // world position of camera
    vec3 cameraPos;
    // world position of pixel
    vec3 worldPos;
    // world normal of material
    vec3 worldNormal;
    // lighting environment
    ShadingLightInput lighting;
    // meterial params
    ShadingMaterialParams material;
};


///////////////////////////////////////////////
//Setup the lighting to use for shading
#ifdef SHADING_USE_DEBUG_LIGHTS

void ShadingSetupLighting(inout ShadingInput shadingInput)
{
    // set intensity for major/minor lights
    float majorIntensity = SHADING_DIRECTIONAL_INTENSITY * 0.5;
    float minorIntensity = 0.3 * majorIntensity;

    // set common lighting values
    shadingInput.lighting.lightColor = SHADING_LIGHT_COLOR;
    shadingInput.lighting.ambientIntensity = SHADING_AMBIENT_INTENSITY;

    // add major lights along TOP/BOTTOM view vectors
    shadingInput.lighting.lights[0].dir= vec3( 0, -1, 0);
    shadingInput.lighting.lights[0].intensity =  majorIntensity;

    shadingInput.lighting.lights[1].dir = vec3( 0,  1,  0);
    shadingInput.lighting.lights[1].intensity = majorIntensity;

    // add minor lights along cornder view vectors
    shadingInput.lighting.lights[2].dir = normalize(vec3(-1, -1, -1));
    shadingInput.lighting.lights[2].intensity = minorIntensity;
    
    shadingInput.lighting.lights[3].dir = normalize(vec3( 1,  1,  1));
    shadingInput.lighting.lights[3].intensity = minorIntensity;

    shadingInput.lighting.lights[4].dir = normalize(vec3(-1,  1,  1));
    shadingInput.lighting.lights[4].intensity = minorIntensity;

    shadingInput.lighting.lights[5].dir = normalize(vec3( 1, -1, -1));
    shadingInput.lighting.lights[5].intensity = minorIntensity;
}
#else

mat3 GetForwardLightRotation()
{
    return mat3(
        cViewInv[0][0], cViewInv[0][1], cViewInv[0][2],
        cViewInv[1][0], cViewInv[1][1], cViewInv[1][2],
        cViewInv[2][0], cViewInv[2][1], cViewInv[2][2]
    );
}

vec3 ShadingGetForwardLight(inout ShadingInput shadingInput)
{
    vec3 viewRay = vec3(0, 0, 1.0);
    mat3 lightRot = GetForwardLightRotation();
    vec3 worldRay = normalize(viewRay * lightRot);
    return worldRay;
}

void ShadingSetupLighting(inout ShadingInput shadingInput)
{
    shadingInput.lighting.lightColor = SHADING_LIGHT_COLOR;
    shadingInput.lighting.ambientIntensity = SHADING_AMBIENT_INTENSITY;
    shadingInput.lighting.lights[0].dir = ShadingGetForwardLight(shadingInput);
    shadingInput.lighting.lights[0].intensity = SHADING_DIRECTIONAL_INTENSITY;
}

#endif

///////////////////////////////////////////////
// Compute the Phong model lighting for the given parameters
#ifdef SHADING_MODEL_PHONG

void ShadingSetupMaterial(inout ShadingInput shadingInput, vec4 baseColor)
{
    shadingInput.material.diffuse = baseColor;
    shadingInput.material.specularPower = SHADING_PHONG_DEFAULT_SPECULAR_POWER;
    shadingInput.material.specularStrength = SHADING_PHONG_DEFAULT_SPECULAR_STRENGTH;
}

vec4 Shade(const ShadingInput shadingInput)
{                                         
    vec3 N = normalize(shadingInput.worldNormal);
    vec3 V = normalize(shadingInput.cameraPos - shadingInput.worldPos);

    // initialize color to ambient value
    vec3 color = shadingInput.lighting.ambientIntensity * shadingInput.material.diffuse.rgb;
    for (int i = 0; i < SHADING_LIGHT_COUNT; i++)
    {
        // light and half-light vectors
        vec3 L = normalize(-shadingInput.lighting.lights[i].dir);
        vec3 H = normalize(V + L);

        float NoH = Saturate(dot(N,H));
        float NoL = Saturate(dot(N,L));
        float LoH = Saturate(dot(L,H));

        // radiance for a single light
        vec3 radiance = shadingInput.lighting.lightColor * shadingInput.lighting.lights[i].intensity;

        // diffuse and specular contributions
        vec3 Cd = radiance * shadingInput.material.diffuse.rgb * NoL;
        vec3 Cs = radiance * shadingInput.material.specularStrength * pow(NoH, shadingInput.material.specularPower);

        // combine diffuse and specular
        color += Cd + Cs;  
    }          

    // combine shading with original opacity
    return vec4(color, shadingInput.material.diffuse.a);
}

#endif


///////////////////////////////////////////////
// PBR implementation based on standard model
#ifdef SHADING_MODEL_PBR

void ShadingSetupMaterial(inout ShadingInput shadingInput, vec4 baseColor)
{
    shadingInput.material.albedo = baseColor;
    /// TODO: Take material parameters from the common uniforms
    shadingInput.material.roughness = SHADING_PBR_DEFAULT_ROUGHNESS;
    shadingInput.material.metallic = SHADING_PBR_DEFAULT_METALLIC;
}

vec4 Shade(const ShadingInput shadingInput)
{               
    // convert linear roughness to real roughness value
    float roughness = shadingInput.material.roughness;
    float roughness2 = roughness * roughness;

    // albedo color for material
    vec3 albedo = shadingInput.material.albedo.rgb;
        
    vec3 F0 = mix(vec3(0.04), albedo, shadingInput.material.metallic);
    vec3 N = normalize(shadingInput.worldNormal);
    vec3 V = normalize(shadingInput.cameraPos - shadingInput.worldPos);

    float NoV = clamp(dot(N,V), 1e-5,  1.0);

    // initialize color to ambient
    vec3 color = albedo * shadingInput.lighting.ambientIntensity;

    // accumulate lighting contributions
    for(int i=0; i < SHADING_LIGHT_COUNT; ++i) {
        vec3 L = normalize(-shadingInput.lighting.lights[i].dir);
        vec3 H = normalize(V + L);

        float NoH = Saturate(dot(N,H));
        float NoL = Saturate(dot(N,L));
        float LoH = Saturate(dot(L,H));
        float HoV = Saturate(dot(H,V));

        // diffuse BRDF                
        vec3 Fd = BurleyDiffuse(albedo, roughness, NoV, NoL, LoH);

        // specular BRDF
        float D = Distribution(NoH, roughness2);
        float V = SmithGGXSchlickVisibility(NoL, NoV, roughness2);
        vec3 F = SchlickFresnelCustom(F0, LoH);
        
        vec3 Fr = D * V * F;

        // radiance for single light
        vec3 radiance = shadingInput.lighting.lightColor * shadingInput.lighting.lights[i].intensity;
                
        // add light contribution to total
        color += (Fd + Fr) * radiance * NoL;
    }                
    
    // combine shading albedo transparency
    return vec4(color, shadingInput.material.albedo.a);
}

#endif

// Apply tonemapping to the given color
vec4 ShadingTonemapColor(vec4 color)
{
    return Tonemap(color, SHADING_TONEMAP_WHITEPOINT);
}


// Run the default shading for the given world position normal and base
// color value, using the default lighting data
vec4 DoShading(vec4 color, vec3 worldPos, vec3 worldNormal)
{
    ShadingInput shadingInput;
    shadingInput.worldPos = worldPos;
    shadingInput.worldNormal = worldNormal;
    shadingInput.cameraPos = cCameraPosPS;
    ShadingSetupLighting(shadingInput);
    ShadingSetupMaterial(shadingInput, color);

    vec4 result = Shade(shadingInput);

    // apply tone mapping
#if !defined(SHADING_NO_TONEMAP) && defined(SHADING_TONEMAP)
    result = ShadingTonemapColor(result);
#endif

    return result;
}

// Simple screen door discard helper, given a pixel coordinate
// and block size checks if the pixel should be discarded for 
// the screen door patter
bool ScreendoorShouldDiscard(vec2 pixel, int blockSize)
{    
    int x = int(pixel.x);
    int y = int(pixel.y);
    int xBit = (x / blockSize) % 2;
    int yBit = (y / blockSize) % 2;
    return (xBit + yBit) == 1;
}

#endif
````

### Annotation.glsl

原始位置：`data/Shaders/GLSL/Annotation.glsl`。

SHA256：`d4fede803557e006f3791842a8a77bc24240172fbd83938389e3e219613d5bd9`。

````glsl
#version 430 

// enable early fragment tests
#ifdef COMPILEPS
layout (early_fragment_tests) in;
#endif

/////////////////////////////////////////////////////
// URHO3D SHADER INCLUDES
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "PostProcess.glsl"

/////////////////////////////////////////////////////
// OUR SHADER INCLUDES
#include "Utilities.glsl"

#ifdef PICK
#include "Pick.glsl"
#endif 

//////////////////////////////////////////////////////
// VERTEX ATTRIBUTES

varying vec2 vTexCoord;
varying vec4 vWorldPos;

#ifdef VERTEXCOLOR
    varying vec4 vColor;
#endif

#ifdef PICK
flat varying int vPickID;
#endif


//////////////////////////////////////////////////////
// VERTEX PROGRAM
void VS()
{        
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vTexCoord = GetTexCoord(iTexCoord);
    vWorldPos = vec4(worldPos, GetDepth(gl_Position));

#ifdef PICK
    vPickID = iObjectIndex;
#endif

#ifdef VERTEXCOLOR
    vColor = iColor;
#endif

}
//////////////////////////////////////////////////////


//////////////////////////////////////////////////////
// FRAGMENT PROGRAM DEFINITIONS
#ifdef COMPILEPS

// Design ID uniform
uniform int cDesignID = 0;

// Design-to-world and world-to-design instance transforms
uniform mat4 cDesignToWorld;
uniform mat4 cWorldToDesign;

// Fragment uniforms
uniform vec4 cSelectionColor;
uniform vec4 cHighlightColor;

#endif

//////////////////////////////////////////////////////
// FRAGMENT PROGRAM
void PS()
{       
    // source color for the fragment
    #if defined(USE_SELECTION_COLOR)
        vec4 color = cSelectionColor;
    #elif defined(USE_HIGHLIGHT_COLOR)
        vec4 color = cHighlightColor;
    #else
        #if defined(VERTEXCOLOR)
            vec4 color = vColor;
        #else
            vec4 color = cMatDiffColor;
        #endif
    #endif

    #if defined(DIFFMAP)
        vec4 diffColor = texture2D(sDiffMap, vTexCoord);
        #ifdef ALPHAMASK
            if (diffColor.a < 0.5) {
                discard;
            }
        #endif
        color = diffColor * color;
    #endif
    
    #if !defined(NO_GAMMA)
       color = ToLinearRGBA(color);
    #endif

    #if defined(PICK)
        WritePick(gl_FragCoord.xy, vWorldPos.xyz, color, cDesignID, vPickID, ivec3(0));
    #endif    

    // write render targets
    gl_FragData[0] = color;
}

//////////////////////////////////////////////////////
````

### Background.glsl

原始位置：`data/Shaders/GLSL/Background.glsl`。

SHA256：`4e94b44bafa255deca35a58c7f8547bf56835edf856ea7f27a8a7a31f62eb3ee`。

````glsl
#version 430
// Background shader

// CORE SHADER INCLUDES
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "PostProcess.glsl"

// OUR SHADER INCLUDES
#include "Utilities.glsl"

///////////////////////////////////////////////////////////////

varying vec2 vQuadTexPos;

///////////////////////////////////////////////////////////////
#ifdef COMPILEPS

// gradiant mixing modes for the background color
#define FILL_SOLID              0
#define FILL_H_GRADIANT         1
#define FILL_V_GRADIANT         2
#define FILL_RADIAL_GRADIANT    3

// Main color for background (default=blue-gray)
uniform vec4 cBGColor = vec4(0.6, 0.65, 0.7, 1.0);
// Brightness weight 1 (default=full brightness)
uniform float cBrightness1 = 1.0;
// Brightness weight 2 (default=half brightness)
uniform float cBrightness2 = 0.5;
// Mix mode for the background (gradiant orientation)
uniform int cFillMode = FILL_SOLID;

// Get the gradiant factor for the given 
float ComputeColorScaleFactor(vec2 uv)
{
    // gradiant scaling factor
    float factor = 1.0;

    // no scaling factor for solid fill mode
    if (cFillMode == FILL_SOLID) {
        return 1.0;
    }

    // brightness weights to interpolate between
    float w1 = cBrightness1;
    float w2 = cBrightness2;
        
    // distance from center of rect (on interval [0-1])
    float dx = abs(0.5 - uv.x) * 2;
    float dy = abs(0.5 - uv.y) * 2;
        
    // compute scaling factor for brightness    
    if (cFillMode == FILL_H_GRADIANT) {
        factor = mix(w1, w2, dx*dx);
    } else if (cFillMode == FILL_V_GRADIANT) {
        factor = mix(w1, w2, dy*dy);
    } else if (cFillMode == FILL_RADIAL_GRADIANT) {
        float factorX = mix(w1, w2, dx*dx);
        float factorY = mix(w1, w2, dy*dy);
        factor = mix(factorX, factorY, 0.5);
    }    
    return factor;
}

float ComputeBrightness(vec2 uv, float b)
{    
    // brightness weights
    float w1 = cBrightness1;
    float w2 = cBrightness2;

    float bMin = 0.05;
    float b1 = w1 * b;
    float b2 = 0; 
    if(b1 < bMin) {
        b2 = b1 + bMin;
    }
    
    // distance from center of rect (on interval [0-1])
    float dx = abs(0.5 - uv.x) * 2;
    float dy = abs(0.5 - uv.y) * 2;

    // gradiant scaling factor
    float factor = 1.0;

    // compute scaling factor for brightness    
    if (cFillMode == FILL_H_GRADIANT) {
        factor = mix(b1, w2, dx*dx);
    } else if (cFillMode == FILL_V_GRADIANT) {
        factor = mix(w1, w2, dy*dy);
    } else if (cFillMode == FILL_RADIAL_GRADIANT) {
        float factorX = mix(w1, w2, dx*dx);
        float factorY = mix(w1, w2, dy*dy);
        factor = mix(factorX, factorY, 0.5);
    }

    float brightness = mix(b2, b1, factor);
    vec3 dithered = Dither(vec3(brightness));
    brightness = (dithered.x + dithered.y + dithered.z) / 3;
    return brightness;
}


vec4 GetBackgroundColor(vec2 uv)
{   
    // Background color is assumed to be in sRGB, convert to linear RGB
    vec3 rgb = ToGamma(cBGColor.rgb);
    if (cFillMode != FILL_SOLID) {
        vec3 hsv = RgbToHsv(rgb);
        hsv.z = ComputeBrightness(uv, hsv.z);
        rgb = HsvToRgb(hsv);
    }
    return vec4(rgb, cBGColor.a);
}

#endif

///////////////////////////////////////////////////////////////

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);    
    vec4 clipPos = GetClipPos(worldPos);        
    vQuadTexPos = GetQuadTexCoord(clipPos);
    gl_Position = clipPos;
}


void PS()
{
    gl_FragColor = GetBackgroundColor(vQuadTexPos);
}
````

### Utilities.glsl

原始位置：`data/Shaders/GLSL/Utilities.glsl`。

SHA256：`a514bae18bc3f494eeda3354ca2661654d1102921701d67da04d7a7873a90236`。

````glsl

// Convert an RGB triplet to HSV space
vec3 RgbToHsv(vec3 c)
{
    vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));

    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

// Convert an HSV triplet to RGB space
vec3 HsvToRgb(vec3 c)
{
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

// Encode a pick ID value to an RGBA pixel value, allows integer pick identifiers
// to be written to the pick buffer render target as color values
vec4 UIntToVec4(uint bits)
{    
    uint b1 = bits         & 0xFFu;
    uint b2 = (bits >>  8) & 0xFFu;
    uint b3 = (bits >> 16) & 0xFFu;
    uint b4 = (bits >> 24) & 0xFFu;           
    return vec4(float(b1), float(b2), float(b3), float(b4)) / 255.0;
}

// Clamp value to [0,1]] range
float Saturate(float value)
{
    return clamp(value, 0.0, 1.0);
}

// Dim the given RGB color by the given amount 
//  rgb - RGB color
//
//  brightness - on interval [0,1] will scale the brightness of the color by the given amout, e.g.
//      a value of 0.9 is 90% brightness
//
//  saturation - on interval [0,1], will scale the saturation of the color by the given amount, e.g. 
//      a value of 0.9 is 90% saturation
//
vec3 DimColorRgb(vec3 rgb, float brightness, float saturation)
{
    brightness = clamp(brightness, 0.0, 1.0);
    saturation = clamp(saturation, 0.0, 1.0);
    vec3 hsv = RgbToHsv(rgb);
    hsv.y *= saturation;
    hsv.z *= brightness; 
    return HsvToRgb(hsv);
}

// Alpha blend "src" over "dst"
vec4 AlphaBlend(vec4 src, vec4 dst)
{
    float alpha = src.a + dst.a * (1.0 - src.a);
    vec3 rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / alpha;
    return vec4(rgb, alpha);
}


#ifdef COMPILEPS

// Gamma convert RGBA value
vec4 ToLinearRGBA(vec4 color)
{
    return vec4(ToGamma(color.rgb), color.a);
}


// perform tonemapping for the given color
vec4 Tonemap(vec4 rgba, float whitepoint)
{
    return vec4(ReinhardEq4Tonemap(rgba.rgb, whitepoint), rgba.a);
}

// Jimenez 2014, "Next Generation Post-Processing in Call of Duty"
float InterleavedGradientNoise(vec2 n) 
{
    float f = 0.06711056 * n.x + 0.00583715 * n.y;
    return fract(52.9829189 * fract(f));
}

// Default granularity for dithering
#define DITHER_DEFAULT_GRANULARITY  (1.0/255.0)

// Dithering utilities to correct for banding 
vec3 Dither(vec3 rgb, vec2 pixel, float granularity)
{
    float noise = InterleavedGradientNoise(pixel);
    return rgb + mix(-granularity, granularity, noise);
}

vec3 Dither(vec3 rgb, float granularity)
{
    return Dither(rgb, gl_FragCoord.xy, granularity);
}

vec3 Dither(vec3 rgb, vec2 pixel)
{    
    return Dither(rgb, pixel, DITHER_DEFAULT_GRANULARITY);
}

vec3 Dither(vec3 rgb)
{
    return Dither(rgb, gl_FragCoord.xy);
}


#endif
````

## 附录 B 渲染 Pass 配置原文

### RenderPaths/Default.xml

原始位置：`data/RenderPaths/Default.xml`。

SHA256：`c149751a25b1916f558df81205160e9ae06ca2a7c0643d7f38edf1acbc3b6f5a`。

````xml
<renderpath>

    <!-- //////////////////////////////////////// -->
    <!-- RENDER TARGET DEFINITIONS --> 

    <!--
        Target for writing scene color values
    -->
    <rendertarget name="color" sizedivisor="1 1" format="rgba" />

    <!-- 
        Target for writing/testing scene depths
    -->
    <rendertarget name="depth" sizedivisor="1 1" format="d32s8" />

    <!--
        Target for writing BOIT color accumulation values (see: BOIT.glsl)
    -->
    <rendertarget name="accum" sizedivisor="1 1" format="rgba16f" />

    <!--
        Target for writing BOIT revealage (opacity) values (see: BOIT.glsl)
    -->
    <rendertarget name="reveal" sizedivisor="1 1" format="r8" />

    <!--
        Target for writing overlay color
    -->
    <rendertarget name="overlay" sizedivisor="1 1" format="rgba" />


    <!-- //////////////////////////////////////// -->
    <!-- SETUP COMMANDS --> 

    <!-- Clear main color output and depth buffers-->
    <command type="clear" color="0 0 0 0" output="color" depth="0.0" stencil="0" depthstencil="depth"/>
           
    <!-- //////////////////////////////////////// -->
    <!-- BACKGROUND -->    
    <command tag="background" type="quad" vs="Background" ps="Background" blend="replace" output="color"/>    
        
    <!-- //////////////////////////////////////// -->
    <!-- PICK PASSES -->
    <!-- Do pick pass(es) for the scene, disabled by default, enabled as needed by the pick system -->
    <command tag="pick" enabled="false" type="sendevent" name="PickSystem:PickStart" />
    <command tag="pick" enabled="false" type="scenepass" pass="pick" output="color" depthstencil="depth"/>
    <command tag="pick" enabled="false" type="sendevent" name="RGRender:Pick" output="color" depthstencil="depth"/>    
    <command tag="pick" enabled="false" type="sendevent" name="PickSystem:PickFinish" />

    <!-- //////////////////////////////////////// -->
    <!-- BASE PASSES -->    

    <!-- Render standard geometry for the scene -->
    <command type="scenepass" pass="base" metadata="base" output="color" depthstencil="depth"/>

    <!-- Do RGRenderer base pass for the scene-->
    <command type="sendevent" name="RGRender:Base" output="color" depthstencil="depth"  />    

    <!-- //////////////////////////////////////// -->
    <!-- TRANSPARENCY PASSES -->
    <!-- Do RGRender pass of translucent objects for the scene -->
    <command type="clear" color="0 0 0 0" output="accum"/>
    <command type="clear" color="1 1 1 1" output="reveal"/>
    <command type="sendevent" name="RGRender:Alpha" depthstencil="depth">
        <output index="0" name="accum"/>
        <output index="1" name="reveal"/>
    </command>
    <command type="scenepass" pass="boit-accum" output="accum" depthstencil="depth" />
    <command type="scenepass" pass="boit-reveal" output="reveal" depthstencil="depth" />


    <command type="quad" vs="BOITComposite" ps="BOITComposite" blend="alpha" output="color">
        <texture unit="0" name="accum"/>
        <texture unit="1" name="reveal"/>
    </command>        

    <!-- //////////////////////////////////////// -->
    <!-- ANNOATION PASS -->
    <command type="scenepass" pass="annotation" output="color" depthstencil="depth" />

    <!-- //////////////////////////////////////// -->
    <!-- OVERLAY PASSES --> 
    
    <!-- Selection passes -->
    <command type="clear" color="0 0 0 0" output="overlay"/>
    <command type="scenepass" pass="selection" depthstencil="depth" output="overlay"/>
    <command type="sendevent" name="RGRender:Selection" depthstencil="depth" output="overlay" />
    <command type="scenepass" pass="annotation-selection" depthstencil="depth" output="overlay"/>
    <command type="quad" vs="CopyFramebuffer" ps="CopyFramebuffer" blend="alpha" output="color">
        <texture unit="diff" name="overlay"/>
    </command> 

    <!-- Highlight passes -->
    <command type="clear" color="0 0 0 0" output="overlay"/>
    <command type="scenepass" pass="highlight" depthstencil="depth" output="overlay"/>
    <command type="sendevent" name="RGRender:Highlight" depthstencil="depth" output="overlay" />
    <command type="scenepass" pass="annotation-highlight" depthstencil="depth" output="overlay"/>
    <command type="quad" vs="CopyFramebuffer" ps="CopyFramebuffer" blend="alpha" output="color">
        <texture unit="diff" name="overlay"/>
    </command> 

    <!-- Widget overlay passes -->
    <command type="clear" color="0 0 0 0" output="overlay" depth="0.0" stencil="0" depthstencil="depth"/>
    <command type="scenepass" pass="widgets" depthstencil="depth" blend="alpha" output="color" />

    <!-- Debug geometry pass -->
    <command type="debug" depthstencil="depth" blend="replace" output="color" />

    <!-- //////////////////////////////////////// -->
    <!-- POSTPROCESSING -->            
  
    <!-- Do gamma correction for color buffer -->
    <command tag="gamma" type="quad" vs="GammaCorrection" ps="GammaCorrection" output="viewport">
        <texture unit="diffuse" name="color" />
    </command>    

    <!-- FXAA -->
    <command tag="FXAA3" type="quad" vs="FXAA3" ps="FXAA3" psdefines="FXAA_QUALITY_PRESET=12" output="viewport">
        <texture unit="diffuse" name="viewport" />
    </command>


</renderpath>
````

### Techniques/Annotation.xml

原始位置：`data/Techniques/Annotation.xml`。

SHA256：`3b0fa1c7a6c940a70d248275626e2b0f8aaa56636f05632bf4ec3b5f53ad1212`。

````xml
<technique vs="Annotation" ps="Annotation" >

    <!-- Pass for supporting picking of annotation geometry -->
    <pass name="pick" depthwrite="false" colorwrite="false"
        vsdefines="PICK"
        psdefines="PICK NO_SHADING" />

    <!-- Pass for rendering base annotation geometry -->
    <pass name="annotation" />

    <!-- Pass for rendering selected annotation geometry -->
    <pass name="annotation-selection" depthwrite="false" depthtest="always" 
        psdefines="USE_SELECTION_COLOR ALPHAMASK" />

    <!-- Pass for rendering highlighted annotation geometry -->
    <pass name="annotation-highlight" depthwrite="false" depthtest="always"
        psdefines="USE_HIGHLIGHT_COLOR ALPHAMASK" />

    <!-- Pass for annotation objects in the widgets pass-->
    <pass name="widgets" />

</technique>
````

### Techniques/AnnotationLineSet.xml

原始位置：`data/Techniques/AnnotationLineSet.xml`。

SHA256：`d1ab416fe8eed3719ab5e9ee0b6c88768953cc7be34c23035dbefe0be01dce54`。

````xml
<technique vs="LineSet" ps="LineSet" >

    <!-- Pass for supporting picking of standard geometry -->
    <pass name="pick" cull="none" depthwrite="false" colorwrite="false"
        vsdefines="PICK" 
        psdefines="PICK NO_SHADING" />

    <!-- Pass for rendering base annotation geometry -->
    <pass name="annotation" cull="none"/>

    <!-- Pass for rendering selected annotation geometry -->
    <pass name="annotation-selection" cull="none" depthwrite="false" depthtest="always"
        psdefines="USE_SELECTION_COLOR" />

    <!-- Pass for rendering highlighted annotation geometry -->
    <pass name="annotation-highlight" cull="none" depthwrite="false" depthtest="always"
        psdefines="USE_HIGHLIGHT_COLOR" />

    <!-- Pass for annotation objects in the widgets pass-->
    <pass name="widgets" cull="none" />

</technique>
````

### Techniques/AnnotationText.xml

原始位置：`data/Techniques/AnnotationText.xml`。

SHA256：`124b0c22e2073811c5a1713a40515cb87b05843263671880aa25da0dc8f8f8b6`。

````xml
<technique vs="Text" ps="Text" psdefines="ALPHAMAP" >

    <!-- Pass for supporting picking of standard geometry (CURRENTLY NOT SUPPORTED)
    <pass name="pick" depthwrite="false" colorwrite="false"
        vsdefines="PICK ALPHAMAP" 
        psdefines="PICK ALPHAMAP" />
     -->

    <!-- Pass for rendering base annotation geometry -->
    <pass name="annotation" blend="alpha"/>

    <!-- Pass for rendering selected annotation geometry -->
    <pass name="annotation-selection" depthwrite="false" depthtest="always"
        psdefines="USE_SELECTION_COLOR" />

    <!-- Pass for rendering highlighted annotation geometry -->
    <pass name="annotation-highlight" depthwrite="false" depthtest="always"
        psdefines="USE_HIGHLIGHT_COLOR" />

    <!-- Pass for annotation objects in the widgets pass-->
    <pass name="widgets" depthtest="false" depthwrite="false" blend="alpha" />

</technique>
````

### Techniques/StandardMaterial.xml

原始位置：`data/Techniques/StandardMaterial.xml`。

SHA256：`ff0f836499022aaa74f92f84b374246afc17a130fc18408259ca4180a58bb7a0`。

````xml
<technique vs="StandardMaterial" ps="StandardMaterial" >

    <!-- Pass for supporting picking of standard geometry -->
    <pass name="pick" depthwrite="false" colorwrite="false"
        vsdefines="PICK" 
        psdefines="PICK NO_SHADING" />    

    <!-- Pass for rendering opaque standard geometry -->
    <pass name="base" />    

    <!-- Pass for rendering BOIT accumulation values -->
    <pass name="boit-accum" depthwrite="false" blend="boit-accum"
        psdefines="BOIT_ACCUM"/>

    <!-- Pass for rendering BOIT revealage values-->
    <pass name="boit-reveal" depthwrite="false" blend="boit-reveal"
        psdefines="BOIT_REVEAL"/>
    
    <!-- Pass for rendering selected standard geometry -->
    <pass name="selection" depthwrite="false" depthtest="always" 
        psdefines="NO_SHADING USE_SELECTION_COLOR" />

    <!-- Pass for rendering highlighted standard geometry -->
    <pass name="highlight" depthwrite="false" depthtest="always"
        psdefines="NO_SHADING USE_HIGHLIGHT_COLOR" />

    <!-- Pass for rendering widget geometries -->
    <pass name="widgets" />

</technique>
````

## 附录 C 依赖及选定后处理 Shader 原文

这里补齐附录 A 的递归 include，并加入默认管线引用的文字、Gamma、拷贝和 FXAA 代码。包含这些文件不等于 PCB 工具启用了其中每个条件编译分支。

### BRDF.glsl

原始位置：`engine/Shaders/GLSL/BRDF.glsl`。

SHA256：`34b664288ed421113cd54f6b8086f959f1b74cc22f90217eea39f57156c1cdc1`。

````glsl
#line 20001
#ifdef COMPILEPS
  #ifdef PBR
    // Following BRDF methods are based upon research Frostbite EA
    //[Lagrade et al. 2014, "Moving Frostbite to Physically Based Rendering"]
    
    //Schlick Fresnel
    //specular  = the rgb specular color value of the pixel
    //VdotH     = the dot product of the camera view direction and the half vector 
    vec3 SchlickFresnel(vec3 specular, float VdotH)
    {
        return specular + (vec3(1.0, 1.0, 1.0) - specular) * pow(1.0 - VdotH, 5.0);
    }

    //Schlick Gaussian Fresnel
    //specular  = the rgb specular color value of the pixel
    //VdotH     = the dot product of the camera view direction and the half vector 
    vec3 SchlickGaussianFresnel(in vec3 specular, in float VdotH)
    {
        float sphericalGaussian = pow(2.0, (-5.55473 * VdotH - 6.98316) * VdotH);
        return specular + (vec3(1.0, 1.0, 1.0) - specular) * sphericalGaussian;
    }

    vec3 SchlickFresnelCustom(vec3 specular, float LdotH)
    {
        float ior = 0.25;
        float airIor = 1.000277;
        float f0 = (ior - airIor) / (ior + airIor);
        float max_ior = 2.5;
        f0 = clamp(f0 * f0, 0.0, (max_ior - airIor) / (max_ior + airIor));
        return specular * (f0   + (1 - f0) * pow(2, (-5.55473 * LdotH - 6.98316) * LdotH));
    }

    //Get Fresnel
    //specular  = the rgb specular color value of the pixel
    //VdotH     = the dot product of the camera view direction and the half vector 
    vec3 Fresnel(vec3 specular, float VdotH, float LdotH)
    {
        return SchlickFresnelCustom(specular, LdotH);
        //return SchlickFresnel(specular, VdotH);
    }

    // Smith GGX corrected Visibility
    // NdotL        = the dot product of the normal and direction to the light
    // NdotV        = the dot product of the normal and the camera view direction
    // roughness    = the roughness of the pixel
    float SmithGGXSchlickVisibility(float NdotL, float NdotV, float roughness)
    {
        float rough2 = roughness * roughness;
        float lambdaV = NdotL  * sqrt((-NdotV * rough2 + NdotV) * NdotV + rough2);   
        float lambdaL = NdotV  * sqrt((-NdotL * rough2 + NdotL) * NdotL + rough2);
    
        return 0.5 / (lambdaV + lambdaL);
    }

    float NeumannVisibility(float NdotV, float NdotL) 
    {
        return NdotL * NdotV / max(1e-7, max(NdotL, NdotV));
    }

    // Get Visibility
    // NdotL        = the dot product of the normal and direction to the light
    // NdotV        = the dot product of the normal and the camera view direction
    // roughness    = the roughness of the pixel
    float Visibility(float NdotL, float NdotV, float roughness)
    {
        return NeumannVisibility(NdotV, NdotL);
        //return SmithGGXSchlickVisibility(NdotL, NdotV, roughness);
    }

    // Blinn Distribution
    // NdotH        = the dot product of the normal and the half vector
    // roughness    = the roughness of the pixel
    float BlinnPhongDistribution(in float NdotH, in float roughness)
    {
        float specPower = max((2.0 / (roughness * roughness)) - 2.0, 1e-4); // Calculate specular power from roughness
        return pow(clamp(NdotH, 0.0, 1.0), specPower);
    }

    // Beckmann Distribution
    // NdotH        = the dot product of the normal and the half vector
    // roughness    = the roughness of the pixel
    float BeckmannDistribution(in float NdotH, in float roughness)
    {
        float rough2 = roughness * roughness;
        float roughnessA = 1.0 / (4.0 * rough2 * pow(NdotH, 4.0));
        float roughnessB = NdotH * NdotH - 1.0;
        float roughnessC = rough2 * NdotH * NdotH;
        return roughnessA * exp(roughnessB / roughnessC);
    }

    // GGX Distribution
    // NdotH        = the dot product of the normal and the half vector
    // roughness    = the roughness of the pixel
    float GGXDistribution(float NdotH, float roughness)
    {
        float rough2 = roughness * roughness;
        float tmp =  (NdotH * rough2 - NdotH) * NdotH + 1.0;
        return rough2 / (tmp * tmp);
    }

    // Get Distribution
    // NdotH        = the dot product of the normal and the half vector
    // roughness    = the roughness of the pixel
    float Distribution(float NdotH, float roughness)
    {
        return GGXDistribution(NdotH, roughness);
    }

    // Lambertian Diffuse
    // diffuseColor = the rgb color value of the pixel
    // roughness    = the roughness of the pixel
    // NdotV        = the normal dot with the camera view direction
    // NdotL        = the normal dot with the light direction
    // VdotH        = the camera view direction dot with the half vector
    vec3 LambertianDiffuse(vec3 diffuseColor)
    {
        return diffuseColor * (1.0 / M_PI) ;
    }

    // Custom Lambertian Diffuse
    // diffuseColor = the rgb color value of the pixel
    // roughness    = the roughness of the pixel
    // NdotV        = the normal dot with the camera view direction
    // NdotL        = the normal dot with the light direction
    // VdotH        = the camera view direction dot with the half vector
    vec3 CustomLambertianDiffuse(vec3 diffuseColor, float NdotV, float roughness)
    {
        return diffuseColor * (1.0 / M_PI) * pow(NdotV, 0.5 + 0.3 * roughness);
    }

    // Burley Diffuse
    // diffuseColor = the rgb color value of the pixel
    // roughness    = the roughness of the pixel
    // NdotV        = the normal dot with the camera view direction
    // NdotL        = the normal dot with the light direction
    // VdotH        = the camera view direction dot with the half vector
    vec3 BurleyDiffuse(vec3 diffuseColor, float roughness, float NdotV, float NdotL, float VdotH)
    {
        float energyBias = mix(roughness, 0.0, 0.5);
        float energyFactor = mix(roughness, 1.0, 1.0 / 1.51);
        float fd90 = energyBias + 2.0 * VdotH * VdotH * roughness;
        float f0 = 1.0;
        float lightScatter = f0 + (fd90 - f0) * pow(1.0 - NdotL, 5.0);
        float viewScatter = f0 + (fd90 - f0) * pow(1.0 - NdotV, 5.0);

        return diffuseColor * lightScatter * viewScatter * energyFactor;
    }

    //Get Diffuse
    // diffuseColor = the rgb color value of the pixel
    // roughness    = the roughness of the pixel
    // NdotV        = the normal dot with the camera view direction
    // NdotL        = the normal dot with the light direction
    // VdotH        = the camera view direction dot with the half vector
    vec3 Diffuse(vec3 diffuseColor, float roughness, float NdotV, float NdotL, float VdotH)
    {
        //return LambertianDiffuse(diffuseColor);
        return CustomLambertianDiffuse(diffuseColor, NdotV, roughness);
        //return BurleyDiffuse(diffuseColor, roughness, NdotV, NdotL, VdotH);
    }

  #endif
#endif
````

### Constants.glsl

原始位置：`engine/Shaders/GLSL/Constants.glsl`。

SHA256：`13c3abb0e2b84adfe3916759ab18177c64c3742ec23c2e592e19c4d705c32f23`。

````glsl
#define M_PI 3.14159265358979323846
#define M_EPSILON 0.0001

#ifdef PBR
#define ROUGHNESS_FLOOR 0.004
#define METALNESS_FLOOR 0.03
#endif
````

### CopyFramebuffer.glsl

原始位置：`engine/Shaders/GLSL/CopyFramebuffer.glsl`。

SHA256：`818a088ebc929c8a5d1471e7c931db19ac987af5321d60ccb62748c29fef2f10`。

````glsl
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"

varying vec2 vScreenPos;

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vScreenPos = GetScreenPosPreDiv(gl_Position);
}

void PS()
{
    gl_FragColor = texture2D(sDiffMap, vScreenPos);
}
````

### FXAA3.glsl

原始位置：`engine/Shaders/GLSL/FXAA3.glsl`。

SHA256：`596feca5a57a3cb5294aa12eac2c77dc1ed72800b99e08a3a6a0647324f68941`。

````glsl
//----------------------------------------------------------------------------------
//
// Copyright (c) 2014, NVIDIA CORPORATION. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
//  * Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
//  * Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//  * Neither the name of NVIDIA CORPORATION nor the names of its
//    contributors may be used to endorse or promote products derived
//    from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS ``AS IS'' AND ANY
// EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT OWNER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY
// OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//
//----------------------------------------------------------------------------------
/*============================================================================


                    NVIDIA FXAA 3.11 by TIMOTHY LOTTES

------------------------------------------------------------------------------

                           Modified for Urho3D

============================================================================*/

/*==========================================================================*/
//
//                      Urho3D specific preparations
//
/*--------------------------------------------------------------------------*/

#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"

varying vec2 vScreenPos;

/*==========================================================================*/
//
//                      Shader Requirements
//
/*--------------------------------------------------------------------------*/

#define FXAA_FAST_PIXEL_OFFSET 0

/*--------------------------------------------------------------------------*/

#if (FXAA_FAST_PIXEL_OFFSET == 1)
    #define Fxaa_vec2 ivec2
#else
    #define Fxaa_vec2 vec2
#endif

/*============================================================================
                        FXAA QUALITY - TUNING KNOBS
------------------------------------------------------------------------------
NOTE the other tuning knobs are now in the shader function inputs!
============================================================================*/
#ifndef FXAA_QUALITY_PRESET
    //
    // Choose the quality preset.
    // This needs to be compiled into the shader as it effects code.
    // Best option to include multiple presets is to 
    // in each shader define the preset, then include this file.
    //
    // OPTIONS
    // -----------------------------------------------------------------------
    // 10 to 15 - default medium dither (10=fastest, 15=highest quality)
    // 20 to 29 - less dither, more expensive (20=fastest, 29=highest quality)
    // 39       - no dither, very expensive 
    //
    // NOTES
    // -----------------------------------------------------------------------
    // 12 = slightly faster then FXAA 3.9 and higher edge quality (default)
    // 13 = about same speed as FXAA 3.9 and better than 12
    // 23 = closest to FXAA 3.9 visually and performance wise
    //  _ = the lowest digit is directly related to performance
    // _  = the highest digit is directly related to style
    // 
    #define FXAA_QUALITY_PRESET 12
#endif


/*============================================================================

                           FXAA QUALITY - PRESETS

============================================================================*/

/*============================================================================
                     FXAA QUALITY - MEDIUM DITHER PRESETS
============================================================================*/
#if (FXAA_QUALITY_PRESET == 10)
    #define FXAA_QUALITY_PS 3
    #define FXAA_QUALITY_P0 1.5
    #define FXAA_QUALITY_P1 3.0
    #define FXAA_QUALITY_P2 12.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 11)
    #define FXAA_QUALITY_PS 4
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 3.0
    #define FXAA_QUALITY_P3 12.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 12)
    #define FXAA_QUALITY_PS 5
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 4.0
    #define FXAA_QUALITY_P4 12.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 13)
    #define FXAA_QUALITY_PS 6
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 4.0
    #define FXAA_QUALITY_P5 12.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 14)
    #define FXAA_QUALITY_PS 7
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 4.0
    #define FXAA_QUALITY_P6 12.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 15)
    #define FXAA_QUALITY_PS 8
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 4.0
    #define FXAA_QUALITY_P7 12.0
#endif

/*============================================================================
                     FXAA QUALITY - LOW DITHER PRESETS
============================================================================*/
#if (FXAA_QUALITY_PRESET == 20)
    #define FXAA_QUALITY_PS 3
    #define FXAA_QUALITY_P0 1.5
    #define FXAA_QUALITY_P1 2.0
    #define FXAA_QUALITY_P2 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 21)
    #define FXAA_QUALITY_PS 4
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 22)
    #define FXAA_QUALITY_PS 5
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 23)
    #define FXAA_QUALITY_PS 6
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 24)
    #define FXAA_QUALITY_PS 7
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 3.0
    #define FXAA_QUALITY_P6 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 25)
    #define FXAA_QUALITY_PS 8
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 4.0
    #define FXAA_QUALITY_P7 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 26)
    #define FXAA_QUALITY_PS 9
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 2.0
    #define FXAA_QUALITY_P7 4.0
    #define FXAA_QUALITY_P8 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 27)
    #define FXAA_QUALITY_PS 10
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 2.0
    #define FXAA_QUALITY_P7 2.0
    #define FXAA_QUALITY_P8 4.0
    #define FXAA_QUALITY_P9 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 28)
    #define FXAA_QUALITY_PS 11
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 2.0
    #define FXAA_QUALITY_P7 2.0
    #define FXAA_QUALITY_P8 2.0
    #define FXAA_QUALITY_P9 4.0
    #define FXAA_QUALITY_P10 8.0
#endif
/*--------------------------------------------------------------------------*/
#if (FXAA_QUALITY_PRESET == 29)
    #define FXAA_QUALITY_PS 12
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.5
    #define FXAA_QUALITY_P2 2.0
    #define FXAA_QUALITY_P3 2.0
    #define FXAA_QUALITY_P4 2.0
    #define FXAA_QUALITY_P5 2.0
    #define FXAA_QUALITY_P6 2.0
    #define FXAA_QUALITY_P7 2.0
    #define FXAA_QUALITY_P8 2.0
    #define FXAA_QUALITY_P9 2.0
    #define FXAA_QUALITY_P10 4.0
    #define FXAA_QUALITY_P11 8.0
#endif

/*============================================================================
                     FXAA QUALITY - EXTREME QUALITY
============================================================================*/
#if (FXAA_QUALITY_PRESET == 39)
    #define FXAA_QUALITY_PS 12
    #define FXAA_QUALITY_P0 1.0
    #define FXAA_QUALITY_P1 1.0
    #define FXAA_QUALITY_P2 1.0
    #define FXAA_QUALITY_P3 1.0
    #define FXAA_QUALITY_P4 1.0
    #define FXAA_QUALITY_P5 1.5
    #define FXAA_QUALITY_P6 2.0
    #define FXAA_QUALITY_P7 2.0
    #define FXAA_QUALITY_P8 2.0
    #define FXAA_QUALITY_P9 2.0
    #define FXAA_QUALITY_P10 4.0
    #define FXAA_QUALITY_P11 8.0
#endif

/*============================================================================

                            Support Functions

============================================================================*/

float CalcLuma(vec3 rgb)
{
    vec3 luma = vec3(0.299, 0.587, 0.114);
    return dot(rgb, luma);
}

/*--------------------------------------------------------------------------*/

#ifdef GL3

#define FxaaTexTop(t, p) vec4(textureLod(t, p, 0.0).rgb, 1.0)
#define LumaTop(t, p) CalcLuma(textureLod(t, p, 0.0).rgb)
#if (FXAA_FAST_PIXEL_OFFSET == 1)
    #define LumaOff(t, p, o, r) CalcLuma(textureLodOffset(t, p, 0.0, o).rgb)
#else
    #define LumaOff(t, p, o, r) CalcLuma(textureLod(t, p + (o * r), 0.0).rgb)
#endif

#else

#define FxaaTexTop(t, p) vec4(texture2DLod(t, p, 0.0).rgb, 1.0)
#define LumaTop(t, p) CalcLuma(texture2DLod(t, p, 0.0).rgb)
#if (FXAA_FAST_PIXEL_OFFSET == 1)
    #define LumaOff(t, p, o, r) CalcLuma(texture2DLodOffset(t, p, 0.0, o).rgb)
#else
    #define LumaOff(t, p, o, r) CalcLuma(texture2DLod(t, p + (o * r), 0.0).rgb)
#endif

#endif

/*============================================================================

                             FXAA3 QUALITY - PC

============================================================================*/
vec4 FxaaPixelShader(
    //
    // Use noperspective interpolation here (turn off perspective interpolation).
    // {xy} = center of pixel
    vec2 pos,
    //
    // Input color texture.
    // {rgb_} = color in linear or perceptual color space
    // if (FXAA_GREEN_AS_LUMA == 0)
    //     {__a} = luma in perceptual color space (not linear)
    sampler2D tex,
    //
    // Only used on FXAA Quality.
    // This must be from a constant/uniform.
    // {x_} = 1.0/screenWidthInPixels
    // {_y} = 1.0/screenHeightInPixels
    vec2 fxaaQualityRcpFrame,
    //
    // Only used on FXAA Quality.
    // This used to be the FXAA_QUALITY_SUBPIX define.
    // It is here now to allow easier tuning.
    // Choose the amount of sub-pixel aliasing removal.
    // This can effect sharpness.
    //   1.00 - upper limit (softer)
    //   0.75 - default amount of filtering
    //   0.50 - lower limit (sharper, less sub-pixel aliasing removal)
    //   0.25 - almost off
    //   0.00 - completely off
    float fxaaQualitySubpix,
    //
    // Only used on FXAA Quality.
    // This used to be the FXAA_QUALITY_EDGE_THRESHOLD define.
    // It is here now to allow easier tuning.
    // The minimum amount of local contrast required to apply algorithm.
    //   0.333 - too little (faster)
    //   0.250 - low quality
    //   0.166 - default
    //   0.125 - high quality 
    //   0.063 - overkill (slower)
    float fxaaQualityEdgeThreshold,
    //
    // Only used on FXAA Quality.
    // This used to be the FXAA_QUALITY_EDGE_THRESHOLD_MIN define.
    // It is here now to allow easier tuning.
    // Trims the algorithm from processing darks.
    //   0.0833 - upper limit (default, the start of visible unfiltered edges)
    //   0.0625 - high quality (faster)
    //   0.0312 - visible limit (slower)
    // Special notes when using FXAA_GREEN_AS_LUMA,
    //   Likely want to set this to zero.
    //   As colors that are mostly not-green
    //   will appear very dark in the green channel!
    //   Tune by looking at mostly non-green content,
    //   then start at zero and increase until aliasing is a problem.
    float fxaaQualityEdgeThresholdMin
) {
/*--------------------------------------------------------------------------*/
    vec2 posM;
    posM.x = pos.x;
    posM.y = pos.y;
    
    vec4 rgbyM = FxaaTexTop(tex, posM);
    rgbyM.y = CalcLuma(rgbyM.rgb);
    #define lumaM rgbyM.y
    float lumaS = LumaOff(tex, posM, Fxaa_vec2( 0, 1), fxaaQualityRcpFrame.xy);
    float lumaE = LumaOff(tex, posM, Fxaa_vec2( 1, 0), fxaaQualityRcpFrame.xy);
    float lumaN = LumaOff(tex, posM, Fxaa_vec2( 0,-1), fxaaQualityRcpFrame.xy);
    float lumaW = LumaOff(tex, posM, Fxaa_vec2(-1, 0), fxaaQualityRcpFrame.xy);
/*--------------------------------------------------------------------------*/
    float maxSM = max(lumaS, lumaM);
    float minSM = min(lumaS, lumaM);
    float maxESM = max(lumaE, maxSM);
    float minESM = min(lumaE, minSM);
    float maxWN = max(lumaN, lumaW);
    float minWN = min(lumaN, lumaW);
    float rangeMax = max(maxWN, maxESM);
    float rangeMin = min(minWN, minESM);
    float rangeMaxScaled = rangeMax * fxaaQualityEdgeThreshold;
    float range = rangeMax - rangeMin;
    float rangeMaxClamped = max(fxaaQualityEdgeThresholdMin, rangeMaxScaled);
    bool earlyExit = range < rangeMaxClamped;
/*--------------------------------------------------------------------------*/
    if(earlyExit)
        return FxaaTexTop(tex, pos);
/*--------------------------------------------------------------------------*/
    float lumaNW = LumaOff(tex, posM, Fxaa_vec2(-1,-1), fxaaQualityRcpFrame.xy);
    float lumaSE = LumaOff(tex, posM, Fxaa_vec2( 1, 1), fxaaQualityRcpFrame.xy);
    float lumaNE = LumaOff(tex, posM, Fxaa_vec2( 1,-1), fxaaQualityRcpFrame.xy);
    float lumaSW = LumaOff(tex, posM, Fxaa_vec2(-1, 1), fxaaQualityRcpFrame.xy);
/*--------------------------------------------------------------------------*/
    float lumaNS = lumaN + lumaS;
    float lumaWE = lumaW + lumaE;
    float subpixRcpRange = 1.0/range;
    float subpixNSWE = lumaNS + lumaWE;
    float edgeHorz1 = (-2.0 * lumaM) + lumaNS;
    float edgeVert1 = (-2.0 * lumaM) + lumaWE;
/*--------------------------------------------------------------------------*/
    float lumaNESE = lumaNE + lumaSE;
    float lumaNWNE = lumaNW + lumaNE;
    float edgeHorz2 = (-2.0 * lumaE) + lumaNESE;
    float edgeVert2 = (-2.0 * lumaN) + lumaNWNE;
/*--------------------------------------------------------------------------*/
    float lumaNWSW = lumaNW + lumaSW;
    float lumaSWSE = lumaSW + lumaSE;
    float edgeHorz4 = (abs(edgeHorz1) * 2.0) + abs(edgeHorz2);
    float edgeVert4 = (abs(edgeVert1) * 2.0) + abs(edgeVert2);
    float edgeHorz3 = (-2.0 * lumaW) + lumaNWSW;
    float edgeVert3 = (-2.0 * lumaS) + lumaSWSE;
    float edgeHorz = abs(edgeHorz3) + edgeHorz4;
    float edgeVert = abs(edgeVert3) + edgeVert4;
/*--------------------------------------------------------------------------*/
    float subpixNWSWNESE = lumaNWSW + lumaNESE;
    float lengthSign = fxaaQualityRcpFrame.x;
    bool horzSpan = edgeHorz >= edgeVert;
    float subpixA = subpixNSWE * 2.0 + subpixNWSWNESE;
/*--------------------------------------------------------------------------*/
    if(!horzSpan) lumaN = lumaW;
    if(!horzSpan) lumaS = lumaE;
    if(horzSpan) lengthSign = fxaaQualityRcpFrame.y;
    float subpixB = (subpixA * (1.0/12.0)) - lumaM;
/*--------------------------------------------------------------------------*/
    float gradientN = lumaN - lumaM;
    float gradientS = lumaS - lumaM;
    float lumaNN = lumaN + lumaM;
    float lumaSS = lumaS + lumaM;
    bool pairN = abs(gradientN) >= abs(gradientS);
    float gradient = max(abs(gradientN), abs(gradientS));
    if(pairN) lengthSign = -lengthSign;
    float subpixC = clamp((abs(subpixB) * subpixRcpRange), 0.0, 1.0);
/*--------------------------------------------------------------------------*/
    vec2 posB;
    posB.x = posM.x;
    posB.y = posM.y;
    vec2 offNP;
    offNP.x = (!horzSpan) ? 0.0 : fxaaQualityRcpFrame.x;
    offNP.y = ( horzSpan) ? 0.0 : fxaaQualityRcpFrame.y;
    if(!horzSpan) posB.x += lengthSign * 0.5;
    if( horzSpan) posB.y += lengthSign * 0.5;
/*--------------------------------------------------------------------------*/
    vec2 posN;
    posN.x = posB.x - offNP.x * FXAA_QUALITY_P0;
    posN.y = posB.y - offNP.y * FXAA_QUALITY_P0;
    vec2 posP;
    posP.x = posB.x + offNP.x * FXAA_QUALITY_P0;
    posP.y = posB.y + offNP.y * FXAA_QUALITY_P0;
    float subpixD = ((-2.0)*subpixC) + 3.0;
    float lumaEndN = LumaTop(tex, posN);
    float subpixE = subpixC * subpixC;
    float lumaEndP = LumaTop(tex, posP);
/*--------------------------------------------------------------------------*/
    if(!pairN) lumaNN = lumaSS;
    float gradientScaled = gradient * 1.0/4.0;
    float lumaMM = lumaM - lumaNN * 0.5;
    float subpixF = subpixD * subpixE;
    bool lumaMLTZero = lumaMM < 0.0;
/*--------------------------------------------------------------------------*/
    lumaEndN -= lumaNN * 0.5;
    lumaEndP -= lumaNN * 0.5;
    bool doneN = abs(lumaEndN) >= gradientScaled;
    bool doneP = abs(lumaEndP) >= gradientScaled;
    if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P1;
    if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P1;
    bool doneNP = (!doneN) || (!doneP);
    if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P1;
    if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P1;
/*--------------------------------------------------------------------------*/
    if(doneNP) {
        if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
        if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
        if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
        if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
        doneN = abs(lumaEndN) >= gradientScaled;
        doneP = abs(lumaEndP) >= gradientScaled;
        if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P2;
        if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P2;
        doneNP = (!doneN) || (!doneP);
        if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P2;
        if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P2;
/*--------------------------------------------------------------------------*/
        #if (FXAA_QUALITY_PS > 3)
        if(doneNP) {
            if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
            if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
            if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
            if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
            doneN = abs(lumaEndN) >= gradientScaled;
            doneP = abs(lumaEndP) >= gradientScaled;
            if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P3;
            if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P3;
            doneNP = (!doneN) || (!doneP);
            if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P3;
            if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P3;
/*--------------------------------------------------------------------------*/
            #if (FXAA_QUALITY_PS > 4)
            if(doneNP) {
                if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                doneN = abs(lumaEndN) >= gradientScaled;
                doneP = abs(lumaEndP) >= gradientScaled;
                if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P4;
                if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P4;
                doneNP = (!doneN) || (!doneP);
                if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P4;
                if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P4;
/*--------------------------------------------------------------------------*/
                #if (FXAA_QUALITY_PS > 5)
                if(doneNP) {
                    if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                    if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                    if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                    if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                    doneN = abs(lumaEndN) >= gradientScaled;
                    doneP = abs(lumaEndP) >= gradientScaled;
                    if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P5;
                    if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P5;
                    doneNP = (!doneN) || (!doneP);
                    if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P5;
                    if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P5;
/*--------------------------------------------------------------------------*/
                    #if (FXAA_QUALITY_PS > 6)
                    if(doneNP) {
                        if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                        if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                        if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                        if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                        doneN = abs(lumaEndN) >= gradientScaled;
                        doneP = abs(lumaEndP) >= gradientScaled;
                        if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P6;
                        if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P6;
                        doneNP = (!doneN) || (!doneP);
                        if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P6;
                        if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P6;
/*--------------------------------------------------------------------------*/
                        #if (FXAA_QUALITY_PS > 7)
                        if(doneNP) {
                            if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                            if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                            if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                            if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                            doneN = abs(lumaEndN) >= gradientScaled;
                            doneP = abs(lumaEndP) >= gradientScaled;
                            if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P7;
                            if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P7;
                            doneNP = (!doneN) || (!doneP);
                            if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P7;
                            if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P7;
/*--------------------------------------------------------------------------*/
    #if (FXAA_QUALITY_PS > 8)
    if(doneNP) {
        if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
        if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
        if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
        if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
        doneN = abs(lumaEndN) >= gradientScaled;
        doneP = abs(lumaEndP) >= gradientScaled;
        if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P8;
        if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P8;
        doneNP = (!doneN) || (!doneP);
        if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P8;
        if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P8;
/*--------------------------------------------------------------------------*/
        #if (FXAA_QUALITY_PS > 9)
        if(doneNP) {
            if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
            if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
            if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
            if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
            doneN = abs(lumaEndN) >= gradientScaled;
            doneP = abs(lumaEndP) >= gradientScaled;
            if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P9;
            if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P9;
            doneNP = (!doneN) || (!doneP);
            if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P9;
            if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P9;
/*--------------------------------------------------------------------------*/
            #if (FXAA_QUALITY_PS > 10)
            if(doneNP) {
                if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                doneN = abs(lumaEndN) >= gradientScaled;
                doneP = abs(lumaEndP) >= gradientScaled;
                if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P10;
                if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P10;
                doneNP = (!doneN) || (!doneP);
                if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P10;
                if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P10;
/*--------------------------------------------------------------------------*/
                #if (FXAA_QUALITY_PS > 11)
                if(doneNP) {
                    if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                    if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                    if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                    if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                    doneN = abs(lumaEndN) >= gradientScaled;
                    doneP = abs(lumaEndP) >= gradientScaled;
                    if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P11;
                    if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P11;
                    doneNP = (!doneN) || (!doneP);
                    if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P11;
                    if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P11;
/*--------------------------------------------------------------------------*/
                    #if (FXAA_QUALITY_PS > 12)
                    if(doneNP) {
                        if(!doneN) lumaEndN = LumaTop(tex, posN.xy);
                        if(!doneP) lumaEndP = LumaTop(tex, posP.xy);
                        if(!doneN) lumaEndN = lumaEndN - lumaNN * 0.5;
                        if(!doneP) lumaEndP = lumaEndP - lumaNN * 0.5;
                        doneN = abs(lumaEndN) >= gradientScaled;
                        doneP = abs(lumaEndP) >= gradientScaled;
                        if(!doneN) posN.x -= offNP.x * FXAA_QUALITY_P12;
                        if(!doneN) posN.y -= offNP.y * FXAA_QUALITY_P12;
                        doneNP = (!doneN) || (!doneP);
                        if(!doneP) posP.x += offNP.x * FXAA_QUALITY_P12;
                        if(!doneP) posP.y += offNP.y * FXAA_QUALITY_P12;
/*--------------------------------------------------------------------------*/
                    }
                    #endif
/*--------------------------------------------------------------------------*/
                }
                #endif
/*--------------------------------------------------------------------------*/
            }
            #endif
/*--------------------------------------------------------------------------*/
        }
        #endif
/*--------------------------------------------------------------------------*/
    }
    #endif
/*--------------------------------------------------------------------------*/
                        }
                        #endif
/*--------------------------------------------------------------------------*/
                    }
                    #endif
/*--------------------------------------------------------------------------*/
                }
                #endif
/*--------------------------------------------------------------------------*/
            }
            #endif
/*--------------------------------------------------------------------------*/
        }
        #endif
/*--------------------------------------------------------------------------*/
    }
/*--------------------------------------------------------------------------*/
    float dstN = posM.x - posN.x;
    float dstP = posP.x - posM.x;
    if(!horzSpan) dstN = posM.y - posN.y;
    if(!horzSpan) dstP = posP.y - posM.y;
/*--------------------------------------------------------------------------*/
    bool goodSpanN = (lumaEndN < 0.0) != lumaMLTZero;
    float spanLength = (dstP + dstN);
    bool goodSpanP = (lumaEndP < 0.0) != lumaMLTZero;
    float spanLengthRcp = 1.0/spanLength;
/*--------------------------------------------------------------------------*/
    bool directionN = dstN < dstP;
    float dst = min(dstN, dstP);
    bool goodSpan = directionN ? goodSpanN : goodSpanP;
    float subpixG = subpixF * subpixF;
    float pixelOffset = (dst * (-spanLengthRcp)) + 0.5;
    float subpixH = subpixG * fxaaQualitySubpix;
/*--------------------------------------------------------------------------*/
    float pixelOffsetGood = goodSpan ? pixelOffset : 0.0;
    float pixelOffsetSubpix = max(pixelOffsetGood, subpixH);
    if(!horzSpan) posM.x += pixelOffsetSubpix * lengthSign;
    if( horzSpan) posM.y += pixelOffsetSubpix * lengthSign;
    return FxaaTexTop(tex, posM);
}
/*==========================================================================*/

/*============================================================================

                      Urho3D Vertex- and Pixelshader
                      
============================================================================*/

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vScreenPos = GetScreenPosPreDiv(gl_Position);
}

void PS()
{
    vec2 rcpFrame = vec2(cGBufferInvSize.x, cGBufferInvSize.y);

    gl_FragColor = FxaaPixelShader(
        vScreenPos,                         // vec2 pos,
        sDiffMap,                           // sampler2D tex,
        rcpFrame,                           // vec2 fxaaQualityRcpFrame,
        0.75,                               // float fxaaQualitySubpix,
        0.166,                              // float fxaaQualityEdgeThreshold,
        0.0833                              // float fxaaQualityEdgeThresholdMin
    );
}
````

### GammaCorrection.glsl

原始位置：`engine/Shaders/GLSL/GammaCorrection.glsl`。

SHA256：`6e9484509453a70e469a7c22b5e1181abcde8d1fd13166bac0d1bc55be3311f0`。

````glsl
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"
#include "ScreenPos.glsl"
#include "PostProcess.glsl"

varying vec2 vScreenPos;

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);
    vScreenPos = GetScreenPosPreDiv(gl_Position);
}

void PS()
{
    vec3 color = texture2D(sDiffMap, vScreenPos).rgb;
    gl_FragColor = vec4(ToInverseGamma(color), 1.0);
}
````

### PostProcess.glsl

原始位置：`engine/Shaders/GLSL/PostProcess.glsl`。

SHA256：`b64447aac9befc8c8ab9aca07accfb89cfe905efab55f7167130bbe5772aca0d`。

````glsl
#ifdef COMPILEPS
const float PI = 3.14159265;

vec2 Noise(vec2 coord)
{
    float noiseX = clamp(fract(sin(dot(coord, vec2(12.9898, 78.233))) * 43758.5453), 0.0, 1.0);
    float noiseY = clamp(fract(sin(dot(coord, vec2(12.9898, 78.233) * 2.0)) * 43758.5453), 0.0, 1.0);
    return vec2(noiseX, noiseY);
}

// Adapted: http://callumhay.blogspot.com/2010/09/gaussian-blur-shader-glsl.html
vec4 GaussianBlur(int blurKernelSize, vec2 blurDir, vec2 blurRadius, float sigma, sampler2D texSampler, vec2 texCoord)
{
    int blurKernelSizeHalfSize = blurKernelSize / 2;

    // Incremental Gaussian Coefficent Calculation (See GPU Gems 3 pp. 877 - 889)
    vec3 gaussCoeff;
    gaussCoeff.x = 1.0 / (sqrt(2.0 * PI) * sigma);
    gaussCoeff.y = exp(-0.5 / (sigma * sigma));
    gaussCoeff.z = gaussCoeff.y * gaussCoeff.y;

    vec2 blurVec = blurRadius * blurDir;
    vec4 avgValue = vec4(0.0);
    float gaussCoeffSum = 0.0;

    avgValue += texture2D(texSampler, texCoord) * gaussCoeff.x;
    gaussCoeffSum += gaussCoeff.x;
    gaussCoeff.xy *= gaussCoeff.yz;

    for (int i = 1; i <= blurKernelSizeHalfSize; i++)
    {
        avgValue += texture2D(texSampler, texCoord - float(i) * blurVec) * gaussCoeff.x;
        avgValue += texture2D(texSampler, texCoord + float(i) * blurVec) * gaussCoeff.x;

        gaussCoeffSum += 2.0 * gaussCoeff.x;
        gaussCoeff.xy *= gaussCoeff.yz;
    }

    return avgValue / gaussCoeffSum;
}

const vec3 LumWeights = vec3(0.2126, 0.7152, 0.0722);

vec3 ReinhardEq3Tonemap(vec3 x)
{
    return x / (1.0 + x);
}

vec3 ReinhardEq4Tonemap(vec3 x, float white)
{
    return x * (1.0 + x / white) / (1.0 + x);
}

// Unchared2 tone mapping (See http://filmicgames.com)
const float A = 0.15;
const float B = 0.50;
const float C = 0.10;
const float D = 0.20;
const float E = 0.02;
const float F = 0.30;

vec3 Uncharted2Tonemap(vec3 x)
{
   return ((x*(A*x+C*B)+D*E)/(x*(A*x+B)+D*F))-E/F;
}

#ifndef GL_ES
vec3 ColorCorrection(vec3 color, sampler3D lut)
{
    float lutSize = 16.0;
    float scale = (lutSize - 1.0) / lutSize;
    float offset = 1.0 / (2.0 * lutSize);
    return texture3D(lut, clamp(color, 0.0, 1.0) * scale + offset).rgb;
}
#endif

const float Gamma = 2.2;
const float InverseGamma = 1.0 / 2.2;

vec3 ToGamma(vec3 color)
{
    return vec3(pow(color.r, Gamma), pow(color.g, Gamma), pow(color.b, Gamma));
}

vec3 ToInverseGamma(vec3 color)
{
    return vec3(pow(color.r, InverseGamma), pow(color.g, InverseGamma), pow(color.b, InverseGamma));
}
#endif
````

### Samplers.glsl

原始位置：`engine/Shaders/GLSL/Samplers.glsl`。

SHA256：`7b5642d21ccfa9294e96a8e6f93c99f5fe917bbd46f8cb605d24c033ebb38149`。

````glsl
#ifdef COMPILEPS
uniform sampler2D sDiffMap;
uniform samplerCube sDiffCubeMap;
uniform sampler2D sNormalMap;
uniform sampler2D sSpecMap;
uniform sampler2D sEmissiveMap;
uniform sampler2D sEnvMap;
uniform samplerCube sEnvCubeMap;
uniform sampler2D sLightRampMap;
uniform sampler2D sLightSpotMap;
uniform samplerCube sLightCubeMap;
#ifndef GL_ES
    uniform sampler3D sVolumeMap;
    uniform sampler2D sAlbedoBuffer;
    uniform sampler2D sNormalBuffer;
    uniform sampler2D sDepthBuffer;
    uniform sampler2D sLightBuffer;
    #ifdef VSM_SHADOW
        uniform sampler2D sShadowMap;
    #else
        uniform sampler2DShadow sShadowMap;
    #endif
    uniform samplerCube sFaceSelectCubeMap;
    uniform samplerCube sIndirectionCubeMap;
    uniform samplerCube sZoneCubeMap;
    uniform sampler3D sZoneVolumeMap;
#else
    uniform highp sampler2D sShadowMap;
#endif

#ifdef GL3
#define texture2D texture
#define texture2DProj textureProj
#define texture3D texture
#define textureCube texture
#define texture2DLod textureLod
#define texture2DLodOffset textureLodOffset
#endif

vec3 DecodeNormal(vec4 normalInput)
{
    #ifdef PACKEDNORMAL
        vec3 normal;
        normal.xy = normalInput.ag * 2.0 - 1.0;
        normal.z = sqrt(max(1.0 - dot(normal.xy, normal.xy), 0.0));
        return normal;
    #else
        return normalize(normalInput.rgb * 2.0 - 1.0);
    #endif
}

vec3 EncodeDepth(float depth)
{
    #ifndef GL3
        vec3 ret;
        depth *= 255.0;
        ret.x = floor(depth);
        depth = (depth - ret.x) * 255.0;
        ret.y = floor(depth);
        ret.z = (depth - ret.y);
        ret.xy *= 1.0 / 255.0;
        return ret;
    #else
        // OpenGL 3 can use different MRT formats, so no need for encoding
        return vec3(depth, 0.0, 0.0);
    #endif
}

float DecodeDepth(vec3 depth)
{
    #ifndef GL3
        const vec3 dotValues = vec3(1.0, 1.0 / 255.0, 1.0 / (255.0 * 255.0));
        return dot(depth, dotValues);
    #else
        // OpenGL 3 can use different MRT formats, so no need for encoding
        return depth.r;
    #endif
}

float ReconstructDepth(float hwDepth)
{
    return dot(vec2(hwDepth, cDepthReconstruct.y / (hwDepth - cDepthReconstruct.x)), cDepthReconstruct.zw);
}
#endif
````

### ScreenPos.glsl

原始位置：`engine/Shaders/GLSL/ScreenPos.glsl`。

SHA256：`8c71a12d2a37f57326ba7cb8ecf6f7700abe72a32f6ac1c062cd491bde575f6a`。

````glsl
#ifdef COMPILEVS
mat3 GetCameraRot()
{
    return mat3(cViewInv[0][0], cViewInv[0][1], cViewInv[0][2],
        cViewInv[1][0], cViewInv[1][1], cViewInv[1][2],
        cViewInv[2][0], cViewInv[2][1], cViewInv[2][2]);
}

vec4 GetScreenPos(vec4 clipPos)
{
    return vec4(
        clipPos.x * cGBufferOffsets.z + cGBufferOffsets.x * clipPos.w,
        clipPos.y * cGBufferOffsets.w + cGBufferOffsets.y * clipPos.w,
        0.0,
        clipPos.w);
}

vec2 GetScreenPosPreDiv(vec4 clipPos)
{
    return vec2(
        clipPos.x / clipPos.w * cGBufferOffsets.z + cGBufferOffsets.x,
        clipPos.y / clipPos.w * cGBufferOffsets.w + cGBufferOffsets.y);
}

vec2 GetQuadTexCoord(vec4 clipPos)
{
    return vec2(
        clipPos.x / clipPos.w * 0.5 + 0.5,
        clipPos.y / clipPos.w * 0.5 + 0.5);
}

vec2 GetQuadTexCoordNoFlip(vec3 worldPos)
{
    return vec2(
        worldPos.x * 0.5 + 0.5,
        -worldPos.y * 0.5 + 0.5);
}

vec3 GetFarRay(vec4 clipPos)
{
    vec3 viewRay = vec3(
        clipPos.x / clipPos.w * cFrustumSize.x,
        clipPos.y / clipPos.w * cFrustumSize.y,
        cFrustumSize.z);

    return viewRay * GetCameraRot();
}

vec3 GetNearRay(vec4 clipPos)
{
    vec3 viewRay = vec3(
        clipPos.x / clipPos.w * cFrustumSize.x,
        clipPos.y / clipPos.w * cFrustumSize.y,
        0.0);

    return (viewRay * GetCameraRot()) * cDepthMode.x;
}
#endif
````

### Text.glsl

原始位置：`engine/Shaders/GLSL/Text.glsl`。

SHA256：`ce4e69c1477a0da5902d824a58ff91b5c25b7bc15307c89bf95d2642bd3a663e`。

````glsl
#include "Uniforms.glsl"
#include "Samplers.glsl"
#include "Transform.glsl"

varying vec2 vTexCoord;
varying vec4 vColor;

#ifdef TEXT_EFFECT_SHADOW
uniform vec2 cShadowOffset;
uniform vec4 cShadowColor;
#endif

#ifdef TEXT_EFFECT_STROKE
uniform vec4 cStrokeColor;
#endif

void VS()
{
    mat4 modelMatrix = iModelMatrix;
    vec3 worldPos = GetWorldPos(modelMatrix);
    gl_Position = GetClipPos(worldPos);

    vTexCoord = iTexCoord;
    vColor = iColor;
}

/*
    1) Simplest SDF shader:

    float distance = texture2D(sDiffMap, vTexCoord).a;
    if (distance >= 0.5)
        gl_FragColor.a = vColor.a; // This is glyph
    else
        gl_FragColor.a = 0.0; // Outside glyph

    2) Glyph with antialiazed border:

    float distance = texture2D(sDiffMap, vTexCoord).a;
    gl_FragColor.a = vColor.a * smoothstep(0.495, 0.505, distance);

    3) Quality improvement for far and small text:

    float distance = texture2D(sDiffMap, vTexCoord).a;
    // How much "distance" is changed for neighboring pixels.
    // If text is far then width is big. Far text will be blurred.
    float width = fwidth(distance);
    gl_FragColor.a = vColor.a * smoothstep(0.5 - width, 0.5 + width, distance);
*/

#if defined(COMPILEPS) && defined(SIGNED_DISTANCE_FIELD)
    float GetAlpha(float distance, float width)
    {
        return smoothstep(0.5 - width, 0.5 + width, distance);
    }

    // Comment this define to turn off supersampling
    #define SUPERSAMPLING
#endif

void PS()
{
#ifdef SIGNED_DISTANCE_FIELD
    gl_FragColor.rgb = vColor.rgb;
    float distance = texture2D(sDiffMap, vTexCoord).a;

    #ifdef TEXT_EFFECT_STROKE
        #ifdef SUPERSAMPLING
            float outlineFactor = smoothstep(0.5, 0.525, distance); // Border of glyph
            gl_FragColor.rgb = mix(cStrokeColor.rgb, vColor.rgb, outlineFactor);
        #else
            if (distance < 0.525)
               gl_FragColor.rgb = cStrokeColor.rgb;
        #endif
    #endif

    #ifdef TEXT_EFFECT_SHADOW
        if (texture2D(sDiffMap, vTexCoord - cShadowOffset).a > 0.5 && distance <= 0.5)
            gl_FragColor = cShadowColor;
        #ifndef SUPERSAMPLING
        else if (distance <= 0.5)
            gl_FragColor.a = 0;
        #endif
        else
    #endif
        {
            float width = fwidth(distance);
            float alpha = GetAlpha(distance, width);

            #ifdef SUPERSAMPLING
                vec2 deltaUV = 0.354 * fwidth(vTexCoord); // (1.0 / sqrt(2.0)) / 2.0 = 0.354
                vec4 square = vec4(vTexCoord - deltaUV, vTexCoord + deltaUV);

                float distance2 = texture2D(sDiffMap, square.xy).a;
                float distance3 = texture2D(sDiffMap, square.zw).a;
                float distance4 = texture2D(sDiffMap, square.xw).a;
                float distance5 = texture2D(sDiffMap, square.zy).a;

                alpha += GetAlpha(distance2, width)
                       + GetAlpha(distance3, width)
                       + GetAlpha(distance4, width)
                       + GetAlpha(distance5, width);

                // For calculating of average correct would be dividing by 5.
                // But when text is blurred, its brightness is lost. Therefore divide by 4.
                alpha = alpha * 0.25;
            #endif

            gl_FragColor.a = alpha;
        }
#else
    #ifdef ALPHAMAP
        gl_FragColor.rgb = vColor.rgb;
        #ifdef GL3
            gl_FragColor.a = vColor.a * texture2D(sDiffMap, vTexCoord).r;
        #else
            gl_FragColor.a = vColor.a * texture2D(sDiffMap, vTexCoord).a;
        #endif
    #else
        gl_FragColor = vColor * texture2D(sDiffMap, vTexCoord);
    #endif
#endif
}
````

### Transform.glsl

原始位置：`engine/Shaders/GLSL/Transform.glsl`。

SHA256：`00c4266e2bca4fde9371ba92d3e4e9111c63c76214c2180dda3ff4fb4da4a577`。

````glsl
#ifdef COMPILEVS

// Silence GLSL 150 deprecation warnings
#ifdef GL3
#define attribute in
#define varying out
#endif

attribute vec4 iPos;
attribute vec3 iNormal;
attribute vec4 iColor;
attribute vec2 iTexCoord;
attribute vec2 iTexCoord1;
attribute vec4 iTangent;
attribute vec4 iBlendWeights;
attribute vec4 iBlendIndices;
attribute vec3 iCubeTexCoord;
attribute vec4 iCubeTexCoord1;
#ifdef INSTANCED
    attribute vec4 iTexCoord4;
    attribute vec4 iTexCoord5;
    attribute vec4 iTexCoord6;
#endif
attribute int iObjectIndex;

#ifdef SKINNED
mat4 GetSkinMatrix(vec4 blendWeights, vec4 blendIndices)
{
    ivec4 idx = ivec4(blendIndices) * 3;
    const vec4 lastColumn = vec4(0.0, 0.0, 0.0, 1.0);
    return mat4(cSkinMatrices[idx.x], cSkinMatrices[idx.x + 1], cSkinMatrices[idx.x + 2], lastColumn) * blendWeights.x +
        mat4(cSkinMatrices[idx.y], cSkinMatrices[idx.y + 1], cSkinMatrices[idx.y + 2], lastColumn) * blendWeights.y +
        mat4(cSkinMatrices[idx.z], cSkinMatrices[idx.z + 1], cSkinMatrices[idx.z + 2], lastColumn) * blendWeights.z +
        mat4(cSkinMatrices[idx.w], cSkinMatrices[idx.w + 1], cSkinMatrices[idx.w + 2], lastColumn) * blendWeights.w;
}
#endif

#ifdef INSTANCED
mat4 GetInstanceMatrix()
{
    const vec4 lastColumn = vec4(0.0, 0.0, 0.0, 1.0);
    return mat4(iTexCoord4, iTexCoord5, iTexCoord6, lastColumn);
}
#endif

mat3 GetNormalMatrix(mat4 modelMatrix)
{
    return mat3(modelMatrix[0].xyz, modelMatrix[1].xyz, modelMatrix[2].xyz);
}

vec2 GetTexCoord(vec2 texCoord)
{
    return vec2(dot(texCoord, cUOffset.xy) + cUOffset.w, dot(texCoord, cVOffset.xy) + cVOffset.w);
}

vec4 GetClipPos(vec3 worldPos)
{
    vec4 ret = vec4(worldPos, 1.0) * cViewProj;
    // While getting the clip coordinate, also automatically set gl_ClipVertex for user clip planes
    #if !defined(GL_ES) && !defined(GL3)
        gl_ClipVertex = ret;
    #elif defined(GL3)
        gl_ClipDistance[0] = dot(cClipPlane, ret);
    #endif
    return ret;
}

float GetZonePos(vec3 worldPos)
{
    return clamp((vec4(worldPos, 1.0) * cZone).z, 0.0, 1.0);
}

float GetDepth(vec4 clipPos)
{
    return dot(clipPos.zw, cDepthMode.zw);
}

#ifdef BILLBOARD
vec3 GetBillboardPos(vec4 iPos, vec2 iSize, mat4 modelMatrix)
{
    return (iPos * modelMatrix).xyz + vec3(iSize.x, iSize.y, 0.0) * cBillboardRot;
}

vec3 GetBillboardNormal()
{
    return vec3(-cBillboardRot[0][2], -cBillboardRot[1][2], -cBillboardRot[2][2]);
}
#endif

#ifdef DIRBILLBOARD
mat3 GetFaceCameraRotation(vec3 position, vec3 direction)
{
    vec3 cameraDir = normalize(position - cCameraPos);
    vec3 front = normalize(direction);
    vec3 right = normalize(cross(front, cameraDir));
    vec3 up = normalize(cross(front, right));

    return mat3(
        right.x, up.x, front.x,
        right.y, up.y, front.y,
        right.z, up.z, front.z
    );
}

vec3 GetBillboardPos(vec4 iPos, vec3 iDirection, mat4 modelMatrix)
{
    vec3 worldPos = (iPos * modelMatrix).xyz;
    return worldPos + vec3(iTexCoord1.x, 0.0, iTexCoord1.y) * GetFaceCameraRotation(worldPos, iDirection);
}

vec3 GetBillboardNormal(vec4 iPos, vec3 iDirection, mat4 modelMatrix)
{
    vec3 worldPos = (iPos * modelMatrix).xyz;
    return vec3(0.0, 1.0, 0.0) * GetFaceCameraRotation(worldPos, iDirection);
}
#endif

#ifdef TRAILFACECAM
vec3 GetTrailPos(vec4 iPos, vec3 iFront, float iScale, mat4 modelMatrix)
{
    vec3 up = normalize(cCameraPos - iPos.xyz);
    vec3 right = normalize(cross(iFront, up));
    return (vec4((iPos.xyz + right * iScale), 1.0) * modelMatrix).xyz;
}

vec3 GetTrailNormal(vec4 iPos)
{
    return normalize(cCameraPos - iPos.xyz);
}
#endif

#ifdef TRAILBONE
vec3 GetTrailPos(vec4 iPos, vec3 iParentPos, float iScale, mat4 modelMatrix)
{
    vec3 right = iParentPos - iPos.xyz;
    return (vec4((iPos.xyz + right * iScale), 1.0) * modelMatrix).xyz;
}

vec3 GetTrailNormal(vec4 iPos, vec3 iParentPos, vec3 iForward)
{
    vec3 left = normalize(iPos.xyz - iParentPos);
    vec3 up = normalize(cross(normalize(iForward), left));
    return up;
}
#endif

#if defined(SKINNED)
    #define iModelMatrix GetSkinMatrix(iBlendWeights, iBlendIndices)
#elif defined(INSTANCED)
    #define iModelMatrix GetInstanceMatrix()
#else
    #define iModelMatrix cModel
#endif

vec3 GetWorldPos(mat4 modelMatrix)
{
    #if defined(BILLBOARD)
        return GetBillboardPos(iPos, iTexCoord1, modelMatrix);
    #elif defined(DIRBILLBOARD)
        return GetBillboardPos(iPos, iNormal, modelMatrix);
    #elif defined(TRAILFACECAM)
        return GetTrailPos(iPos, iTangent.xyz, iTangent.w, modelMatrix);
    #elif defined(TRAILBONE)
        return GetTrailPos(iPos, iTangent.xyz, iTangent.w, modelMatrix);
    #else
        return (iPos * modelMatrix).xyz;
    #endif
}

vec3 GetWorldNormal(mat4 modelMatrix)
{
    #if defined(BILLBOARD)
        return GetBillboardNormal();
    #elif defined(DIRBILLBOARD)
        return GetBillboardNormal(iPos, iNormal, modelMatrix);
    #elif defined(TRAILFACECAM)
        return GetTrailNormal(iPos);
    #elif defined(TRAILBONE)
        return GetTrailNormal(iPos, iTangent.xyz, iNormal);
    #else
        return normalize(iNormal * GetNormalMatrix(modelMatrix));
    #endif
}

vec4 GetWorldTangent(mat4 modelMatrix)
{
    #if defined(BILLBOARD)
        return vec4(normalize(vec3(1.0, 0.0, 0.0) * cBillboardRot), 1.0);
    #elif defined(DIRBILLBOARD)
        return vec4(normalize(vec3(1.0, 0.0, 0.0) * GetNormalMatrix(modelMatrix)), 1.0);
    #else
        return vec4(normalize(iTangent.xyz * GetNormalMatrix(modelMatrix)), iTangent.w);
    #endif
}

#else

// Silence GLSL 150 deprecation warnings
#ifdef GL3
#define varying in

#ifndef MRT_COUNT

#if defined(DEFERRED)
#define MRT_COUNT 4
#elif defined(PREPASS)
#define MRT_COUNT 2
#else
#define MRT_COUNT 1
#endif

#endif

out vec4 fragData[MRT_COUNT];


#define gl_FragColor fragData[0]
#define gl_FragData fragData
#endif

#endif
````

### Uniforms.glsl

原始位置：`engine/Shaders/GLSL/Uniforms.glsl`。

SHA256：`6fd96d8b5a26eba3ff3b0fc9e9d2d43521e7617da48b9fc92a3088a15a7f6829`。

````glsl
// Use of constant buffers on OpenGL 3 commented out for now as it seems to be slower in practice
//#define USE_CBUFFERS

#if !defined(GL3) || !defined(USE_CBUFFERS)

// OpenGL 2 uniforms (no constant buffers)

#ifdef COMPILEVS

// Vertex shader uniforms
uniform vec3 cAmbientStartColor;
uniform vec3 cAmbientEndColor;
uniform mat3 cBillboardRot;
uniform vec3 cCameraPos;
uniform float cNearClip;
uniform float cFarClip;
uniform vec4 cDepthMode;
uniform vec3 cFrustumSize;
uniform float cDeltaTime;
uniform float cElapsedTime;
uniform vec4 cGBufferOffsets;
uniform vec4 cLightPos;
uniform vec3 cLightDir;
uniform vec4 cNormalOffsetScale;
uniform mat4 cModel;
uniform mat4 cView;
uniform mat4 cViewInv;
uniform mat4 cViewProj;
uniform vec4 cUOffset;
uniform vec4 cVOffset;
uniform mat4 cZone;
#if !defined(GL_ES) || defined(WEBGL)
    uniform mat4 cLightMatrices[4];
#else
    uniform highp mat4 cLightMatrices[2];
#endif
#ifdef SKINNED
    uniform vec4 cSkinMatrices[MAXBONES*3];
#endif
#ifdef NUMVERTEXLIGHTS
    uniform vec4 cVertexLights[4*3];
#endif
#ifdef GL3
    uniform vec4 cClipPlane;
#endif
#endif

#ifdef COMPILEPS

// Fragment shader uniforms
#ifdef GL_ES
    precision mediump float;
#endif

uniform vec4 cAmbientColor;
uniform vec3 cCameraPosPS;
uniform float cDeltaTimePS;
uniform vec4 cDepthReconstruct;
uniform float cElapsedTimePS;
uniform vec4 cFogParams;
uniform vec3 cFogColor;
uniform vec2 cGBufferInvSize;
uniform vec4 cLightColor;
uniform vec4 cLightPosPS;
uniform vec3 cLightDirPS;
uniform vec4 cNormalOffsetScalePS;
uniform vec4 cMatDiffColor;
uniform vec3 cMatEmissiveColor;
uniform vec3 cMatEnvMapColor;
uniform vec4 cMatSpecColor;
#ifdef PBR
    uniform float cRoughness;
    uniform float cMetallic;
    uniform float cLightRad;
    uniform float cLightLength;
#endif
uniform vec3 cZoneMin;
uniform vec3 cZoneMax;
uniform float cNearClipPS;
uniform float cFarClipPS;
uniform vec4 cShadowCubeAdjust;
uniform vec4 cShadowDepthFade;
uniform vec2 cShadowIntensity;
uniform vec2 cShadowMapInvSize;
uniform vec4 cShadowSplits;
uniform mat4 cLightMatricesPS[4];
#ifdef VSM_SHADOW
uniform vec2 cVSMShadowParams;
#endif
#endif

#else

// OpenGL 3 uniforms (using constant buffers)

#ifdef COMPILEVS

uniform FrameVS
{
    float cDeltaTime;
    float cElapsedTime;
};

uniform CameraVS
{
    vec3 cCameraPos;
    float cNearClip;
    float cFarClip;
    vec4 cDepthMode;
    vec3 cFrustumSize;
    vec4 cGBufferOffsets;
    mat4 cView;
    mat4 cViewInv;
    mat4 cViewProj;
    vec4 cClipPlane;
};

uniform ZoneVS
{
    vec3 cAmbientStartColor;
    vec3 cAmbientEndColor;
    mat4 cZone;
};

uniform LightVS
{
    vec4 cLightPos;
    vec3 cLightDir;
    vec4 cNormalOffsetScale;
#ifdef NUMVERTEXLIGHTS
    vec4 cVertexLights[4 * 3];
#else
    mat4 cLightMatrices[4];
#endif
};

#ifndef CUSTOM_MATERIAL_CBUFFER
uniform MaterialVS
{
    vec4 cUOffset;
    vec4 cVOffset;
};
#endif

uniform ObjectVS
{
    mat4 cModel;
#ifdef BILLBOARD
    mat3 cBillboardRot;
#endif
#ifdef SKINNED
    uniform vec4 cSkinMatrices[MAXBONES*3];
#endif
};

#endif

#ifdef COMPILEPS

// Pixel shader uniforms
uniform FramePS
{
    float cDeltaTimePS;
    float cElapsedTimePS;
};

uniform CameraPS
{
    vec3 cCameraPosPS;
    vec4 cDepthReconstruct;
    vec2 cGBufferInvSize;
    float cNearClipPS;
    float cFarClipPS;
};

uniform ZonePS
{
    vec4 cAmbientColor;
    vec4 cFogParams;
    vec3 cFogColor;
    vec3 cZoneMin;
    vec3 cZoneMax;
};

uniform LightPS
{
    vec4 cLightColor;
    vec4 cLightPosPS;
    vec3 cLightDirPS;
    vec4 cNormalOffsetScalePS;
    vec4 cShadowCubeAdjust;
    vec4 cShadowDepthFade;
    vec2 cShadowIntensity;
    vec2 cShadowMapInvSize;
    vec4 cShadowSplits;
    mat4 cLightMatricesPS[4];
#ifdef VSM_SHADOW
    vec2 cVSMShadowParams;
#endif
#ifdef PBR
    float cLightRad;
    float cLightLength;
#endif
};

#ifndef CUSTOM_MATERIAL_CBUFFER
uniform MaterialPS
{
    vec4 cMatDiffColor;
    vec3 cMatEmissiveColor;
    vec3 cMatEnvMapColor;
    vec4 cMatSpecColor;
#ifdef PBR
    float cRoughness;
    float cMetallic;
#endif
};
#endif

#endif

#endif
````

## 附录 D 内嵌 GLSL 原文

这些是 DLL/EXE 中以明文保存的 shader 字符串。原始压缩排版保持不变，个别代码块只有两行长行；代码体未改写。

### D01 顶点 GLSL

原始位置：`LayoutBaseUI.dll @ 0x13770c0 (.rdata, RVA 0x13782c0)`。

SHA256：`05dfb4fa00a399e63c573f498c6208e4fac26bb0d581b28fcc8d0097463da4da`。

````glsl
#version 330 core
in vec3 pos;in vec2 snorm;in vec3 tm1;in vec3 tm2;in vec2 tm3;uniform mat4 proj;uniform mat4 modelview;uniform float elev;uniform bool use2d;uniform bool wire;uniform bool instdraw;uniform bool instdraw1;uniform vec3 surfnorm;uniform int clipcnt;uniform vec4 clipplane[6];uniform vec4 color;out vec3 vVaryingNormal;out vec4 v_Color;out float iswire;void main(){v_Color = color;float x = pos.x;float y = pos.y;if(instdraw){    x = pos.x*tm1.x + pos.y*tm1.y + tm1.z;    y = pos.x*tm2.x + pos.y*tm2.y + tm2.z;}if(instdraw1){    x = pos.x + tm3.x;    y = pos.y + tm3.y;}vec4 lp = vec4(x, y, pos.z, 1.0);iswire = 0;if(wire){    iswire = 1;}if(use2d){    lp.z = elev;    gl_Position = proj * modelview * lp;    if(!wire)        vVaryingNormal = mat3(modelview) * surfnorm;}else{    gl_Position = proj * modelview * lp;    if(!wire)        vVaryingNormal = mat3(modelview) * vec3(snorm, 0);}for ( int i=0; i<clipcnt; i++ )    gl_ClipDistance[i] = dot(lp, clipplane[i]);}
````

### D02 顶点 GLSL

原始位置：`LayoutBaseUI.dll @ 0x13774a0 (.rdata, RVA 0x13786a0)`。

SHA256：`9c05bd46bf7987f83987cd2ed58338e43e4b091ca2184f18670c9772824ae1b8`。

````glsl
#version 330 core
in vec3 pos;in vec2 snorm;in vec3 tm1;in vec3 tm2;in vec2 tm3;uniform mat4 proj;uniform mat4 modelview;uniform float elev;uniform bool use2d;uniform bool wire;uniform bool instdraw;uniform bool instdraw1;uniform vec3 surfnorm;uniform int clipcnt;uniform vec4 clipplane[6];uniform vec4 color;out vec3 vVaryingNormal;out vec4 v_Color;out float iswire;void main(){v_Color = color;float x = pos.x;float y = pos.y;if(instdraw){    x = pos.x*tm1.x + pos.y*tm1.y + tm1.z;    y = pos.x*tm2.x + pos.y*tm2.y + tm2.z;}if(instdraw1){    x = pos.x + tm3.x;    y = pos.y + tm3.y;}vec4 lp = vec4(x, y, pos.z, 1.0);iswire = 0;if(wire){    iswire = 1;}if(use2d){    lp.z = elev;    gl_Position = proj * modelview * lp;    if(!wire)        vVaryingNormal = mat3(modelview) * surfnorm;}else{    gl_Position = proj * modelview * lp;    if(!wire)        vVaryingNormal = mat3(modelview) * vec3(snorm, 0);}if ( clipcnt>0 ){    gl_ClipDistance[0] = dot(lp, clipplane[0]);    if ( clipcnt>2 )    {        gl_ClipDistance[1] = dot(lp, clipplane[1]);        gl_ClipDistance[2] = dot(lp, clipplane[2]);        if ( clipcnt > 4 )        {            gl_ClipDistance[3] = dot(lp, clipplane[3]);            gl_ClipDistance[4] = dot(lp, clipplane[4]);            if ( clipcnt > 5 )                gl_ClipDistance[5] = dot(lp, clipplane[5]);        }        else        {            if ( clipcnt > 3 )                gl_ClipDistance[3] = dot(lp, clipplane[3]);        }    }    else    {        if ( clipcnt > 1 )        {            gl_ClipDistance[1] = dot(lp, clipplane[1]);        }    }}}
````

### D03 片段/其他 GLSL

原始位置：`LayoutBaseUI.dll @ 0x1377ad0 (.rdata, RVA 0x1378cd0)`。

SHA256：`2d2faa4b745488b8ea1067cdfdd704da1049678527dc71411acbdf0afd1993f0`。

````glsl
#version 330 core
out vec4 outColor;in float iswire;in vec3 vVaryingNormal;in vec4 v_Color;void main(){   if(iswire>0.5)   {       outColor = v_Color;   }   else   {       vec3 lightDir = vec3(0.2, 0.1, 1.0);       float diff = max(dot(normalize(vVaryingNormal), normalize(lightDir)), 0.4);       outColor = vec4(diff*vec3(v_Color), v_Color[3]);   }}
````

### D04 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x292c80 (.rdata, RVA 0x294280)；siwave.exe @ 0x1dd6fe0 (.rdata, RVA 0x1dd7de0)`。

SHA256：`4d791efc900a4c35d49085ba91145934e74f5d63ea7a0db765af25e1c9b0eddd`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
//#extension GL_EXT_clip_cull_distance : enable
layout (location = 0) in vec3 aPos;
layout (location = 1) in vec3 aNormal;


out VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} vs_out;
out vec3 ex_lightDir;

uniform mat4 model;
uniform mat4 view;
uniform mat4 projection;
uniform mat4 lightSpaceMatrix;
uniform vec3 eColor;
uniform vec3 lightDirection;

uniform vec4 plane1; //= vec4(0.0, 0.0, -1.0, 10.0);
uniform vec4 plane2; //= vec4(0.0, 0.0, 1.0, 10.0);

vec4 GetColorHeatMap(float val, float maxZVal)
{
    float t = val / maxZVal;

    float r = 0.0;
    float g = 0.0;
    float b = 0.0;
    if (t > 0.75) {
        // yellow to red
        r = 1.0;
        g = 4.0 - 4.0*t;
        b = 0.0;
    }
    else if (t > 0.5 && t <= 0.75) {
        // green to yellow
        r = 4.0*t - 2.0;
        g = 1.0;
        b = 0.0;
    }
    else if (t > 0.25 && t <= 0.5) {
        // cyan to green
        r = 0.0;
        g = 1.0;
        b = 2.0 - 4.0*t;
    }
    else if (t <= 0.25) {
        // blue to cyan
        r = 0.0;
        g = 4.0*t;
        b = 1.0;
    }

    vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

void main()
{
	gl_Position = projection * view * model * vec4(aPos, 1.0f);
	gl_ClipDistance[0] = dot(plane1, vec4(aPos, 1.0));
	gl_ClipDistance[1] = dot(plane2, vec4(aPos, 1.0));
	if(eColor.z == 5.0)
		vs_out.ex_Color = GetColorHeatMap(eColor.x, eColor.y);
	else
		vs_out.ex_Color = vec4(eColor, 1.0);
	vs_out.ex_FragPosLightSpace = lightSpaceMatrix * model * vec4(aPos, 1.0f);
	vs_out.ex_Normal = vec3( model * vec4(aNormal, 0.0) );
	vs_out.ex_FragPos = vec3( model * vec4(aPos, 1.0) );
	ex_lightDir = lightDirection;
}
````

### D05 片段/其他 GLSL

原始位置：`SharedUIDLL.dll @ 0x293400 (.rdata, RVA 0x294a00)；siwave.exe @ 0x1dd7760 (.rdata, RVA 0x1dd8560)`。

SHA256：`a5c94343892738f8cc98c5abd381ecc74a59701da9411b424042fad7a4ceb2a0`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
out vec4 FragColor;

in VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} fs_in;
in vec3 ex_lightDir;
uniform vec3 viewPos;
uniform vec4 ambientLight;
uniform int isInCanvas;// = 0;
uniform sampler2D shadowMap;
uniform int isShadowOn;

float ShadowCalculation(vec4 fragPosLightSpace)
{
	// perform perspective divide
    vec3 projCoords = fragPosLightSpace.xyz / fragPosLightSpace.w;
    // transform to [0,1] range
    projCoords = projCoords * 0.5 + 0.5;
    // get closest depth value from light's perspective (using [0,1] range fragPosLight as coords)
    float closestDepth = texture(shadowMap, projCoords.xy).r; 
    // get depth of current fragment from light's perspective
    float currentDepth = projCoords.z;
    // check whether current frag pos is in shadow
	vec3 normal = normalize(fs_in.ex_Normal);
	vec3 lightDir = normalize(viewPos - fs_in.ex_FragPos);
	float bias = 0.005; 
	//float shadow = currentDepth - bias > closestDepth  ? 1.0 : 0.0;  
	float shadow = 0.0;
	ivec2 tSize = textureSize(shadowMap, 0);
    vec2 texelSize = 1.0 / vec2(float(tSize.x), float(tSize.y));
    for(int x = -1; x <= 1; ++x)
    {
        for(int y = -1; y <= 1; ++y)
        {
            float pcfDepth = texture(shadowMap, projCoords.xy + vec2(float(x), float(y)) * texelSize).r; 
            shadow += currentDepth - bias > pcfDepth  ? 1.0 : 0.0;        
        }    
    }
    shadow /= 9.0;
    
    // keep the shadow at 0.0 when outside the far_plane region of the light's frustum.
    if(projCoords.z > 1.0)
        shadow = 0.0;
        
    return shadow;
}


void main()
{
    float shadow = ShadowCalculation(fs_in.ex_FragPosLightSpace);
	if(isInCanvas == 1)
       shadow = 0.0f;
	shadow *= 0.6f;
    if(isShadowOn == 0)
		shadow = 0.0f;	
	FragColor = fs_in.ex_Color * ambientLight + vec4((1.0 - shadow)) * fs_in.ex_Color;
		
}
````

### D06 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x293c10 (.rdata, RVA 0x295210)；siwave.exe @ 0x1dd7f70 (.rdata, RVA 0x1dd8d70)`。

SHA256：`2ab434a7669e40b9fc8b72ec359ae7bd21f3de21749a23fce56dfe54c7786307`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
//#extension GL_EXT_clip_cull_distance : enable
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
uniform mat4 lightSpaceMatrix;
uniform vec3 lightDirection;
layout (location = 0) in vec3 in_Position;
layout (location = 1) in vec3 in_Normal;
layout (location = 2) in vec3 in_Color;
//out vec4 ex_Color;
//out vec3 ex_Normal;
//out vec3 ex_FragPos;
out vec3 ex_lightDir;
out VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} vs_out;
uniform vec4 bar_Color;

uniform vec4 plane1; //= vec4(0.0, 0.0, -1.0, 10.0);
uniform vec4 plane2; //= vec4(0.0, 0.0, 1.0, 10.0);
vec4 GetColorGradient(float val)
{
	float t = val /  0.65;

	float r = 0.0;
	float g = 0.0;
	float b = 0.0;
	if ( t > 0.75 ) {
	// yellow to red
	r = 1.0;
	g = 4.0 - 4.0*t;
	b = 0.0;
	}
	else if ( t > 0.5 && t <= 0.75 ) {
	// green to yellow
	r = 4.0*t - 2.0;
	g = 1.0;
	b = 0.0;
	}
	else if ( t > 0.25 && t <= 0.5 ) {
	// cyan to green
	r = 0.0;
	g = 1.0;
	b = 2.0 - 4.0*t;
	}
	else if ( t <= 0.25 ){
	// blue to cyan
	r = 0.0;
	g = 4.0*t;
	b = 1.0;
	}
   vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

vec4 GetColorHeatMap(float val, float maxZVal)
{
    float t = val / maxZVal;

    float r = 0.0;
    float g = 0.0;
    float b = 0.0;
    if (t > 0.75) {
        // yellow to red
        r = 1.0;
        g = 4.0 - 4.0*t;
        b = 0.0;
    }
    else if (t > 0.5 && t <= 0.75) {
        // green to yellow
        r = 4.0*t - 2.0;
        g = 1.0;
        b = 0.0;
    }
    else if (t > 0.25 && t <= 0.5) {
        // cyan to green
        r = 0.0;
        g = 1.0;
        b = 2.0 - 4.0*t;
    }
    else if (t <= 0.25) {
        // blue to cyan
        r = 0.0;
        g = 4.0*t;
        b = 1.0;
    }

    vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

void main()
{
    
    gl_Position =    projectionMatrix * viewMatrix * modelMatrix * vec4(in_Position, 1.0);
	gl_ClipDistance[0] = dot(plane1, vec4(in_Position, 1.0));
	gl_ClipDistance[1] = dot(plane2, vec4(in_Position, 1.0));
    vs_out.ex_FragPos = vec3( modelMatrix * vec4(in_Position, 1.0) );
    if(bar_Color.x == 10.0 && bar_Color.y == 10.0 && bar_Color.z == 10.0){
       vs_out.ex_Color = GetColorGradient(in_Position.y);
     }else if( bar_Color.z == 5.0 ){
	   vs_out.ex_Color = GetColorHeatMap(bar_Color.x, bar_Color.y);
	}else
	{
	   vs_out.ex_Color = bar_Color;
	}
    vs_out.ex_Normal = vec3( modelMatrix * vec4(in_Normal, 0.0) );
    vs_out.ex_FragPosLightSpace = lightSpaceMatrix * vec4(vs_out.ex_FragPos, 1.0);
	 ex_lightDir = lightDirection;
    //vec3( modelMatrix * vec4(lightDirection, 1.0) );
}
````

### D07 片段/其他 GLSL

原始位置：`SharedUIDLL.dll @ 0x294760 (.rdata, RVA 0x295d60)；siwave.exe @ 0x1dd8ac0 (.rdata, RVA 0x1dd98c0)`。

SHA256：`509cef63028756600edd5b490b3a96b44163f0e37de113d43a4bbb70e529161b`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//in vec4 ex_Color;
//in vec3 ex_Normal;
//in vec3 ex_FragPos;
//#extension GL_OES_shader_io_blocks: require
in vec3 ex_lightDir;
in VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} fs_in;

out vec4 out_Color;

struct Material {
vec4 ambient;
vec4 diffuse;
vec4 specular;
float shininess;
}; 

struct Light {
vec3 direction;
vec4 ambient;
vec4 diffuse;
vec4 specular;
};

uniform vec3 viewPos;
//uniform vec4 checkColor;
uniform Material material1;
uniform Light light;
uniform sampler2D shadowMap;
uniform int isShadowOn;

Material material;

uniform int isbkgPlane;// = 0;
uniform int isInCanvas;// = 0;


float ShadowCalculation(vec4 fragPosLightSpace)
{
	// perform perspective divide
    vec3 projCoords = fragPosLightSpace.xyz / fragPosLightSpace.w;
    // transform to [0,1] range
    projCoords = projCoords * 0.5 + 0.5;
    // get closest depth value from light's perspective (using [0,1] range fragPosLight as coords)
    float closestDepth = texture(shadowMap, projCoords.xy).r; 
    // get depth of current fragment from light's perspective
    float currentDepth = projCoords.z;
    // check whether current frag pos is in shadow
	vec3 normal = normalize(fs_in.ex_Normal);
	vec3 lightDir = normalize(viewPos - fs_in.ex_FragPos);
	float bias = 0.005; 
	//float shadow = currentDepth - bias > closestDepth  ? 1.0 : 0.0;  
	float shadow = 0.0;
	ivec2 tSize = textureSize(shadowMap, 0);
    vec2 texelSize = 1.0 / vec2(float(tSize.x), float(tSize.y));
    for(int x = -1; x <= 1; ++x)
    {
        for(int y = -1; y <= 1; ++y)
        {
            float pcfDepth = texture(shadowMap, projCoords.xy + vec2(float(x), float(y)) * texelSize).r; 
            shadow += currentDepth - bias > pcfDepth  ? 1.0 : 0.0;        
        }    
    }
    shadow /= 9.0;
    
    // keep the shadow at 0.0 when outside the far_plane region of the light's frustum.
    if(projCoords.z > 1.0)
        shadow = 0.0;
        
    return shadow;
}
void main()
{
   material.ambient = fs_in.ex_Color;
   material.diffuse = fs_in.ex_Color;

   material.specular = material1.specular;
   material.shininess = material1.shininess;
   vec4 ambientColor = light.ambient * material.ambient;
	  
	 vec3 normal = normalize(fs_in.ex_Normal);
   //vec3 lightDir = normalize(light.position - fs_in.ex_FragPos);
	 vec3 lightDir = normalize(-ex_lightDir);
    vec4 diffuseColor = light.diffuse * (max(dot(lightDir, normal), 0.0) * material.diffuse);

    vec3 viewDir = normalize(viewPos - fs_in.ex_FragPos);
    vec3 reflectDir = reflect(-lightDir, normal);
    vec4 specularColor = light.specular * ( pow(max(dot(viewDir, reflectDir), 0.0), material.shininess) * material.specular );
    // calculate shadow
    float shadow = ShadowCalculation(fs_in.ex_FragPosLightSpace);
	if(isInCanvas == 1)
       shadow = 0.0f;	
	shadow *= 0.6f;
	if(isShadowOn == 0)
		shadow = 0.0f;
	if(isbkgPlane == 1)
	{
		out_Color = (ambientColor + (1.0f - shadow) * fs_in.ex_Color);		
	    out_Color.a = 1.0;
	}else{
        out_Color = (ambientColor + (1.0f - shadow) * (diffuseColor + specularColor));		
	    out_Color.a = 1.0;
	}

}
````

### D08 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x295430 (.rdata, RVA 0x296a30)；siwave.exe @ 0x1dd9790 (.rdata, RVA 0x1dda590)`。

SHA256：`87e1d2fa536bced9f9c80124e36f6a4967e02b81901e9c6c820227bc9875d1b7`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 vertex; // <vec2 pos, vec2 tex>
layout (location = 1) in vec2 tex; // <vec2 pos, vec2 tex>
out vec2 TexCoords;

uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;

void main()
{
	gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(vertex, 1.0);
	TexCoords = tex;
}
````

### D09 片段/其他 GLSL

原始位置：`SharedUIDLL.dll @ 0x295630 (.rdata, RVA 0x296c30)；siwave.exe @ 0x1bf9b90 (.rdata, RVA 0x1bfa990)；siwave.exe @ 0x1dd9990 (.rdata, RVA 0x1dda790)`。

SHA256：`bd42a727794327488adcaaae8b62e5821e35325bdd8848821ae53eda5390b0b2`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
in vec2 TexCoords;
out vec4 ex_color;

uniform sampler2D text;
uniform vec3 textColor;

void main()
{    
    vec4 sampled = vec4(1.0, 1.0, 1.0, texture(text, TexCoords).r);
    ex_color = vec4(textColor, 1.0) * sampled;
}
````

### D10 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x2957c0 (.rdata, RVA 0x296dc0)；siwave.exe @ 0x1dd9b20 (.rdata, RVA 0x1dda920)`。

SHA256：`f55010a6e9656c9b27f3506dcbbabab01efffef875f2896c227d11e3deadf8b4`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 vertex; // <vec2 pos, vec2 tex>

uniform mat4 lightSpaceMatrix;
uniform mat4 modelMatrix;

void main()
{
    gl_Position = lightSpaceMatrix * modelMatrix * vec4(vertex, 1.0);
}
````

### D11 片段/其他 GLSL

原始位置：`SharedUIDLL.dll @ 0x295940 (.rdata, RVA 0x296f40)；siwave.exe @ 0x1dd9ca0 (.rdata, RVA 0x1ddaaa0)；StackupCalc.dll @ 0x40ef60 (.rdata, RVA 0x410560)`。

SHA256：`f2e26c061ea20aabbc6530003bff5142de133bfddc3d3dce5873a97fac6e8d31`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
void main()
{    

}
````

### D12 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x295a00 (.rdata, RVA 0x297000)；siwave.exe @ 0x1dd9d60 (.rdata, RVA 0x1ddab60)`。

SHA256：`b33c1c6d5b15ab2254e2e8ed86f6e225ac33c2f79929ad2effa3df555f1f968d`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 in_Position;
out vec4 ex_Color;
uniform vec3 scaleData; //x:startPt, y:length, z:axistype(1:xasis,2:yaxis)
vec4 GetColor(float val)
{
	float t = (val - scaleData.x) /  scaleData.y;

	float r = 0.0;
	float g = 0.0;
	float b = 0.0;
	if ( t > 0.75 ) {
	// yellow to red
	r = 1.0;
	g = 4.0 - 4.0*t;
	b = 0.0;
	}
	else if ( t > 0.5 && t <= 0.75 ) {
	// green to yellow
	r = 4.0*t - 2.0;
	g = 1.0;
	b = 0.0;
	}
	else if ( t > 0.25 && t <= 0.5 ) {
	// cyan to green
	r = 0.0;
	g = 1.0;
	b = 2.0 - 4.0*t;
	}
	else if ( t <= 0.25 ){
	// blue to cyan
	r = 0.0;
	g = 4.0*t;
	b = 1.0;
	}
   vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

void main()
{
    gl_Position = vec4(in_Position, 1.0);
	if(scaleData.z == 1.0)
      ex_Color = GetColor(in_Position.x);
	else
	  ex_Color = GetColor(in_Position.y);
}
````

### D13 片段/其他 GLSL

原始位置：`SharedUIDLL.dll @ 0x295e10 (.rdata, RVA 0x297410)；siwave.exe @ 0x1dda170 (.rdata, RVA 0x1ddaf70)；StackupCalc.dll @ 0x40f1c0 (.rdata, RVA 0x4107c0)`。

SHA256：`f675869919e96233b26c2fe86064ad68f60c8501a69557462a7c105c73f3e3e3`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
out vec4 FragColor;
in vec4 ex_Color;
void main()
{    
 FragColor = ex_Color;
}
````

### D14 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x295f10 (.rdata, RVA 0x297510)；siwave.exe @ 0x1dda580 (.rdata, RVA 0x1ddb380)`。

SHA256：`998a316e83949af55c81e56f0fe391d2f992d3c754032d0865e8a1f81040b1f6`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 in_Position;
out vec4 ex_Color;
uniform vec3 scaleData; //x:startPt, y:length, z:axistype(1:xasis,2:yaxis)
vec4 GetColor(float val)
{
	float t = (val - scaleData.x) /  scaleData.y;

	float r = 1.0 - 0.2*t;
	float g = 1.0 - 0.2*t;
	float b = 1.0 - 0.2*t;
   vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

void main()
{
    gl_Position = vec4(in_Position, 1.0);
	if(scaleData.z == 1.0)
      ex_Color = GetColor(in_Position.x);
	else
	  ex_Color = GetColor(in_Position.y);
}
````

### D15 顶点 GLSL

原始位置：`SharedUIDLL.dll @ 0x2961e0 (.rdata, RVA 0x2977e0)；siwave.exe @ 0x1dda850 (.rdata, RVA 0x1ddb650)`。

SHA256：`c217a03244852c8b43ee0e0ac6cca1fa1b3aef3fb2ddf29685554029ab51abc5`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 vertex; // <vec2 pos, vec2 tex>
layout (location = 1) in vec2 tex; // <vec2 pos, vec2 tex>
out vec2 TexCoords;


void main()
{
	gl_Position = vec4(vertex, 1.0);
	TexCoords = tex;
}
````

### D16 顶点 GLSL

原始位置：`siwave.exe @ 0x1bf8a60 (.rdata, RVA 0x1bf9860)`。

SHA256：`657a0b923e1d744465bd96e0000aec135f5a234caad53ef7e49eebb119a51b09`。

````glsl
#version 330 core
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 rotMatrix;
uniform vec4 viewPort;
uniform vec3 viewCenter;
uniform vec3 lightDirection;
layout (location = 0) in vec3 in_Position;
layout (location = 1) in vec4 in_Color;
layout (location = 2) in vec3 in_Normal;
//out vec4 ex_Color;
//out vec3 ex_Normal;
//out vec3 ex_FragPos;
out vec3 ex_lightDir;
out VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} vs_out;

uniform int faceIndex; //Top: 0, Bottom: 1, Side: 2

uniform int clipFlag;
uniform vec4 clipPlane1; //= vec4(0.0, 0.0, -1.0, 10.0);
uniform vec4 clipPlane2; //= vec4(0.0, 0.0, 1.0, 10.0);

uniform int crossSectionFlag;
uniform vec4 crossSectionPlane;

uniform float zScalingFactor;
uniform float zShiftFactor;

void main()
{
	vec4 tempPos = vec4( in_Position, 1.0 );

	if( zScalingFactor > 1.0 )
      tempPos.z = (tempPos.z - viewCenter.z) * zScalingFactor + viewCenter.z;
	
	if( zShiftFactor != 1.0 )
      tempPos.z = tempPos.z + zShiftFactor;

	if( clipFlag == 1 ) {
	  gl_ClipDistance[0] = dot(clipPlane1, tempPos);
	  gl_ClipDistance[1] = dot(clipPlane2, tempPos);
	}

	if( crossSectionFlag == 1 )
	{
	  gl_ClipDistance[2] = dot(crossSectionPlane, tempPos);
	}

	vec4 tempCtr = vec4( viewCenter, 1.0 );

	tempPos = tempPos - tempCtr;
	tempPos = rotMatrix * tempPos;
	tempPos = tempPos + tempCtr;

    gl_Position = projectionMatrix * modelViewMatrix * tempPos;

	vs_out.ex_FragPos = vec3( modelViewMatrix * tempPos );

	vs_out.ex_Color = in_Color;

    if( faceIndex == 0 )
      vs_out.ex_Normal = vec3( modelViewMatrix * rotMatrix * vec4(0.0, 0.0, 1.0, 1.0) );
    else if( faceIndex == 1 )
	  vs_out.ex_Normal = vec3( modelViewMatrix * rotMatrix * vec4(0.0, 0.0, -1.0, 1.0) );
    else
	  vs_out.ex_Normal = vec3( modelViewMatrix * rotMatrix * vec4(in_Normal, 1.0) );

	ex_lightDir = lightDirection;
}
````

### D17 片段/其他 GLSL

原始位置：`siwave.exe @ 0x1bf91e0 (.rdata, RVA 0x1bf9fe0)`。

SHA256：`1fe445484606fd9178374afd34584705efe181a821f82dea4d7934edb7422dde`。

````glsl
#version 330 core
//in vec4 ex_Color;
in vec3 ex_lightDir;

in VS_OUT {
  vec3 ex_FragPos;
  vec3 ex_Normal;
  vec4 ex_Color;
  vec4 ex_FragPosLightSpace;
} fs_in;

uniform vec4 layerColor;
uniform float layerTranslucency;

out vec4 out_Color;

struct Material {
  vec4 ambient;
  vec4 diffuse;
  vec4 specular;
  float shininess;
}; 

struct Light {
  vec3 direction;
  vec4 ambient;
  vec4 diffuse;
  vec4 specular;
};

uniform vec3 viewPos;
uniform Light light;

uniform Material material;

uniform int sceneRotatedFlag;
uniform int lightingModeFlag;
uniform int vtxColorModeFlag;

void main()
{
  if( vtxColorModeFlag == 1 ) {
    out_Color = fs_in.ex_Color;
	out_Color.a = layerTranslucency;
    return;
  }

  if( lightingModeFlag == 0 ) {
    out_Color = material.ambient;
	out_Color.a = layerTranslucency;
    return;
  }

  vec4 ambientColor = light.ambient * material.ambient;
	  
  vec3 normal = normalize(fs_in.ex_Normal);
  vec3 lightDir = normalize(-ex_lightDir);
  vec4 diffuseColor = light.diffuse * (max(dot(lightDir, normal), 0.0) * material.diffuse);

  vec3 viewDir = normalize(viewPos - fs_in.ex_FragPos);
  vec3 reflectDir = reflect(-lightDir, normal);
  vec4 specularColor = light.specular * ( pow(max(dot(viewDir, reflectDir), 0.0), material.shininess) * material.specular );

  out_Color = ambientColor + diffuseColor + specularColor;
  out_Color.a = layerTranslucency;
}
````

### D18 顶点 GLSL

原始位置：`siwave.exe @ 0x1bf9780 (.rdata, RVA 0x1bfa580)`。

SHA256：`0d042c84fe20081b18c8c5967591f9f7fae8943f1312bc8063122d3f3e670f9c`。

````glsl
#version 330 core
uniform mat4 projectionMatrix;
layout (location = 0) in vec3 in_Position;
layout (location = 1) in vec4 in_Color;
layout (location = 2) in vec3 in_Normal;
out vec4 ex_Color;

void main()
{
  gl_Position = projectionMatrix * vec4(in_Position, 1.0);
  ex_Color = in_Color;
}
````

### D19 片段/其他 GLSL

原始位置：`siwave.exe @ 0x1bf98e0 (.rdata, RVA 0x1bfa6e0)`。

SHA256：`8ad6c2dfe94e831873710d378e4baf51ca6a5ec72be6a4edafe22100a3f058d2`。

````glsl
#version 330 core
in vec4 ex_Color;
out vec4 out_Color;

void main()
{
  out_Color = ex_Color;
}
````

### D20 顶点 GLSL

原始位置：`siwave.exe @ 0x1bf9970 (.rdata, RVA 0x1bfa770)`。

SHA256：`9c67a9419ff7e5d5b1041e9e708b130b33bb7f3a050bd366429105af8eee38b0`。

````glsl
#version 330 core
uniform mat4 modelViewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 rotMatrix;
uniform vec3 viewCenter;
layout (location = 0) in vec3 vertex; // <vec2 pos, vec2 tex>
layout (location = 1) in vec2 tex; // <vec2 pos, vec2 tex>
out vec2 TexCoords;

void main()
{
	vec4 tempPos = vec4( vertex, 1.0 );
	vec4 tempCtr = vec4( viewCenter, 1.0 );

	tempPos = tempPos - tempCtr;
	tempPos = rotMatrix * tempPos;
	tempPos = tempPos + tempCtr;

    gl_Position = projectionMatrix * modelViewMatrix * tempPos;
	TexCoords = tex;
}
````

### D21 顶点 GLSL

原始位置：`siwave.exe @ 0x1dda270 (.rdata, RVA 0x1ddb070)`。

SHA256：`d6b73a20bc5fc0fdd61ffdb9d4bb33f55ac2ef18933a2d8ce2db1c8e57765b01`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 aPos;
layout (location = 1) in vec2 aTexCoords;

out vec2 TexCoords;

uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;

void main()
{
    TexCoords = aTexCoords;
    gl_Position = vec4(aPos, 1.0);
}
````

### D22 片段/其他 GLSL

原始位置：`siwave.exe @ 0x1dda430 (.rdata, RVA 0x1ddb230)`。

SHA256：`dbe8b75799fbcb1d6c05ea550e60a02367060f246501f9d9abbdd4c0f963d05d`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
out vec4 FragColor;

in vec2 TexCoords;

uniform sampler2D textureMap;



void main()
{             
    FragColor = texture(textureMap, TexCoords);
}
````

### D23 片段/其他 GLSL

原始位置：`StackupCalc.dll @ 0x40d740 (.rdata, RVA 0x40ed40)`。

SHA256：`3b7e6c08614a5974fbdd540c2eef106a9ff55111b232e690feeab29535fd98c8`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//in vec4 ex_Color;
//in vec3 ex_Normal;
//in vec3 ex_FragPos;
//#extension GL_OES_shader_io_blocks: require
in  vec3 ex_lightDir;
in VS_OUT {
     vec3 ex_FragPos;
     vec3 ex_Normal;
     vec4 ex_Color;
     vec4 ex_FragPosLightSpace;
} fs_in;

out  vec4 out_Color;

struct Material {
 vec4 ambient;
 vec4 diffuse;
 vec4 specular;
 float shininess;
}; 

struct Light {
 vec3 direction;
 vec4 ambient;
 vec4 diffuse;
 vec4 specular;
};

uniform  vec3 viewPos;
//uniform vec4 checkColor;
uniform Material material;
uniform Light light;
uniform  sampler2D shadowMap;
uniform int isShadowOn;

 float ShadowCalculation(vec4 fragPosLightSpace)
{
// perform perspective divide
     vec3 projCoords = fragPosLightSpace.xyz / fragPosLightSpace.w;
    // transform to [0,1] range
    projCoords = projCoords * 0.5 + 0.5;
    // get closest depth value from light's perspective (using [0,1] range fragPosLight as coords)
     float closestDepth = texture(shadowMap, projCoords.xy).r; 
    // get depth of current fragment from light's perspective
     float currentDepth = projCoords.z;
   // check whether current frag pos is in shadow
     float bias = 0.005;
    //float shadow = currentDepth - bias > closestDepth  ? 1.0 : 0.0; 
	 float shadow = 0.0;
	 ivec2 tSize = textureSize(shadowMap, 0);
	 vec2 texelSize = 1.0 / vec2(float(tSize.x), float(tSize.y));
	for(int x = -1; x <= 1; ++x)
	{
		for(int y = -1; y <= 1; ++y)
		{
			 float pcfDepth = texture(shadowMap, projCoords.xy + vec2(float(x), float(y)) * texelSize).r; 
			shadow += currentDepth - bias > pcfDepth ? 1.0 : 0.0;        
		}    
	}
	shadow /= 9.0;
	
    if(projCoords.z > 1.0)
        shadow = 0.0;
  
    return shadow;
}
void main()
{
    vec4 ambientColor = light.ambient * material.ambient;
	  
	  vec3 normal = normalize(fs_in.ex_Normal);
   //vec3 lightDir = normalize(light.position - fs_in.ex_FragPos);
	  vec3 lightDir = normalize(-ex_lightDir);
     vec4 diffuseColor = light.diffuse * (max(dot(lightDir, normal), 0.0) * material.diffuse);

     vec3 viewDir = normalize(viewPos - fs_in.ex_FragPos);
     vec3 reflectDir = reflect(-lightDir, normal);
     vec4 specularColor = light.specular * ( pow(max(dot(viewDir, reflectDir), 0.0), material.shininess) * material.specular );
    // calculate shadow
     float shadow = ShadowCalculation(fs_in.ex_FragPosLightSpace);
     shadow *= 0.6f;
     if(isShadowOn == 0)
		shadow = 0.0f;
     out_Color = (ambientColor + (1.0f - shadow) * (diffuseColor + specularColor));
	 out_Color.a = 0.9;
}
````

### D24 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40e190 (.rdata, RVA 0x40f790)`。

SHA256：`87ead94ca98d5167aa84f17758578b59fa127e749ce4dbb6e3a2e7b9c1cb2322`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
uniform mat4 lightSpaceMatrix;
uniform vec3 lightDirection;
layout (location = 0) in vec3 in_Position;
layout (location = 1) in vec4 in_Color;
layout (location = 2) in vec3 in_Normal;
//out vec4 ex_Color;
//out vec3 ex_Normal;
//out vec3 ex_FragPos;
out vec3 ex_lightDir;
out VS_OUT {
    vec3 ex_FragPos;
    vec3 ex_Normal;
    vec4 ex_Color;
    vec4 ex_FragPosLightSpace;
} vs_out;
void main()
{
    gl_Position =    projectionMatrix * viewMatrix * modelMatrix * vec4(in_Position, 1.0);
    vs_out.ex_FragPos = vec3( modelMatrix * vec4(in_Position, 1.0) );
    vs_out.ex_Color = in_Color;
    vs_out.ex_Normal = vec3( modelMatrix * vec4(in_Normal, 0.0) );
    vs_out.ex_FragPosLightSpace = lightSpaceMatrix * vec4(vs_out.ex_FragPos, 1.0);
	 ex_lightDir = lightDirection;
    //vec3( modelMatrix * vec4(lightDirection, 1.0) );
}
````

### D25 片段/其他 GLSL

原始位置：`StackupCalc.dll @ 0x40e5d0 (.rdata, RVA 0x40fbd0)`。

SHA256：`35f67aa308e0aca9c320a61038a17abecfa2235afa772808cf4e0416ec3149b3`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
in vec4 ex_Color;
uniform vec4 layerColor;
out vec4 out_Color;
void main()
{
    out_Color = ex_Color;
	out_Color.a = 1.0;
   //this is to test if its compiled into the binary
}
````

### D26 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40e730 (.rdata, RVA 0x40fd30)`。

SHA256：`fe21e251ca71b043f554238c79a3aad991c670c01b1358972203e919b036bac4`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 in_Position;
layout (location = 1) in vec4 in_Color;
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
out vec4 ex_Color;
void main()
{
    gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(in_Position, 1.0);
	ex_Color = in_Color;
}
````

### D27 片段/其他 GLSL

原始位置：`StackupCalc.dll @ 0x40e920 (.rdata, RVA 0x40ff20)`。

SHA256：`e963ade1662df47bffc022b39b6687de7621502c14fdd6fa9283f098ad4125da`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
in vec2 TexCoords;
out vec4 ex_color;

uniform sampler2D text;
uniform vec3 textColor;

void main()
{    
     vec4 sampled = vec4(1.0, 1.0, 1.0, texture(text, TexCoords).r);
    ex_color = vec4(textColor, 1.0) * sampled;
}
````

### D28 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40eab0 (.rdata, RVA 0x4100b0)`。

SHA256：`d6f6878213bdacb3303464c68fc08b59814c1bfbac5cecff5ae8c38c5580d9c2`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec4 vertex; // <vec2 pos, vec2 tex>
out vec2 TexCoords;

uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
uniform float zLocation;

void main()
{
    gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(vertex.xy, zLocation, 1.0);
    TexCoords = vertex.zw;
}
````

### D29 片段/其他 GLSL

原始位置：`StackupCalc.dll @ 0x40ecb0 (.rdata, RVA 0x4102b0)`。

SHA256：`7d5e97d8737334d93a9dd4b644d1330d55b6f000893a4ae26f0006e59ac48af6`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
uniform vec4 layerColor;
out vec4 out_Color;
void main()
{
    out_Color = vec4(1.0, 1.0, 1.0, 1.0);
}
````

### D30 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40edd0 (.rdata, RVA 0x4103d0)`。

SHA256：`6511bdc0df7aab31e546cb82cdfb7a35668ef115e05a7ce387c099797ddd0d4e`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
uniform mat4 projectionMatrix;
uniform mat4 viewMatrix;
uniform mat4 modelMatrix;
layout (location = 0) in vec3 in_Position;
void main()
{
    gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(in_Position, 1.0);
}
````

### D31 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40f020 (.rdata, RVA 0x410620)`。

SHA256：`698e9ebae75443260e0a5fe766ada69bf6963d51b98193ece96283457e28089e`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 vertex; // <vec2 pos, vec2 tex>

uniform mat4 projectionMatrix;
uniform mat4 modelMatrix;
uniform mat4 viewMatrix;
void main()
{
    gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(vertex, 1.0);
}
````

### D32 顶点 GLSL

原始位置：`StackupCalc.dll @ 0x40f2c0 (.rdata, RVA 0x4108c0)`。

SHA256：`e0c9359a84bd5e9463975666d713a2f72a0c24eb1d0d613bd02e1f78909eb9ad`。

````glsl
#version 330 core
//#extension GL_ARB_explicit_attrib_location: enable
//#extension GL_ARB_explicit_uniform_location: enable
//#extension GL_OES_shader_io_blocks: require
layout (location = 0) in vec3 in_Position;
out vec4 ex_Color;
uniform vec3 scaleData; //x:startPt, y:length, z:axistype(1:xasis,2:yaxis)
uniform float factor = 0.6;
vec4 GetColor(float val)
{
	float t = (val - scaleData.x) /  scaleData.y;

	float r = factor - factor*t;
	float g = factor - factor*t;
	float b = factor - factor*t;
   vec4 color;
	color.x = r;
	color.y = g;
	color.z = b;
	color.w = 1.0;
   return color;
}

void main()
{
    gl_Position = vec4(in_Position, 1.0);
	if(scaleData.z == 1.0)
      ex_Color = GetColor(in_Position.x);
	else
	  ex_Color = GetColor(in_Position.y);
}
````

### D33 顶点 GLSL

原始位置：`Phoenix.dll @ 0x105d0 (.rdata, RVA 0x115d0)`。

SHA256：`5d10e949913787a3119c7a7cc62eb362ce2e2cac21d2107752e3b9014c36e56f`。

````glsl
uniform mat4 uMVPMatrix;uniform mat3 uNMatrix;attribute vec3 varVertex;attribute vec3 varNormal;attribute vec4 varColor;varying vec3 position;varying vec3 normal;varying vec4 color;void main(void) {  gl_Position = uMVPMatrix * vec4(varVertex, 1.0f);  position = gl_Position.xyz;  color = varColor;  normal = normalize(uNMatrix * varNormal);}
````

### D34 片段/其他 GLSL

原始位置：`Phoenix.dll @ 0x10730 (.rdata, RVA 0x11730)`。

SHA256：`851aca4fad7fe03df790e13f03cbc55ec09eeccf87ab8b815c6807c735e9ccef`。

````glsl
uniform vec3 lightDir;varying vec3 position;varying vec3 normal;varying vec4 color;void main(void) {	vec3 L = normalize(lightDir - position);	vec3 ambientColor = vec3(0.05, 0.05, 0.05);	vec3 diffuseColor = color.rgb;	vec3 specularColor = vec3(1.0, 1.0, 1.0);	float shine = 80.0;	float lambertian = max(dot(L, normal), 0.0);	float specular = 0.0;	if (lambertian > 0.0) {		vec3 R = reflect(-L, normal);		vec3 V = normalize(-position);		float specAngle = max(dot(R, V), 0.0);		specular = pow(specAngle, shine);	}gl_FragColor = vec4(ambientColor + lambertian * diffuseColor + specular * specularColor, color.a);}
````


