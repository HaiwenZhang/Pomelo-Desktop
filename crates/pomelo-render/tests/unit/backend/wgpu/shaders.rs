use super::*;
#[test]
fn all_wgpu_shaders_validate() {
    for shader in [
        Shader::Trace,
        Shader::Pad,
        Shader::Text,
        Shader::Label,
        Shader::Copper,
        Shader::Probe,
    ] {
        validated(shader).unwrap_or_else(|e| panic!("WGSL {shader:?}: {e:#}"));
    }
}

#[test]
fn shader_instance_strides_match_uploaded_cpu_structures() {
    let cases = [
        (Shader::Trace, size_of::<crate::tracks::TraceInstance>()),
        (Shader::Pad, size_of::<crate::pads::PadInstance>()),
        (
            Shader::Text,
            size_of::<crate::text_instances::TextInstance>(),
        ),
        (Shader::Label, size_of::<crate::text::msdf::GlyphInstance>()),
        (Shader::Copper, size_of::<crate::copper::CopperVertex>()),
    ];
    for (shader, expected) in cases {
        let (module, _) = validated(shader).unwrap();
        let (_, geometry) = module
            .global_variables
            .iter()
            .find(|(_, variable)| {
                variable
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.group == 0 && binding.binding == 1)
            })
            .expect("geometry storage binding");
        let naga::TypeInner::Array { stride, .. } = module.types[geometry.ty].inner else {
            panic!("{shader:?}: geometry must be an array");
        };
        assert_eq!(
            stride as usize, expected,
            "{shader:?}: GPU/CPU instance stride"
        );
    }
}

#[test]
fn frame_opacity_follows_the_cpu_uniform_payload() {
    for (shader, payload_bytes) in [
        (Shader::Trace, 128),
        (Shader::Pad, 128),
        (Shader::Text, 128),
        (Shader::Label, 128),
        (Shader::Copper, 176),
        (Shader::Probe, 112),
    ] {
        let (module, _) = validated(shader).unwrap();
        let (_, frame) = module
            .global_variables
            .iter()
            .find(|(_, variable)| {
                variable
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.group == 0 && binding.binding == 0)
            })
            .expect("frame uniform binding");
        let naga::TypeInner::Struct { ref members, span } = module.types[frame.ty].inner else {
            panic!("{shader:?}: frame must be a struct");
        };
        let inherited = members.last().expect("opacity member");
        assert_eq!(
            (inherited.name.as_deref(), inherited.offset, span),
            (Some("inherited"), payload_bytes, payload_bytes + 16),
            "{shader:?}: driver appends opacity after the CPU payload"
        );
    }
}

#[test]
fn label_atlas_and_sampler_match_driver_bindings() {
    let (module, _) = validated(Shader::Label).unwrap();
    let mut bindings: Vec<_> = module
        .global_variables
        .iter()
        .filter_map(|(_, variable)| {
            variable
                .binding
                .as_ref()
                .map(|binding| (binding.group, binding.binding))
        })
        .collect();
    bindings.sort_unstable();
    assert_eq!(bindings, [(0, 0), (0, 1), (0, 2), (0, 3)]);
}

#[test]
fn copper_has_separate_indexed_and_clear_vertex_entry_points() {
    let (module, _) = validated(Shader::Copper).unwrap();
    let entry_points: std::collections::BTreeMap<_, _> = module
        .entry_points
        .iter()
        .map(|entry| (entry.name.as_str(), entry.stage))
        .collect();
    assert_eq!(
        entry_points,
        std::collections::BTreeMap::from([
            ("vertex_main", naga::ShaderStage::Vertex),
            ("clear_main", naga::ShaderStage::Vertex),
            ("fragment_main", naga::ShaderStage::Fragment),
        ])
    );
}
