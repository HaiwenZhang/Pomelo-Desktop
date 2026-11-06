//! Three-dimensional source coordinates in millimeters, before display transforms.

pub mod drill;
pub mod solid;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Bounds3 {
    pub fn is_valid(self) -> bool {
        (0..3).all(|i| {
            self.min[i].is_finite()
                && self.max[i].is_finite()
                && self.min[i] <= self.max[i]
                && (self.max[i] - self.min[i]).is_finite()
        })
    }

    pub fn center(self) -> [f64; 3] {
        std::array::from_fn(|i| self.min[i] * 0.5 + self.max[i] * 0.5)
    }

    pub fn corners(self) -> [[f64; 3]; 8] {
        std::array::from_fn(|corner| {
            std::array::from_fn(|axis| {
                if corner & (1 << axis) == 0 {
                    self.min[axis]
                } else {
                    self.max[axis]
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_reject_invalid_coordinates_and_unrepresentable_extents() {
        for bounds in [
            Bounds3 {
                min: [0.0; 3],
                max: [f64::NAN; 3],
            },
            Bounds3 {
                min: [1.0; 3],
                max: [0.0; 3],
            },
            Bounds3 {
                min: [-f64::MAX; 3],
                max: [f64::MAX; 3],
            },
        ] {
            assert!(!bounds.is_valid());
        }
        let bounds = Bounds3 {
            min: [1.0, 2.0, 3.0],
            max: [5.0, 8.0, 3.0],
        };
        assert!(bounds.is_valid());
        assert_eq!(bounds.center(), [3.0, 5.0, 3.0]);
        assert_eq!(bounds.corners()[0], bounds.min);
        assert_eq!(bounds.corners()[7], bounds.max);
    }
}
