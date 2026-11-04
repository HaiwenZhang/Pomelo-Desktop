//! Screen-space fill patterns; geometry, coverage masks and source data stay intact.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum FillPattern {
    #[default]
    Solid,
    DiagonalDown,
    DiagonalUp,
    Horizontal,
    Vertical,
    SparseDots,
    Dots,
    DenseDots,
    VerticalDashes,
    Crosshatch,
    SquareGrid,
    Checkerboard,
    Rings,
    Diamonds,
    Crosses,
    DenseCheckerboard,
}
impl FillPattern {
    pub const ALL: [Self; 16] = [
        Self::Solid,
        Self::DiagonalDown,
        Self::DiagonalUp,
        Self::Horizontal,
        Self::Vertical,
        Self::SparseDots,
        Self::Dots,
        Self::DenseDots,
        Self::VerticalDashes,
        Self::Crosshatch,
        Self::SquareGrid,
        Self::Checkerboard,
        Self::Rings,
        Self::Diamonds,
        Self::Crosses,
        Self::DenseCheckerboard,
    ];
    pub const fn ink(self, x: u32, y: u32) -> bool {
        let x = x & 15;
        let y = y & 15;
        match self {
            Self::Solid => true,
            Self::DiagonalDown => (x + 16 - y).is_multiple_of(4),
            Self::DiagonalUp => (x + y).is_multiple_of(4),
            Self::Horizontal => y.is_multiple_of(4),
            Self::Vertical => x.is_multiple_of(4),
            Self::Crosshatch => (x + y).is_multiple_of(8) || (x + 16 - y).is_multiple_of(8),
            Self::SparseDots => x.is_multiple_of(8) && y.is_multiple_of(8),
            Self::Dots => x.is_multiple_of(4) && y.is_multiple_of(4),
            Self::VerticalDashes => x.is_multiple_of(4) && y % 8 < 4,
            Self::DenseDots => x.is_multiple_of(2) && y.is_multiple_of(2),
            Self::DenseCheckerboard => (x + y).is_multiple_of(2),
            Self::SquareGrid => x.is_multiple_of(4) || y.is_multiple_of(4),
            Self::Rings => {
                let dx = x as i32 % 8 - 4;
                let dy = y as i32 % 8 - 4;
                let d = dx * dx + dy * dy;
                d >= 5 && d <= 10
            }
            Self::Diamonds => {
                let dx = (x as i32 % 8 - 4).abs();
                let dy = (y as i32 % 8 - 4).abs();
                dx + dy == 3
            }
            Self::Crosses => {
                (x % 8 == 4 && y % 8 >= 2 && y % 8 <= 6) || (y % 8 == 4 && x % 8 >= 2 && x % 8 <= 6)
            }
            Self::Checkerboard => (x / 2 + y / 2).is_multiple_of(2),
        }
    }
    pub fn rows(self) -> [u32; 16] {
        std::array::from_fn(|y| {
            (0..16).fold(0, |row, x| row | (u32::from(self.ink(x, y as u32)) << x))
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_are_periodic_nonempty_and_stay_in_sixteen_bits() {
        for pattern in FillPattern::ALL {
            let rows = pattern.rows();
            assert!(rows.iter().any(|&row| row != 0));
            assert!(rows.iter().all(|row| row & !0xffff == 0));
            for y in 0..16 {
                for x in 0..16 {
                    assert_eq!(
                        (rows[y as usize] >> x) & 1,
                        u32::from(pattern.ink(x + 16, y + 32))
                    );
                }
            }
            if pattern != FillPattern::Solid {
                assert!(rows.iter().any(|row| *row != 0xffff));
            }
        }
    }
}
