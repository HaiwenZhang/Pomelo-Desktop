//! Extend the component bundle with only the Lucide icons used by Pomelo.
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;
gpui_kit::assets::icon_assets!(
    ProductIcons,
    [
        CircuitBoard,
        Clock,
        FilePlus,
        LockKeyhole,
        Layers,
        Keyboard,
        X,
        Hand,
        MousePointer2,
        Scan,
        RotateCcw,
        Minus,
        Plus,
        ArrowUp,
        ArrowRight,
        Eye,
        EyeOff,
        Ellipsis,
        Cpu,
        File,
        Ruler,
        Spline,
        MoveHorizontal,
        SquareDashedMousePointer,
        ZoomIn,
        Undo2,
        Redo2,
        BookmarkPlus,
        Highlighter,
        Camera,
        Image,
        FileDown,
        Trash,
        Pencil,
        Locate,
        ListPlus,
        ListX,
        ChevronLeft,
        ChevronRight,
        ListFilter,
        ScanSearch,
        SlidersHorizontal,
        Waypoints,
        Lock,
        FlipHorizontal2,
        MousePointerClick,
        Crosshair,
        CircleDot,
        CircleDashed,
        ListRestart,
        Package,
        ArrowDownAZ,
        ArrowDownWideNarrow,
        PenLine
    ]
);
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ProductIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ProductIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
