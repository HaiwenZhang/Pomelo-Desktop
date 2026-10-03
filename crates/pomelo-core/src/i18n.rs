//! Shared translation resources. Formatting always receives an explicit locale.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// Languages shipped with the application. Source-file encoding is independent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ko")]
    Korean,
}

impl Locale {
    pub const ALL: [Self; 5] = [
        Self::English,
        Self::SimplifiedChinese,
        Self::TraditionalChinese,
        Self::Japanese,
        Self::Korean,
    ];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
            Self::Japanese => "ja",
            Self::Korean => "ko",
        }
    }

    /// Self-names remain recognizable after accidentally selecting a language.
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChinese => "繁體中文",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
        }
    }

    /// Normalize common OS locale tags, honoring script before territory.
    pub fn from_system_tag(tag: &str) -> Option<Self> {
        let normalized = tag.trim().replace('_', "-").to_ascii_lowercase();
        let normalized = normalized.split(['.', '@']).next()?;
        let mut parts = normalized.split('-');
        match parts.next()? {
            "en" => Some(Self::English),
            "ja" => Some(Self::Japanese),
            "ko" => Some(Self::Korean),
            "zh" => {
                let parts: Vec<_> = parts.collect();
                if parts.contains(&"hant") {
                    Some(Self::TraditionalChinese)
                } else if parts.contains(&"hans") {
                    Some(Self::SimplifiedChinese)
                } else if parts.iter().any(|part| matches!(*part, "tw" | "hk" | "mo")) {
                    Some(Self::TraditionalChinese)
                } else {
                    Some(Self::SimplifiedChinese)
                }
            }
            _ => None,
        }
    }
}

/// Saved preference; system selection resolves once at startup or on selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(untagged)]
    Explicit(Locale),
}

impl LanguagePreference {
    pub fn resolve(self, system_tag: Option<&str>) -> Locale {
        match self {
            Self::Explicit(locale) => locale,
            Self::System => system_tag
                .and_then(Locale::from_system_tag)
                .unwrap_or_default(),
        }
    }
}

macro_rules! message_keys {
    ($($variant:ident => $key:literal [$($arg:literal),*]),* $(,)?) => {
        /// Stable message keys and parameter schemas shared by GUI and CLI.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum MessageKey { $(#[serde(rename = $key)] $variant),* }

        impl MessageKey {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $key),* } }
            pub const fn parameters(self) -> &'static [&'static str] { match self { $(Self::$variant => &[$($arg),*]),* } }
        }
    };
}

message_keys! {
    UiNumber => "ui.number" ["value"],
    UiPercent => "ui.percent" ["value"],
    WorkspaceSummary => "ui.summary" ["layers", "components", "nets"],
    BoardFormat => "ui.board_format" [],
    ShortcutOpen => "ui.shortcut_open" [],
    ShortcutSearch => "ui.shortcut_search" [],
    ShortcutFit => "ui.shortcut_fit" [],
    AxisX => "ui.axis_x" [],
    AxisY => "ui.axis_y" [],
    ZoomPercent => "ui.zoom_percent" ["percent"],
    GpuStatus => "ui.gpu" [],
    ProductNative => "ui.native_brand" [],
    ProductViewer => "ui.viewer_brand" [],
    BrdExtension => "ui.brd_extension" [],
    AboutPomelo => "menu.about" [],

    Workspace => "ui.workspace" [],
    StartPage => "ui.start" [],
    ContinueRecent => "ui.continue" [],
    AllegroFormat => "ui.allegro_format" [],
    RecentBoardSummary => "ui.recent_board_summary" ["format", "layers"],
    RecentToday => "ui.recent_today" ["time"],
    RecentYesterday => "ui.recent_yesterday" [],
    RecentMonthDay => "ui.recent_month_day" ["month", "day"],
    RecentYearMonthDay => "ui.recent_year_month_day" ["year", "month", "day"],
    RecentTimeUnknown => "ui.recent_time_unknown" [],
    RecentPreviewPrepareFailed => "diagnostic.recent_preview_prepare_failed" [],
    RecentPreviewLoadFailed => "diagnostic.recent_preview_load_failed" [],
    ViewAllRecent => "ui.view_all_recent" [],
    LocalWorkspace => "ui.local_workspace" [],
    DropTitle => "ui.drop_title" [],
    SupportedFormats => "ui.supported_formats" [],
    PrivacyTitle => "ui.privacy_title" [],
    PrivacyDescription => "ui.privacy_description" [],
    ExploreTitle => "ui.explore_title" [],
    ExploreDescription => "ui.explore_description" [],
    DesktopTitle => "ui.desktop_title" [],
    DesktopDescription => "ui.desktop_description" [],
    QuickStart => "ui.quick_start" [],
    WaitingForFile => "ui.waiting" [],
    ReadOnly => "ui.read_only" [],
    LocalFile => "ui.local_file" [],
    Ready => "ui.ready" [],
    DisplayPanel => "ui.display" [],
    PanelNavigation => "ui.panel_navigation" [],
    ToggleLeftPanel => "ui.toggle_left_panel" [],
    ToggleRightPanel => "ui.toggle_right_panel" [],
    CollapseLeftPanel => "ui.collapse_left_panel" [],
    ExpandLeftPanel => "ui.expand_left_panel" [],
    CollapseRightPanel => "ui.collapse_right_panel" [],
    ExpandRightPanel => "ui.expand_right_panel" [],
    DocumentNavigation => "ui.document_navigation" [],
    SelectedObject => "ui.selected_object" [],
    NothingSelected => "ui.nothing_selected" [],
    LayerFilter => "ui.layer_filter" [],
    LayerTraces => "ui.layer_traces" [],
    LayerVias => "ui.layer_vias" [],
    LayerPads => "ui.layer_pads" [],
    ShowLayer => "ui.show_layer" [],
    HideLayer => "ui.hide_layer" [],
    ShowAllLayers => "ui.show_all" [],
    HideAllLayers => "ui.hide_all" [],
    LayerSettings => "ui.layer_settings" [],
    FileInformation => "ui.file_information" [],
    PropertyIdentifier => "ui.property_identifier" [],
    PropertyHitLayer => "ui.property_hit_layer" [],
    DrillLayer => "ui.drill_layer" [],
    PropertySegments => "ui.property_segments" [],
    PropertyPins => "ui.property_pins" [],
    PropertyVias => "ui.property_vias" [],
    PropertyZones => "ui.property_zones" [],
    PropertyDrawings => "ui.property_drawings" [],
    PropertyLength => "ui.property_length" [],
    PropertyNet => "ui.property_net" [],
    PropertyReference => "ui.property_reference" [],
    PropertyPinName => "ui.property_pin_name" [],
    PropertyPadstack => "ui.property_padstack" [],
    ConnectedObjects => "ui.connected_objects" [],
    PropertyLengthValue => "ui.property_length_value" ["length", "unit"],
    LayerConductor => "ui.layer_conductor" [],
    LayerPlane => "ui.layer_plane" [],
    LayerDielectric => "ui.layer_dielectric" [],
    LayerFunctionUnknown => "ui.layer_function_unknown" [],
    SelectTool => "ui.select_tool" [],
    PanTool => "ui.pan_tool" [],
    LayerColor => "ui.layer_color" [],
    NetColor => "ui.net_color" [],
    FeaturePending => "ui.feature_pending" [],
    FullBoard => "ui.full_board" [],
    BoardSelection => "ui.board_selection" ["name"],
    ToolsMenu => "menu.tools" [],
    HelpMenu => "menu.help" [],

    ViaDefinitionUnsupported => "import.allegro.via_definition_unsupported" ["via", "stack"],
    BondFingerUnsupported => "import.allegro.bond_finger_unsupported" ["finger"],
    BondWireUnsupported => "import.allegro.bond_wire_unsupported" ["track"],
    TrackLayerUndefined => "import.allegro.track_layer_undefined" ["track", "layer"],
    PinDefinitionUnsupported => "import.allegro.pin_definition_unsupported" ["pin", "stack"],
    DieBackUnsupported => "import.allegro.die_back_unsupported" ["pin"],
    FileMenu => "menu.file" [],
    ViewMenu => "menu.view" [],
    NextDocument => "menu.view.next_document" [],
    PreviousDocument => "menu.view.previous_document" [],
    OpenFile => "menu.file.open" [],
    CloseDocument => "menu.file.close" [],
    Quit => "menu.file.quit" [],
    LanguageMenu => "settings.language" [],
    SettingsMenu => "menu.settings" [],
    SettingsTitle => "settings.title" [],
    SettingsTheme => "settings.theme" [],
    SettingsLanguage => "settings.language.label" [],
    SettingsLanguageHint => "settings.language.hint" [],
    SettingsImmediate => "settings.immediate" [],
    SettingsClose => "settings.close" [],
    FollowSystem => "settings.language.system" [],
    LanguageCurrent => "settings.language.current" ["language"],
    LightTheme => "settings.theme.light" [],
    DarkTheme => "settings.theme.dark" [],
    OpenPrompt => "dialog.open_pcb" [],
    WelcomeTitle => "welcome.title" [],
    WelcomeDescription => "welcome.description" [],
    DropHint => "welcome.drop_hint" ["shortcut"],
    DevelopmentStage => "welcome.development_stage" [],
    LocalReadOnly => "status.local_read_only" [],
    CancelImport => "import.cancel" [],
    ImportQueued => "import.queued" [],
    SceneImported => "import.scene.complete" [],
    SceneSummary => "import.scene.summary" ["layers", "segments", "pins", "vias", "zones"],
    ViewportPending => "render.viewport.pending" [],
    ReadingHeader => "import.stage.reading_header" [],
    Reading => "import.stage.reading" [],
    PreparingTracks => "import.stage.preparing_tracks" [],
    PreparingCopper => "import.stage.preparing_copper" [],
    PreparingPads => "import.stage.preparing_pads" [],
    PreparingDrills => "import.stage.preparing_drills" [],
    PreparingTexts => "import.stage.preparing_texts" [],
    BuildingSearch => "import.stage.building_search" [],
    BuildingPicking => "import.stage.building_picking" [],
    ImportBytesKnown => "import.progress.bytes_known" ["completed", "total"],
    ImportBytesUnknown => "import.progress.bytes_unknown" ["completed"],
    ImportItemsKnown => "import.progress.items_known" ["completed", "total"],
    ImportItemsUnknown => "import.progress.items_unknown" ["completed"],
    Indexing => "import.stage.indexing" [],
    Decoding => "import.stage.decoding" [],
    BuildingGeometry => "import.stage.geometry" [],
    PreparingGpu => "import.stage.gpu" [],
    GpuDemoTitle => "render.demo.title" [],
    GpuDemoReady => "render.demo.ready" ["adapter", "backend"],
    GpuDemoReadback => "render.demo.readback" [],
    GpuDemoRerender => "render.demo.rerender" [],
    NativeGpuLegend => "render.native.legend" [],
    NativeGpuTitle => "render.native.title" [],
    NativeGpuPath => "render.native.path" ["adapter"],
    NativeGpuHint => "render.native.hint" [],
    NativeGpuColors => "render.native.colors" [],
    NativeGpuClip => "render.native.clip" [],
    NativeGpuOverlay => "render.native.overlay" [],
    NativeGpuOverlayBody => "render.native.overlay_body" [],
    GpuProbeUsage => "render.probe.usage" [],
    RenderPrepareInvalid => "render.prepare.invalid" ["object"],
    RenderPrepareAllocation => "render.prepare.allocation" [],
    TraceViewportScope => "render.trace.scope" [],
    TraceUpload => "render.trace.upload" ["completed", "total"],
    GpuFailed => "render.failed" [],
    Cancelled => "import.cancelled" [],
    ProbeComplete => "import.header.complete" [],
    HeaderFormat => "import.header.format" ["format", "version", "writer"],
    HeaderSize => "import.header.size" ["megabytes", "count"],
    ScenePending => "import.header.scene_pending" [],
    OutOfBounds => "import.allegro.out_of_bounds" ["offset", "requested", "length"],
    UnsupportedMagic => "import.allegro.unsupported_magic" ["magic"],
    InvalidDivisor => "import.allegro.invalid_divisor" [],
    UnsupportedUnits => "import.allegro.unsupported_units" ["units", "divisor"],
    InvalidGeometry => "import.allegro.invalid_geometry" ["key", "offset"],
    GeometryLimit => "import.geometry_limit" ["actual", "limit"],
    DefaultLayerName => "board.layer.default_name" ["index"],
    SemanticCacheLimit => "import.semantic_cache_limit" ["actual", "limit"],
    PadDimensions => "import.allegro.pad_dimensions" ["stack", "pad_type"],
    PadDonut => "import.allegro.pad_donut" ["stack"],
    PadUnsupported => "import.allegro.pad_unsupported" ["stack", "pad_type", "shape"],
    InvalidEncoding => "import.invalid_encoding" ["offset"],
    DuplicateString => "import.allegro.duplicate_string" ["key", "offset"],
    DuplicateRecord => "import.allegro.duplicate_record" ["key", "offset"],
    UnknownRecord => "import.allegro.unknown_record" ["record_type", "offset"],
    UnalignedRecord => "import.allegro.unaligned_record" ["offset"],
    InvalidRecord => "import.allegro.invalid_record" ["offset"],
    UnsupportedRecordLayout => "import.allegro.unsupported_record_layout" ["record_type", "version", "offset"],
    IndexLimit => "import.index_limit" ["actual", "limit"],
    DecodeLimit => "import.decode_limit" ["actual", "limit"],
    MissingReference => "import.allegro.missing_reference" ["key", "offset"],
    ReferenceType => "import.allegro.reference_type" ["key", "offset", "actual", "expected"],
    ReferenceCycle => "import.allegro.reference_cycle" ["key", "offset"],
    ResourceLimit => "import.resource_limit" ["actual", "limit"],
    SceneNotImplemented => "import.scene_not_implemented" [],
    NoGeometry => "import.allegro.no_geometry" [],
    BondWireLayerName => "board.special.bond_wire" [],
    DiePadLayerName => "board.special.die_pad" [],
    DrawingClassBoard => "board.drawing.class.board" [],
    DrawingClassValue => "board.drawing.class.value" [],
    DrawingClassType => "board.drawing.class.type" [],
    DrawingClassFrame => "board.drawing.class.frame" [],
    DrawingClassManufacturing => "board.drawing.class.manufacturing" [],
    DrawingClassPackage => "board.drawing.class.package" [],
    DrawingClassReference => "board.drawing.class.reference" [],
    DrawingClassTolerance => "board.drawing.class.tolerance" [],
    DrawingClassPartNumber => "board.drawing.class.part_number" [],
    DrawingClassUnknown => "board.drawing.class.unknown" ["class"],
    DrawingSubclassUnknown => "board.drawing.subclass.unknown" ["subclass"],
    DrawingBottomDisplay => "board.drawing.bottom_display" [],
    DrawingTopDisplay => "board.drawing.top_display" [],
    DrawingBottomSilk => "board.drawing.bottom_silk" [],
    DrawingTopSilk => "board.drawing.top_silk" [],
    DrawingBottomAssembly => "board.drawing.bottom_assembly" [],
    DrawingTopAssembly => "board.drawing.top_assembly" [],
    DrawingBottomMask => "board.drawing.bottom_mask" [],
    DrawingTopMask => "board.drawing.top_mask" [],
    DrawingDimension => "board.drawing.dimension" [],
    DrawingAssemblyNotes => "board.drawing.assembly_notes" [],
    DrawingPlatingBar => "board.drawing.plating_bar" [],
    DrawingComponentCenter => "board.drawing.component_center" [],
    DrawingPadstackName => "board.drawing.padstack_name" [],
    DrawingPinNumber => "board.drawing.pin_number" [],
    DrawingBottomPlacement => "board.drawing.bottom_placement" [],
    DrawingTopPlacement => "board.drawing.top_placement" [],
    TextFontTablesMultiple => "import.allegro.text_font_tables_multiple" [],
    TextFontTableInvalid => "import.allegro.text_font_table_invalid" [],
    TextLinkMissing => "import.allegro.text_link_missing" ["key"],
    TextLinkType => "import.allegro.text_link_type" ["key", "record_type"],
    TextContentMissing => "import.allegro.text_content_missing" ["key"],
    TextFontInvalid => "import.allegro.text_font_invalid" ["key", "font"],
    DrawingPathMissing => "import.allegro.drawing_path_missing" ["graphic", "key"],
    DrawingGeometryInvalid => "import.allegro.drawing_geometry_invalid" ["graphic", "key"],
    DrawingOwnerLinkMissing => "import.allegro.drawing_owner_link_missing" ["key"],
    DrawingOwnerMismatch => "import.allegro.drawing_owner_mismatch" ["key"],
    DrawingOrphan => "import.allegro.drawing_orphan" ["key"],
    CopperMeshFailed => "import.allegro.copper_mesh_failed" ["key"],
    CopperBoundaryEmpty => "import.allegro.copper_boundary_empty" ["key"],
    CopperLayerUndefined => "import.allegro.copper_layer_undefined" ["key", "layer"],
    FileNotFound => "file.not_found" [],
    PermissionDenied => "file.permission_denied" [],
    FileIoFailed => "file.io_failed" [],
    FileDialogFailed => "dialog.open_failed" [],
    DocumentIdExhausted => "document.id_exhausted" [],
    WindowFailed => "app.window_failed" [],
    StartupFailureTitle => "app.startup.failure_title" [],
    StartupFailureClose => "app.startup.failure_close" [],
    StartupFailureLog => "app.startup.failure_log" ["path"],
    StartupFailureLogUnavailable => "app.startup.failure_log_unavailable" [],
    StartupPlatformFailed => "app.startup.platform_failed" [],
    StartupFailureOpenLogFolder => "app.startup.open_log_folder" [],
    StartupFailureLogOpenFailed => "app.startup.log_open_failed" ["path"],
    ConfigInvalid => "settings.config_invalid" [],
    ConfigSaveFailed => "settings.config_save_failed" [],
    ConfigDirectoryMissing => "settings.config_directory_missing" [],
    TechnicalDetails => "diagnostic.technical_details" [],
    ImportDiagnostics => "diagnostic.import_count" ["count"],
    DiagnosticInfo => "diagnostic.info" [],
    DiagnosticWarning => "diagnostic.warning" [],
    DiagnosticError => "diagnostic.error" [],
    DiagnosticOffset => "diagnostic.offset" ["offset"],
    DiagnosticObject => "diagnostic.object" ["id"],
    DiagnosticPage => "diagnostic.page" ["current", "total"],
    RenderTextsUnavailable => "render.texts_unavailable" ["count"],
    RenderTextMissingGlyph => "render.text_missing_glyph" ["character"],
    RenderTextCharacterLimit => "render.text_character_limit" [],
    RenderTextStrokeLimit => "render.text_stroke_limit" [],
    RenderTextStrokeBudget => "render.text_stroke_budget" ["prepared", "limit", "objects"],
    RenderTextInvalidGeometry => "render.text_invalid_geometry" [],
    RenderFontLimit => "render.font_limit" [],
    RenderFontResourceInvalid => "render.font_resource_invalid" [],
    RenderFontDuplicate => "render.font_duplicate" ["character"],
    RenderFontInvalid => "render.font_invalid" ["character"],
    RenderDrawingsUnavailable => "render.drawings_unavailable" ["count"],
    MessageInvalid => "diagnostic.message_invalid" ["key"],
    CliUsage => "cli.usage" [],
    CliMissingOption => "cli.missing_option" ["option"],
    CliMissingValue => "cli.missing_value" ["option"],
    CliUnsupportedLocale => "cli.unsupported_locale" ["locale"],
    CliUnsupportedEncoding => "cli.unsupported_encoding" ["encoding"],
    StartupInvalidOption => "app.startup.invalid_option" ["option"],
    SearchNets => "view.search_nets" [],
    SearchComponents => "view.search_components" [],
    SearchNetResult => "view.search_net_result" ["name", "count"],
    SearchComponentResult => "view.search_component_result" ["name", "count"],
    ModeObject => "view.mode_object" [],
    ModeTrack => "view.mode_track" [],
    ModeNet => "view.mode_net" [],
    ModeComponent => "view.mode_component" [],
    ClearSelection => "view.clear_selection" [],
    LocateSelected => "view.locate_selected" ["kind"],
    CloseNamedDocument => "view.close_named_document" ["name"],
    InspectorEmpty => "view.inspector_empty" [],
    SelectionSegments => "view.selection_segments" ["count"],
    SelectionPins => "view.selection_pins" ["count"],
    SelectionVias => "view.selection_vias" ["count"],
    SelectionZones => "view.selection_zones" ["count"],
    SelectionLengthScope => "view.selection_length_scope" [],
    Inspector => "view.inspector" [],
    SelectionStats => "view.selection_stats" ["segments", "pins", "vias", "zones"],
    SelectionLength => "view.selection_length" ["length"],
    SearchBoard => "view.search" [],
    Locating => "view.locating" [],
    Selecting => "view.selecting" [],
    PickCycleHint => "view.pick_cycle_hint" [],
    PickCandidatePosition => "view.pick_candidate_position" ["current", "total"],
    SelectionIdentity => "view.selection_identity" ["kind", "id"],
    SourceSegment => "view.source_segment" [],
    PickCategories => "view.pick_categories" [],
    PickNoneEnabled => "view.pick_none_enabled" [],
    PickTrackDisabled => "view.pick_track_disabled" [],
    PickComponentDisabled => "view.pick_component_disabled" [],
    SourcePin => "view.source_pin" [],
    SourceVia => "view.source_via" [],
    SourceZone => "view.source_zone" [],
    SourceDrawing => "view.source_drawing" [],
    SourceText => "view.source_text" ["name"],
    PickDrawings => "view.pick_drawings" [],
    SourceReference => "view.source_reference" ["name"],
    SourcePinName => "view.source_pin_name" ["name"],
    SourceBondPin => "view.source_bond_pin" ["id"],
    SourceBondFinger => "view.source_bond_finger" ["id"],
    SourceBondProfile => "view.source_bond_profile" ["name"],
    SourceBondMaterial => "view.source_bond_material" ["name"],
    SourceFingerName => "view.source_finger_name" ["name"],
    SourceNetName => "view.source_net_name" ["name"],
    SourcePadstackName => "view.source_padstack_name" ["name"],
    SourcePosition => "view.source_position" ["x", "y"],
    SourceStart => "view.source_start" ["x", "y"],
    SourceEnd => "view.source_end" ["x", "y"],
    SourceDrillSize => "view.source_drill_size" ["width", "height"],
    SourceTraceWidth => "view.source_trace_width" ["width"],
    SourcePositionMils => "view.source_position_mils" ["x", "y"],
    SourceStartMils => "view.source_start_mils" ["x", "y"],
    SourceEndMils => "view.source_end_mils" ["x", "y"],
    SourceDrillSizeMils => "view.source_drill_size_mils" ["width", "height"],
    SourceTraceWidthMils => "view.source_trace_width_mils" ["width"],
    SourceBackdrillDisplayDiameterMils => "view.source_backdrill_display_diameter_mils" ["diameter"],
    SourceRotation => "view.source_rotation" ["degrees"],
    SourceMirrored => "view.source_mirrored" [],
    SourceUnmirrored => "view.source_unmirrored" [],
    SourceLayerId => "view.source_layer_id" ["id"],
    SourceLayerName => "view.source_layer_name" ["id", "name"],
    InspectRelatedNet => "view.inspect_related_net" ["id"],
    InspectRelatedComponent => "view.inspect_related_component" ["id"],
    SourceMembersShown => "view.source_members_shown" ["shown", "total"],
    MembersPage => "view.members_page" ["page", "pages"],
    MembersPrevious => "view.members_previous" [],
    MembersNext => "view.members_next" [],
    LoadingMembers => "view.loading_members" [],
    MembersFailed => "view.members_failed" [],
    MembersInvalid => "view.members_invalid" ["object"],
    LocateMember => "view.locate_member" ["kind", "id"],
    SourceBackdrillReference => "view.source_backdrill_reference" ["id"],
    SourceBackdrillDisplayDiameter => "view.source_backdrill_display_diameter" ["diameter"],
    SourceBackdrillSpan => "view.source_backdrill_span" ["start", "stop", "protected"],
    PickFailed => "view.pick_failed" [],
    RetryImport => "app.retry_import" [],
    RetryEncoding => "app.retry_encoding" [],
    RetryEncodingHint => "app.retry_encoding_hint" [],
    RecentFiles => "app.recent_files" [],
    RecentFilesEmpty => "app.recent_files_empty" [],
    RemoveRecent => "app.remove_recent" [],
    RelocateRecent => "app.relocate_recent" [],
    RelocatePrompt => "app.relocate_prompt" [],
    RequestGenerationExhausted => "app.request_generation_exhausted" [],
    HoverObject => "view.hover_object" ["kind", "id"],
    HoverInvalid => "view.hover_invalid" ["object"],
    HoverFailed => "view.hover_failed" [],
    PickInvalid => "view.pick_invalid" ["object"],
    SearchLocateInvalid => "view.locate_invalid" ["object"],
    SearchLocateEmpty => "view.locate_empty" [],
    SearchLocateFailed => "view.locate_failed" [],
    Searching => "view.searching" [],
    SearchCollationUnavailable => "view.search_collation_unavailable" ["locale"],
    NoSearchResults => "view.search_empty" [],
    Layers => "view.layers" [],
    ShowDrills => "view.show_drills" [],
    ShowBackdrills => "view.show_backdrills" [],
    FillPads => "view.fill_pads" [],
    ShowCopper => "view.show_copper" [],
    ShowTexts => "view.show_texts" [],
    ShowDrawings => "view.show_drawings" [],
    ReloadDocument => "document.reload" [],
    LayerToTop => "view.layer_to_top" [],
    LayerToBottom => "view.layer_to_bottom" [],
    ResetLayerOrder => "view.reset_layer_order" [],
    ScaleDistance => "view.scale_distance" ["distance"],
    ScaleDistanceMils => "view.scale_distance_mils" ["distance"],
    SelectionLengthMils => "view.selection_length_mils" ["length"],
    UnitMillimeters => "view.unit_millimeters" [],
    UnitMils => "view.unit_mils" [],
    CopperOpacity => "view.copper_opacity" ["percent"],
    CopperOpacityLabel => "view.copper_opacity_label" [],
    PickCopper => "view.pick_copper" [],
    DecreaseOpacity => "view.decrease_opacity" [],
    IncreaseOpacity => "view.increase_opacity" [],
    FitBoard => "view.fit" [],
    FlipBoard => "view.flip" [],
    ZoomIn => "view.zoom_in" [],
    ZoomOut => "view.zoom_out" [],
    NavigationHint => "view.navigation_hint" [],
    CliUnsupportedStage => "cli.unsupported_stage" [],
    CliNoCases => "cli.no_cases" ["path"],
    CliProbesFailed => "cli.probes_failed" ["count"],
    CliOperationFailed => "cli.operation_failed" ["operation", "path"],
    CliSerializeFailed => "cli.serialize_failed" [],
    CliDecodeRequestInvalid => "cli.decode_request_invalid" [],
    CliSourceMismatch => "cli.source_mismatch" [],
}

/// Typed values remain machine-readable; complete sentences are never arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageArg {
    Text(String),
    Unsigned(u64),
    Signed(i64),
    Boolean(bool),
}

impl fmt::Display for MessageArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => value.fmt(f),
            Self::Unsigned(value) => value.fmt(f),
            Self::Signed(value) => value.fmt(f),
            Self::Boolean(value) => value.fmt(f),
        }
    }
}

impl From<String> for MessageArg {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<&str> for MessageArg {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl From<u64> for MessageArg {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}
impl From<usize> for MessageArg {
    fn from(value: usize) -> Self {
        Self::Unsigned(value as u64)
    }
}
impl From<u32> for MessageArg {
    fn from(value: u32) -> Self {
        Self::Unsigned(u64::from(value))
    }
}
impl From<u16> for MessageArg {
    fn from(value: u16) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

/// A retained message can be rendered again after changing language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub key: MessageKey,
    pub args: BTreeMap<String, MessageArg>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MessageError {
    #[error("I18N_ARGUMENT_MISSING {0}")]
    MissingArgument(&'static str),
    #[error("I18N_ARGUMENT_UNEXPECTED {0}")]
    UnexpectedArgument(String),
}

impl Message {
    pub fn new(key: MessageKey) -> Self {
        Self {
            key,
            args: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, name: impl Into<String>, value: impl Into<MessageArg>) -> Self {
        self.args.insert(name.into(), value.into());
        self
    }

    pub fn render(&self, locale: Locale) -> Result<String, MessageError> {
        for &name in self.key.parameters() {
            if !self.args.contains_key(name) {
                return Err(MessageError::MissingArgument(name));
            }
        }
        for name in self.args.keys() {
            if !self.key.parameters().contains(&name.as_str()) {
                return Err(MessageError::UnexpectedArgument(name.clone()));
            }
        }
        let template = rust_i18n::t!(self.key.as_str(), locale = locale.tag());
        let names: Vec<_> = self.args.keys().map(String::as_str).collect();
        let values: Vec<_> = self.args.values().map(ToString::to_string).collect();
        // A single substitution pass keeps literal "%{...}" in source names intact.
        Ok(rust_i18n::replace_patterns(&template, &names, &values))
    }

    /// Parameter faults produce a translated recovery message, never raw keys.
    pub fn display(&self, locale: Locale) -> String {
        self.render(locale).unwrap_or_else(|_| {
            rust_i18n::t!(
                "diagnostic.message_invalid",
                locale = locale.tag(),
                key = self.key.as_str()
            )
            .into_owned()
        })
    }
}

pub fn text(locale: Locale, key: MessageKey) -> String {
    Message::new(key).display(locale)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unknown_locale_uses_the_english_catalog() {
        assert_eq!(
            rust_i18n::t!(
                "import.allegro.invalid_divisor",
                locale = "unsupported-test"
            ),
            rust_i18n::t!("import.allegro.invalid_divisor", locale = "en")
        );
    }
}
